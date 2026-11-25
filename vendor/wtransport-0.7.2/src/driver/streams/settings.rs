use crate::driver::streams::unilocal::StreamUniLocalH3;
use crate::driver::streams::uniremote::StreamUniRemoteH3;
use crate::driver::streams::ProtoReadError;
use crate::driver::streams::ProtoWriteError;
use crate::driver::DriverError;
use crate::error::StreamWriteError;
use crate::VarInt;
use std::future::pending;
// Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): for `ReadingFrame`.
use std::future::Future;
use std::pin::Pin;
use tokio::sync::watch;
use wtransport_proto::bytes;
use wtransport_proto::error::ErrorCode;
use wtransport_proto::frame::Frame;
use wtransport_proto::frame::FrameKind;
use wtransport_proto::settings::Settings;
use wtransport_proto::stream_header::StreamKind;

pub struct LocalSettingsStream {
    stream: Option<StreamUniLocalH3>,
    settings: Settings,
}

impl LocalSettingsStream {
    pub fn empty() -> Self {
        let settings = Settings::builder()
            .qpack_max_table_capacity(VarInt::from_u32(0))
            .qpack_blocked_streams(VarInt::from_u32(0))
            .enable_connect_protocol() // TODO(biagio): it would be nice to have this only for server
            .enable_webtransport()
            .enable_h3_datagrams()
            .webtransport_max_sessions(VarInt::from_u32(1))
            .build();

        Self {
            stream: None,
            settings,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.stream.is_none()
    }

    pub fn set_stream(&mut self, stream: StreamUniLocalH3) {
        assert!(matches!(stream.kind(), StreamKind::Control));
        self.stream = Some(stream);
    }

    pub async fn send_settings(&mut self) -> Result<(), DriverError> {
        match self
            .stream
            .as_mut()
            .expect("Cannot send settings on empty stream")
            .write_frame(self.settings.generate_frame())
            .await
        {
            Ok(()) => Ok(()),
            Err(ProtoWriteError::NotConnected) => Err(DriverError::NotConnected),
            Err(ProtoWriteError::Stopped) => {
                Err(DriverError::Proto(ErrorCode::ClosedCriticalStream))
            }
        }
    }

    pub async fn run(&mut self) -> DriverError {
        match self.stream.as_mut() {
            Some(stream) => match stream.stopped().await {
                StreamWriteError::NotConnected => DriverError::NotConnected,
                StreamWriteError::Closed | StreamWriteError::Stopped(_) => {
                    DriverError::Proto(ErrorCode::ClosedCriticalStream)
                }
                StreamWriteError::QuicProto => DriverError::Proto(ErrorCode::ClosedCriticalStream),
            },
            None => pending().await,
        }
    }
}

pub struct RemoteSettingsStream {
    stream: Option<StreamUniRemoteH3>,
    settings: watch::Sender<Option<Settings>>,
    // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): the frame being read, which
    // holds `stream` until it is whole (see `read_frame`).
    reading: Option<ReadingFrame>,
}

// Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): a read of one frame that owns its
// stream and gives it back with the frame.
type ReadingFrame = Pin<
    Box<dyn Future<Output = (StreamUniRemoteH3, Result<Frame<'static>, ProtoReadError>)> + Send>,
>;

impl RemoteSettingsStream {
    pub fn empty() -> Self {
        Self {
            stream: None,
            settings: watch::channel(None).0,
            reading: None, // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89)
        }
    }

    pub fn is_empty(&self) -> bool {
        // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): a stream in the middle of a
        // frame is held by `reading`.
        self.stream.is_none() && self.reading.is_none()
    }

    pub fn set_stream(&mut self, stream: StreamUniRemoteH3) {
        assert!(matches!(stream.kind(), StreamKind::Control));
        self.stream = Some(stream);
    }

    pub fn subscribe(&self) -> RemoteSettingsWatcher {
        RemoteSettingsWatcher(self.settings.subscribe())
    }

    pub async fn run(&mut self) -> DriverError {
        loop {
            let frame = match self.read_frame().await {
                Ok(frame) => frame,
                Err(driver_error) => return driver_error,
            };

            if self.settings.borrow().is_none() {
                if !matches!(frame.kind(), FrameKind::Settings) {
                    return DriverError::Proto(ErrorCode::MissingSettings);
                }

                let settings = match Settings::with_frame(&frame) {
                    Ok(settings) => settings,
                    Err(error_code) => return DriverError::Proto(error_code),
                };

                self.settings.send_replace(Some(settings));
            } else if !matches!(frame.kind(), FrameKind::Exercise(_)) {
                return DriverError::Proto(ErrorCode::FrameUnexpected);
            }
        }
    }

    // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): `run` is polled inside the
    // driver's `select!`, which drops it whenever another branch is ready first (a stream or a
    // datagram arriving). A frame read straight from the stream lost the bytes it had already
    // taken when that happened, so a SETTINGS frame that came in two packets with a new stream
    // between was read again from its middle: the connection closed (H3_FRAME_UNEXPECTED,
    // H3_EXCESSIVE_LOAD, H3_MISSING_SETTINGS) or the session never started. The read now lives in
    // `reading`, which owns the stream until the frame is whole, and the next call resumes it.
    async fn read_frame(&mut self) -> Result<Frame<'static>, DriverError> {
        if self.reading.is_none() {
            let Some(mut stream) = self.stream.take() else {
                return pending().await;
            };
            self.reading = Some(Box::pin(async move {
                let result = stream.read_frame().await;
                (stream, result)
            }));
        }

        let (stream, result) = self.reading.as_mut().expect("a frame is being read").await;
        self.reading = None;
        self.stream = Some(stream);

        match result {
            Ok(frame) => Ok(frame),
            Err(ProtoReadError::H3(error_code)) => Err(DriverError::Proto(error_code)),
            Err(ProtoReadError::IO(io_error)) => match io_error {
                bytes::IoReadError::ImmediateFin
                | bytes::IoReadError::UnexpectedFin
                | bytes::IoReadError::Reset => {
                    Err(DriverError::Proto(ErrorCode::ClosedCriticalStream))
                }
                bytes::IoReadError::NotConnected => Err(DriverError::NotConnected),
            },
        }
    }
}

pub struct RemoteSettingsWatcher(watch::Receiver<Option<Settings>>);

impl RemoteSettingsWatcher {
    pub async fn accept_settings(&mut self) -> Option<Settings> {
        self.0.changed().await.ok()?;

        Some(
            self.0
                .borrow()
                .clone()
                .expect("On change settings must be set"),
        )
    }
}
