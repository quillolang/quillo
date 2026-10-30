//! What a function says when it is handed something it can't use: `udp.send`'s
//! payloads, `media.rtp`'s tracks, a channel's name, a pattern too big to
//! compile, and a `style_with` that fails on one style.

use quillo::eval_str;

#[track_caller]
fn refused(src: &str, want: &str) {
    match eval_str(src) {
        Ok(v) => panic!("{} gave {}", src, v.repr()),
        Err(e) => assert_eq!(e.msg, want, "{}", src),
    }
}

#[test]
fn udp_has_nothing_to_send_for_null() {
    refused("udp.send(\"127.0.0.1\", 9, null)", "`udp.send` has nothing to send");
}

#[test]
fn udp_will_not_send_more_than_a_packet_holds() {
    refused("udp.send(\"127.0.0.1\", 9, repeat(\"x\", 70000))", "`udp.send`: 70000 bytes is more than a UDP packet holds (65507)");
}

#[test]
fn a_media_track_is_a_port_or_a_port_with_its_codec() {
    refused("media.rtp(\"cam\", { video: \"a\" })", "`media.rtp`: video is a port, or { port, codec: \"h264\" }");
    refused("media.rtp(\"cam\", { video: 0 })", "`media.rtp`: video is a port, or { port, codec: \"h264\" }");
    refused("media.rtp(\"cam\", { video: 70000 })", "`media.rtp`: video is a port, or { port, codec: \"h264\" }");
    refused("media.rtp(\"cam\", { video: { codec: \"h264\" } })", "`media.rtp`: video is a port, or { port, codec: \"h264\" }");
    refused("media.rtp(\"cam\", { audio: { port: \"x\" } })", "`media.rtp`: audio is a port, or { port, codec: \"opus\" }");
}

#[test]
fn a_media_track_takes_only_its_own_codec() {
    refused("media.rtp(\"cam\", { video: { port: 5004, codec: \"vp8\" } })", "`media.rtp`: vp8 in video is not supported (h264 is)");
    refused("media.rtp(\"cam\", { audio: { port: 5006, codec: \"aac\" } })", "`media.rtp`: aac in audio is not supported (opus is)");
    refused("media.rtp(\"cam\", { audio: { port: 5006, codec: 5 } })", "`media.rtp`: 5 in audio is not supported (opus is)");
}

#[test]
fn a_channels_name_is_plain_and_short() {
    let told = |got: &str| format!("`channel.on`: a channel's name is letters, digits, `_`, `-`, `.` and `:`, up to 64, got {}", got);
    refused("channel.on(\"two words\", (m, p) => m)", &told("\"two words\""));
    refused("channel.on(\"\", (m, p) => m)", &told("\"\""));
    refused(&format!("channel.on(\"{}\", (m, p) => m)", "x".repeat(65)), &told(&format!("\"{}\"", "x".repeat(65))));
    // And every function that takes one says its own name.
    refused("channel.send(\"a/b\", \"id\", 1)", "`channel.send`: a channel's name is letters, digits, `_`, `-`, `.` and `:`, up to 64, got \"a/b\"");
}

#[test]
fn a_channels_name_comes_first() {
    refused("channel.on(5, (m, p) => m)", "`channel.on` takes a channel's name first, got int");
    refused("channel.on()", "`channel.on` takes a channel's name first, got nothing");
    refused("channel.broadcast(null, 1)", "`channel.broadcast` takes a channel's name first, got null");
}

#[test]
fn a_pattern_too_big_to_compile_is_refused_when_used() {
    let e = eval_str("re.test(\"a\", \"(a{1000}){1000}\")").err().expect("refused");
    assert!(e.msg.starts_with("`re.test`: the pattern is too big: "), "{}", e.msg);
    assert!(e.msg.contains("size limit"), "{}", e.msg);
    assert!(quillo::re::compile("(a{1000}){1000}", "").unwrap_err().starts_with("the pattern is too big: "));
    // With flags too, and not when it is small.
    assert!(quillo::re::compile("(a{1000}){1000}", "i").is_err());
    assert_eq!(quillo::eval_str("re.test(\"aaa\", \"a{3}\")").unwrap().repr(), "true");
}

#[cfg(unix)]
#[test]
fn style_with_that_fails_on_one_style_names_that_style() {
    use std::os::unix::fs::PermissionsExt;
    let d = std::env::temp_dir().join(format!("quillo-refused-style-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // Takes the styles together, its marks left out, then none alone.
    let tool = d.join("fussy.sh");
    let fussy = "#!/bin/sh\nin=$(cat)\ncase \"$in\" in\n*quillo-style*) printf '%s\\n' \"$in\" | grep -v quillo-style ;;\n*) echo \"will not take this alone\" >&2; exit 3 ;;\nesac\n";
    std::fs::write(&tool, fussy).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let src = "style_with(\"./fussy.sh\")\ncomponent Card() {\n    style css`.t { color: red }`\n    <p.t>card</p>\n}\nroute GET / = <html><body><Card/></body></html>\n";
    let mut it = quillo::Interp::new(&d);
    let e = it.load_source(src, "app.qwe").err().expect("the load fails").to_string();
    assert_eq!(e, "app.qwe:2: style_with `./fussy.sh` failed on `Card`'s style: will not take this alone");
    let _ = std::fs::remove_dir_all(&d);
}
