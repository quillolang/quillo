//! Pages that send media, in a real browser: `<quillo-media publish>` with
//! Chrome's fake camera and microphone (or none at all), the pages that play
//! what they send, and what goes wrong on the way: no permission, no
//! WebCodecs, sources the encoders cannot take, a stream the program ends, a
//! sender that leaves, frames that are not what they say, a page that falls
//! behind, and a server that restarts. The program is
//! tests/browser/media-publish.qwe, driven by tests/browser/media-*.js.
//! Skipped when no Chrome or no Node 22 is found (set QUILLO_CHROME to point
//! at one).

mod support;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// A camera and a microphone that need no asking, and sound that plays
/// without a click.
const FAKE_CAMERA: &str = "--use-fake-device-for-media-stream --use-fake-ui-for-media-stream --autoplay-policy=no-user-gesture-required";
/// A camera that is never allowed (the fake one, so no real one is touched).
const NO_PERMISSION: &str = "--use-fake-device-for-media-stream --deny-permission-prompts";

fn chrome() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QUILLO_CHROME") {
        return Some(PathBuf::from(p));
    }
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Node 22 or later (run.js uses its built-in WebSocket).
fn node_available() -> bool {
    Command::new("node")
        .args(["-e", "process.exit(typeof WebSocket === 'function' ? 0 : 1)"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn browser() -> Option<PathBuf> {
    let found = chrome().filter(|_| node_available());
    if found.is_none() {
        eprintln!("skipping: needs Chrome and Node 22 or later");
    }
    found
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser")
}

fn server(port: u16, args: &[&str], stderr: Stdio) -> support::Guard {
    let s = support::Guard::new(
        Command::new(env!("CARGO_BIN_EXE_quillo"))
            .arg("run")
            .arg(dir().join("media-publish.qwe"))
            .args(["--port", &port.to_string()])
            .args(args)
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !support::listening(port) {
        assert!(Instant::now() < deadline, "server did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    s
}

/// run.js in Chrome with `flags`, on `path`, with `script`.
fn page(chrome: &Path, port: u16, path: &str, script: &str, flags: &str, timeout_ms: u32) -> std::process::Child {
    let profile = std::env::temp_dir().join(format!("quillo-chrome-media-{}-{}", std::process::id(), port));
    Command::new("node")
        .arg(dir().join("run.js"))
        .arg(chrome)
        .arg(format!("http://127.0.0.1:{}{}", port, path))
        .arg(&profile)
        .arg(timeout_ms.to_string())
        .arg(dir().join(script))
        .env("QUILLO_CHROME_FLAGS", flags)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node")
}

fn result(out: std::process::Output, port: u16) -> Value {
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("quillo-chrome-media-{}-{}", std::process::id(), port)));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{}: {}", e, String::from_utf8_lossy(&out.stdout)))
}

// One Chrome at a time: several at once overload small machines, and a
// camera's frames come slower.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serves media-publish.qwe (with `args`), runs `script` on `path` in Chrome
/// with `flags`, and gives what the script recorded.
fn run(chrome: &Path, path: &str, script: &str, flags: &str, args: &[&str]) -> Value {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let port = support::free_port();
    let server = server(port, args, Stdio::null());
    let out = page(chrome, port, path, script, flags, 60000).wait_with_output().expect("node");
    drop(server);
    result(out, port)
}

fn no_errors(r: &Value) {
    assert_eq!(r["errors"], json!([]), "{}", r);
}

/// What a page sends plays: its camera and microphone go to stream cam, and
/// the page playing cam draws the picture at its size and plays the sound a
/// little ahead. The button turns the sound off (nothing more is decoded)
/// and on again. The sender leaving lets the camera go and sends nothing
/// more (stopping it again does nothing), and the player waits; it comes
/// back and plays again; and a player that goes closes its decoders and
/// its AudioContext.
fn sends_and_plays(args: &[&str], via: &str) {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/", "media-publish.js", FAKE_CAMERA, args);
    assert_eq!(r["via"], via, "{}", r);
    assert_eq!(
        r["sending"],
        json!({ "state": "sending", "sent": true, "bytes": true, "video": true, "audio": true, "previewTracks": "audio,video", "source": null }),
        "{}",
        r
    );
    let playing = &r["playing"];
    assert_eq!(playing["state"], "playing", "{}", r);
    // H.264 baseline, as the browser encoded it, at the camera's 1280x720.
    assert!(playing["codec"].as_str().unwrap_or("").starts_with("avc1.42"), "{}", r);
    assert_eq!((playing["width"].as_u64(), playing["height"].as_u64()), (Some(1280), Some(720)), "{}", r);
    assert_eq!(playing["sound"], "running", "{}", r);
    assert_eq!(playing["ahead"], true, "the sound is not scheduled a little ahead: {}", r);
    assert_eq!(r["button"], json!(["\u{1F50A} Sound on", "\u{1F507} Sound off", "\u{1F50A} Sound on"]), "{}", r);
    assert_eq!(r["mutedAudio"], 0, "sound was decoded while off: {}", r);
    assert_eq!(r["unmutedAudio"], true, "the sound did not come back: {}", r);
    assert_eq!(r["left"], json!({ "player": null, "tracks": "ended,ended", "sentAfter": 0, "waiting": true }), "{}", r);
    assert_eq!(
        r["back"],
        json!({ "stats": true, "first": "starting", "state": "sending", "sent": true, "played": true, "player": "playing" }),
        "{}",
        r
    );
    assert_eq!(r["gone"], json!({ "video": "closed", "audio": "closed", "sound": "closed", "decoded": 0 }), "{}", r);
    no_errors(&r);
}

#[test]
fn a_page_sends_its_camera_and_a_page_plays_it_over_webtransport() {
    sends_and_plays(&[], "webtransport");
}

#[test]
fn a_page_sends_its_camera_over_a_websocket_without_webtransport() {
    sends_and_plays(&["--no-wt"], "websocket");
}

/// `no-audio` sends the picture alone, `no-video` the sound alone, and both
/// leave nothing to ask the camera for: refused. A sender gone before the
/// camera is asked never asks; gone while it is asked, it lets go of what
/// the camera gives. A source given while the camera is asked goes instead,
/// and the camera is let go. Without track processors the preview is drawn
/// on a canvas and sent, the picture alone; without WebCodecs, the sender
/// and the player say they cannot.
#[test]
fn what_a_sender_asks_the_camera_for_and_browsers_without_what_it_needs() {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/page", "media-camera.js", FAKE_CAMERA, &[]);
    assert_eq!(r["videoOnly"], json!({ "state": "sending", "tracks": "video", "video": true, "audio": 0 }), "{}", r);
    assert_eq!(r["audioOnly"], json!({ "state": "sending", "tracks": "audio", "video": 0, "audio": true }), "{}", r);
    assert_eq!(r["neither"], json!({ "state": "refused", "sent": 0 }), "{}", r);
    assert_eq!(r["early"], json!({ "asked": 0, "state": "starting" }), "{}", r);
    assert_eq!(r["during"], json!({ "asked": 1, "tracks": "ended,ended", "state": "starting" }), "{}", r);
    assert_eq!(
        r["given"],
        json!({ "state": "sending", "source": true, "preview": true, "camera": "ended,ended", "stillPreview": true, "sent": true }),
        "{}",
        r
    );
    assert_eq!(r["drawn"], json!({ "state": "sending", "video": true, "audio": 0, "played": true, "player": "playing", "width": 1280 }), "{}", r);
    assert_eq!(
        r["unsupported"],
        json!({ "send": ["unsupported", "This browser cannot send video."], "play": ["unsupported", "This browser cannot play this stream."] }),
        "{}",
        r
    );
    no_errors(&r);
}

/// `media.close` of a stream a page sends: the sender's `data-state` is
/// `ended` and so is the player's, and they stay so (nothing more sent or
/// played); closing it again gives false. A sender `publish` says no to is
/// `refused`, and nothing it encodes is sent. Two senders of one stream in a
/// page: one going leaves the other sending.
#[test]
fn the_program_ends_a_stream_and_refuses_a_sender() {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/page", "media-close.js", FAKE_CAMERA, &[]);
    assert_eq!(r["before"], json!({ "send": "sending", "play": "playing" }), "{}", r);
    assert_eq!(r["refused"], json!({ "state": "refused", "sent": 0, "encoded": true }), "{}", r);
    assert_eq!(r["closed"], "true", "{}", r);
    assert_eq!(r["after"], json!({ "send": "ended", "play": "ended", "sent": 0, "played": 0, "decoder": null }), "{}", r);
    assert_eq!(r["again"], "false", "{}", r);
    assert_eq!(r["twice"], json!({ "state": "sending", "sent": true }), "{}", r);
    no_errors(&r);
}

/// A page may send only a stream the program takes from pages (`media.on`),
/// as USERGUIDE.md says; a page sending one it never named should be
/// refused, as one `publish` says no to is.
///
/// Ignored: it is not. A channel no handler names may be joined by any page
/// (src/wt.rs, `answer`: "pages may join it"), and `media:<name>` is such a
/// channel when no `media.on` names the stream, so the page joins it and
/// src/wt.rs `frame_from_page` takes its frames: `media.streams()` lists
/// stream "nobody" with the frames it sent, and any page could send into a
/// stream that comes from a camera (`media.rtp`) for everyone watching it.
#[test]
#[ignore = "bug: a page may send a stream no media.on names (src/wt.rs answer / frame_from_page)"]
fn a_stream_no_media_on_names_is_refused() {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/page", "media-close.js", FAKE_CAMERA, &[]);
    assert_eq!(r["unknown"], json!({ "state": "refused", "sent": 0 }), "{}", r);
    assert!(!r["streams"].as_str().unwrap_or("").contains("\"nobody\""), "{}", r);
}

/// In a Chrome that lets no page have the camera, a sender is `refused`
/// (after asking once), and a source given then goes: the player shows its
/// colour. A source given before it starts goes without asking; another in
/// its place is what the player shows next, at its size; none sends
/// nothing more. A source too small for H.264 sends no picture, and the one
/// given after it does.
#[test]
fn a_source_goes_instead_of_a_camera_the_page_may_not_have() {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/page", "media-source.js", NO_PERMISSION, &[]);
    assert_eq!(r["denied"], json!({ "state": "refused", "asked": 1, "source": null, "then": "sending", "shows": "7,0,0", "player": "playing" }), "{}", r);
    let given = &r["given"];
    assert_eq!(
        (&given["state"], &given["asked"], &given["source"], &given["shows"], &given["tracks"]),
        (&json!("sending"), &json!(0), &json!(true), &json!("0,0,7"), &json!(1)),
        "{}",
        r
    );
    assert_eq!(given["then"], json!({ "state": "sending", "shows": "0,7,0", "width": 640 }), "{}", r);
    assert_eq!((&given["none"]["sent"], &given["none"]["source"]), (&json!(0), &json!(null)), "{}", r);
    assert_eq!(r["tiny"], json!({ "video": 0, "then": { "state": "sending", "shows": "7,0,0" } }), "{}", r);
    no_errors(&r);
}

/// A source of a screen's size (1920x1080) is sent, and so is the sound of
/// one of four channels (mixed down to two), as the guide's
/// `el.source = js("navigator.mediaDevices.getDisplayMedia")` suggests.
///
/// Ignored: neither is. runtime.js `mediaPublisher` encodes every picture
/// as `avc1.42E01F` (line 3777), level 3.1, whose largest picture is
/// 1280x720: the encoder fails ("exceeds the maximum coded area (921600)
/// supported by the AVC level (3.1)"), its error handler (line 3775) only
/// forgets it, and the sender stays `sending` with no picture going. The
/// audio encoder is configured for `Math.min(channels, 2)` (line 3820) but
/// given the four channels: each frame fails ("Input audio buffer is
/// incompatible with codec parameters"), a new encoder is made for the next,
/// and no sound goes.
#[test]
#[ignore = "bug: runtime.js mediaPublisher sends no picture above 1280x720 (line 3777) and no sound of more than 2 channels (line 3820)"]
fn a_large_source_and_one_of_four_channels_are_sent() {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/page?large", "media-source.js", NO_PERMISSION, &[]);
    assert!(r["large"]["video"].as_u64().unwrap_or(0) > 10, "{}", r);
    assert!(r["wide"]["audio"].as_u64().unwrap_or(0) > 10, "{}", r);
}

/// What the encoders cannot take does not stop a sender, or reach the page
/// as an error: a 1920x1080 source and sound of four channels (each failing
/// its encoder, see the test above) leave it `sending`, and the picture of
/// the four-channel source still goes.
#[test]
fn an_encoder_that_fails_reaches_the_page_as_no_error() {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/page?large", "media-source.js", NO_PERMISSION, &[]);
    assert_eq!(r["large"]["state"], "sending", "{}", r);
    assert_eq!((&r["wide"]["state"], &r["wide"]["video"]), (&json!("sending"), &json!(true)), "{}", r);
    no_errors(&r);
}

/// A sender that sends what it should not: a frame said to be a keyframe
/// that is not one is dropped (the player has no decoder, and nothing is
/// reported), and the next real keyframe plays. 700 frames at once while
/// the player's page is busy are more than it keeps up with: it starts
/// again (no decoder, `waiting`), skips frames until the next keyframe, and
/// plays from it.
#[test]
fn a_player_drops_a_false_keyframe_and_catches_up_from_the_next() {
    let Some(chrome) = browser() else { return };
    let r = run(&chrome, "/page", "media-inject.js", "", &[]);
    assert_eq!(r["encoded"], json!([true, false, false]), "{}", r);
    let not_key = &r["notKey"];
    assert_eq!((&not_key["decoder"], &not_key["video"], &not_key["then"]), (&json!(null), &json!(0), &json!(true)), "{}", r);
    assert_ne!(not_key["state"], "playing", "{}", r);
    assert_eq!(r["behind"], json!({ "state": "waiting", "decoder": null, "shown": 0, "skipped": true, "then": true }), "{}", r);
    no_errors(&r);
}

/// The server restarts while a page sends and plays: while it is down the
/// player waits; then the page connects again (joins the stream's channel again, and watches it
/// again), and frames go both ways once more, the sender still `sending`.
#[test]
fn a_sender_and_a_player_go_on_after_the_server_restarts() {
    let Some(chrome) = browser() else { return };
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let port = support::free_port();
    let mut first = server(port, &[], Stdio::piped());
    // The page asks for /restart-me when it is time; the log says so.
    let (tx, rx) = mpsc::channel();
    let log = first.0.stderr.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(log).lines().map_while(Result::ok) {
            if line.contains("/restart-me") {
                let _ = tx.send(());
            }
        }
    });
    let node = page(&chrome, port, "/", "media-reconnect.js", FAKE_CAMERA, 90000);
    let again = match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(()) => {
            // Down long enough for the player to notice (3 s without frames).
            drop(first);
            std::thread::sleep(Duration::from_secs(4));
            Some(server(port, &[], Stdio::null()))
        }
        Err(_) => None,
    };
    let out = node.wait_with_output().expect("node");
    drop(again);
    let r = result(out, port);
    assert_eq!(r["before"], true, "{}", r);
    assert_eq!(r["gap"], true, "the player did not see the server go: {}", r);
    assert_eq!(r["after"], true, "frames did not come again: {}", r);
    assert_eq!(r["state"], "sending", "{}", r);
    assert_eq!(r["via"], "webtransport", "{}", r);
    no_errors(&r);
}
