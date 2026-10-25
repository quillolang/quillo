//! Files sent with forms: `request.files`, `save_upload`, `read_upload`.

use quillo::server::HttpRequest;
use quillo::Interp;

fn multipart(parts: &[(&str, Option<(&str, &str)>, &[u8])]) -> Vec<u8> {
    let mut b = Vec::new();
    for (name, file, data) in parts {
        b.extend_from_slice(b"--XyZ\r\n");
        match file {
            Some((filename, ctype)) => b.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n", name, filename, ctype).as_bytes(),
            ),
            None => b.extend_from_slice(format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes()),
        }
        b.extend_from_slice(data);
        b.extend_from_slice(b"\r\n");
    }
    b.extend_from_slice(b"--XyZ--\r\n");
    b
}

#[test]
fn files_arrive_with_their_form_and_save_byte_for_byte() {
    let d = std::env::temp_dir().join(format!("quillo-uploads-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut it = Interp::new(&d);
    it.load_source(
        r#"
route POST /up {
    let a = request.files.avatar
    {
        title: request.form.title,
        name: a.filename, type: a.type, size: a.size,
        saved: save_upload(a, "uploads/" + a.filename),
        notes: read_upload(request.files.notes),
        photos: request.files.photos | map((p) => p.filename),
    }
}
route POST /escape = save_upload(request.files.avatar, "../outside.bin")
"#,
        "app.qwe",
    )
    .unwrap_or_else(|e| panic!("{}", e));
    let binary: &[u8] = &[0, 1, 2, 0xff, 0xfe, b'\r', b'\n', b'-', b'-', 0x80];
    let body = multipart(&[
        ("title", None, b"Holiday"),
        ("avatar", Some(("C:\\fakepath\\me.png", "image/png")), binary),
        ("notes", Some(("notes.txt", "text/plain")), "line one\nline two".as_bytes()),
        ("photos", Some(("a.jpg", "image/jpeg")), b"A"),
        ("photos", Some(("b.jpg", "image/jpeg")), b"B"),
    ]);
    let send = |it: &mut Interp, url: &str| {
        let headers = vec![
            ("Host".to_string(), "x".to_string()),
            ("Content-Type".to_string(), "multipart/form-data; boundary=XyZ".to_string()),
        ];
        it.handle(&HttpRequest { method: "POST".into(), url: url.into(), headers, body: body.clone(), remote: "127.0.0.1".into() })
    };
    let r = send(&mut it, "/up");
    let v: serde_json::Value = serde_json::from_str(&r.text()).unwrap_or_else(|e| panic!("{}: {}", e, r.text()));
    assert_eq!(
        v,
        serde_json::json!({
            "title": "Holiday", "name": "me.png", "type": "image/png", "size": binary.len(),
            "saved": "uploads/me.png", "notes": "line one\nline two", "photos": ["a.jpg", "b.jpg"]
        })
    );
    assert_eq!(std::fs::read(d.join("uploads/me.png")).unwrap(), binary);
    // Only into the program's folder.
    assert_eq!(send(&mut it, "/escape").status, 500);
    assert!(!d.parent().unwrap().join("outside.bin").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn boundaries_are_read_from_the_content_type() {
    assert_eq!(quillo::upload::boundary("multipart/form-data; boundary=\"a b\""), Some("a b".into()));
    assert_eq!(quillo::upload::boundary("Multipart/Form-Data;boundary=xyz"), Some("xyz".into()));
    assert_eq!(quillo::upload::boundary("application/json"), None);
    assert_eq!(quillo::upload::boundary("multipart/form-data"), None);
}

#[test]
fn server_fns_take_files_from_the_programs_own_pages() {
    let d = std::env::temp_dir().join(format!("quillo-rpc-files-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let mut it = Interp::new(&d);
    it.load_source("server fn keep(file: map, name: str) = { saved: save_upload(file, `in/${name}`), size: file.size, type: file.type }", "app.qwe")
        .unwrap_or_else(|e| panic!("{}", e));
    let body = multipart(&[("args", None, br#"[{"$quillo_file": 0}, "pic.bin"]"#), ("file0", Some(("photo.jpg", "image/jpeg")), &[1, 2, 3, 255])]);
    let send = |it: &mut Interp, extra: &[(&str, &str)]| {
        let mut headers = vec![("Host".to_string(), "x".to_string()), ("Content-Type".to_string(), "multipart/form-data; boundary=XyZ".to_string())];
        headers.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        it.handle(&HttpRequest { method: "POST".into(), url: "/_quillo/rpc/keep".into(), headers, body: body.clone(), remote: "127.0.0.1".into() })
    };
    let r = send(&mut it, &[("X-Quillo-Rpc", "1")]);
    assert_eq!(r.text(), r#"{"ok":{"saved":"in/pic.bin","size":4,"type":"image/jpeg"}}"#);
    assert_eq!(std::fs::read(d.join("in/pic.bin")).unwrap(), [1, 2, 3, 255]);
    // A form another site posts has no such header, and is refused.
    assert_eq!(send(&mut it, &[]).status, 415);
    assert_eq!(send(&mut it, &[("X-Quillo-Rpc", "1"), ("Sec-Fetch-Site", "cross-site")]).status, 403);
    let _ = std::fs::remove_dir_all(&d);
}
