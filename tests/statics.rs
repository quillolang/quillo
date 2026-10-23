//! Files served with `file()`: validators and 304s, byte ranges, cache
//! lifetimes, and changes on disk noticed.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::PathBuf;

fn site(name: &str) -> (PathBuf, Interp) {
    let d = std::env::temp_dir().join(format!("quillo-statics-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("public")).unwrap();
    let mut it = Interp::new(&d);
    it.load_source(
        r#"
route GET /f/*path = file("public/" + path)
route GET /v/*path = file("public/" + path, { cache: "1y" })
route GET /bad = file("public/a.txt", { cache: "soon" })
"#,
        "app.qwe",
    )
    .unwrap_or_else(|e| panic!("{}", e));
    (d, it)
}

fn get(it: &mut Interp, url: &str, headers: &[(&str, &str)]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "x".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "127.0.0.1".into() })
}

#[test]
fn a_current_copy_gets_a_304() {
    let (d, mut it) = site("304");
    std::fs::write(d.join("public/a.txt"), "hello").unwrap();
    let r = get(&mut it, "/f/a.txt", &[]);
    assert_eq!((r.status, r.text().as_str()), (200, "hello"));
    assert_eq!(r.header("cache-control"), Some("no-cache"));
    assert_eq!(r.header("accept-ranges"), Some("bytes"));
    let etag = r.header("etag").unwrap().to_string();
    let modified = r.header("last-modified").unwrap().to_string();
    assert!(etag.starts_with("W/\"") && modified.ends_with(" GMT"), "{} {}", etag, modified);

    let r = get(&mut it, "/f/a.txt", &[("If-None-Match", &etag)]);
    assert_eq!((r.status, r.body.len()), (304, 0));
    assert_eq!(r.header("etag"), Some(etag.as_str()));
    assert_eq!(get(&mut it, "/f/a.txt", &[("If-None-Match", "\"other\", *")]).status, 304);
    assert_eq!(get(&mut it, "/f/a.txt", &[("If-None-Match", "\"other\"")]).status, 200);
    assert_eq!(get(&mut it, "/f/a.txt", &[("If-Modified-Since", &modified)]).status, 304);
    // An ETag that does not match wins over a date that does.
    assert_eq!(get(&mut it, "/f/a.txt", &[("If-None-Match", "\"other\""), ("If-Modified-Since", &modified)]).status, 200);

    // A changed file is a different version, and is not served from memory.
    std::fs::write(d.join("public/a.txt"), "hello, again").unwrap();
    let r = get(&mut it, "/f/a.txt", &[("If-None-Match", &etag)]);
    assert_eq!((r.status, r.text().as_str()), (200, "hello, again"));
    assert_ne!(r.header("etag"), Some(etag.as_str()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ranges_are_served_on_their_own() {
    let (d, mut it) = site("range");
    std::fs::write(d.join("public/b.txt"), "0123456789").unwrap();

    let r = get(&mut it, "/f/b.txt", &[("Range", "bytes=2-5")]);
    assert_eq!((r.status, r.text().as_str(), r.header("content-range")), (206, "2345", Some("bytes 2-5/10")));
    let r = get(&mut it, "/f/b.txt", &[("Range", "bytes=-3")]);
    assert_eq!((r.status, r.text().as_str()), (206, "789"));
    let r = get(&mut it, "/f/b.txt", &[("Range", "bytes=7-")]);
    assert_eq!((r.status, r.text().as_str()), (206, "789"));
    let r = get(&mut it, "/f/b.txt", &[("Range", "bytes=10-")]);
    assert_eq!((r.status, r.body.len(), r.header("content-range")), (416, 0, Some("bytes */10")));
    // Several ranges, or another unit: the whole file.
    assert_eq!(get(&mut it, "/f/b.txt", &[("Range", "bytes=0-1,4-5")]).status, 200);
    // If-Range names another version: the whole (new) file.
    let r = get(&mut it, "/f/b.txt", &[("Range", "bytes=2-5"), ("If-Range", "W/\"old\"")]);
    assert_eq!((r.status, r.text().as_str()), (200, "0123456789"));
    let etag = r.header("etag").unwrap().to_string();
    assert_eq!(get(&mut it, "/f/b.txt", &[("Range", "bytes=2-5"), ("If-Range", &etag)]).status, 206);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn versioned_files_can_be_kept_for_long() {
    let (d, mut it) = site("cache");
    std::fs::write(d.join("public/a.txt"), "x").unwrap();
    assert_eq!(get(&mut it, "/v/a.txt", &[]).header("cache-control"), Some("public, max-age=31536000, immutable"));
    assert_eq!(get(&mut it, "/bad", &[]).status, 500);
    assert_eq!(get(&mut it, "/f/missing.txt", &[]).status, 404);
    assert_eq!(get(&mut it, "/f/../app.qwe", &[]).status, 404);
    let _ = std::fs::remove_dir_all(&d);
}
