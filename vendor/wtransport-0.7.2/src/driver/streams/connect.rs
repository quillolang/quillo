use tracing::debug;
use wtransport_proto::bytes::IoReadError;
use wtransport_proto::capsule::capsules;
use wtransport_proto::capsule::Capsule;
use wtransport_proto::capsule::{self};
use wtransport_proto::error::ErrorCode;
use wtransport_proto::frame::FrameKind;
use wtransport_proto::varint::VarInt;

use super::session::StreamSession;
use super::ProtoReadError;
use crate::driver::DriverError;
use crate::error::ApplicationClose;
use std::future::pending;
// Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): for `ReadingFrame`.
use std::future::Future;
use std::pin::Pin;
use wtransport_proto::frame::Frame;

pub struct ConnectStream {
    stream: Option<StreamSession>,
    // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): the frame being read, which
    // holds `stream` until it is whole (see `read_frame`).
    reading: Option<ReadingFrame>,
}

// Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): a read of one frame that owns its
// stream and gives it back with the frame.
type ReadingFrame =
    Pin<Box<dyn Future<Output = (StreamSession, Result<Frame<'static>, ProtoReadError>)> + Send>>;

impl ConnectStream {
    pub fn empty() -> Self {
        Self {
            stream: None,
            reading: None, // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89)
        }
    }

    pub fn is_empty(&self) -> bool {
        // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): a stream in the middle of a
        // frame is held by `reading`.
        self.stream.is_none() && self.reading.is_none()
    }

    pub fn set_stream(&mut self, stream: StreamSession) {
        self.stream = Some(stream);
    }

    pub async fn run(&mut self) -> DriverError {
        loop {
            // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): through `read_frame`,
            // which keeps a frame begun when the driver's `select!` drops this future.
            return match self.read_frame().await {
                Ok(frame) => {
                    if !matches!(frame.kind(), FrameKind::Data) {
                        debug!("Skipping non-data frame of kind {:?}", frame.kind());
                        continue;
                    }

                    let capsule = match Capsule::with_frame(&frame) {
                        Some(capsule)
                            if matches!(
                                capsule.kind(),
                                capsule::CapsuleKind::CloseWebTransportSession
                            ) =>
                        {
                            capsule
                        }
                        // Unknown capsule, skip it
                        _ => {
                            debug!(
                                "Unknown capsule of kind {:?} of {}B",
                                frame.kind(),
                                frame.payload().len()
                            );

                            continue;
                        }
                    };

                    let close_session =
                        match capsules::CloseWebTransportSession::with_capsule(&capsule) {
                            Ok(close_session) => close_session,
                            Err(error_code) => return DriverError::Proto(error_code),
                        };

                    // reset right away to avoid receiving additional data which requires resetting with ErrorCode::Message.
                    self.stream
                        .take()
                        .unwrap()
                        .reset(ErrorCode::NoError.to_code());

                    DriverError::ApplicationClosed(ApplicationClose::new(
                        close_session.error_code(),
                        close_session
                            .reason()
                            .as_bytes()
                            .to_vec()
                            .into_boxed_slice(),
                    ))
                }
                Err(ProtoReadError::H3(error_code)) => DriverError::Proto(error_code),
                Err(ProtoReadError::IO(io_error)) => match io_error {
                    // Cleanly terminating a CONNECT stream without a CLOSE_WEBTRANSPORT_SESSION
                    // capsule SHALL be semantically equivalent to terminating it with a
                    // CLOSE_WEBTRANSPORT_SESSION capsule that has an error code of 0 and an empty error string.
                    IoReadError::ImmediateFin => DriverError::ApplicationClosed(
                        ApplicationClose::new(VarInt::from_u32(0), Box::new([])),
                    ),
                    IoReadError::UnexpectedFin | IoReadError::Reset => {
                        DriverError::Proto(ErrorCode::ClosedCriticalStream)
                    }
                    IoReadError::NotConnected => DriverError::NotConnected,
                },
            };
        }
    }

    // Quillo patch (FEEDBACK75.md, Q-1124; branch feedback86-89): as
    // `RemoteSettingsStream::read_frame`. `run` is polled inside the driver's `select!`, which
    // drops it whenever another branch is ready first, and a frame read straight from the stream
    // lost the bytes it had taken: a capsule split across packets was read again from its
    // middle. The read lives in `reading`, which owns the stream until the frame is whole.
    async fn read_frame(&mut self) -> Result<Frame<'static>, ProtoReadError> {
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
        result
    }
}
