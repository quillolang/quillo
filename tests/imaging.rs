//! `image.info` and `image.convert`: sizes, crops, shrinking, formats, and
//! files that are not images or claim to be huge.

use quillo::Interp;
use std::path::Path;

fn run(dir: &Path, src: &str) -> String {
    let mut it = Interp::new(dir);
    it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

/// A PNG whose header claims a size, with no pixels (what a "bomb" sends).
fn claimed_png(w: u32, h: u32) -> Vec<u8> {
    let mut ihdr = b"IHDR".to_vec();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut crc = flate2::Crc::new();
    crc.update(&ihdr);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend_from_slice(&13u32.to_be_bytes());
    out.extend_from_slice(&ihdr);
    out.extend_from_slice(&crc.sum().to_be_bytes());
    out
}

#[test]
fn images_are_measured_cropped_and_shrunk() {
    let d = std::env::temp_dir().join(format!("quillo-image-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let img = image::RgbImage::from_fn(400, 300, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]));
    img.save(d.join("a.png")).unwrap();
    assert_eq!(run(&d, r#"image.info("a.png")"#), r#"{ width: 400, height: 300, format: "png", alpha: false }"#);
    // A 16:9 crop of the middle, shrunk to 160 wide, as a JPEG.
    assert_eq!(run(&d, r#"image.convert("a.png", "out/c.jpg", { crop: { x: 40, y: 37, w: 320, h: 180 }, width: 160, format: "jpeg" })"#), "{ width: 160, height: 90 }");
    assert_eq!(run(&d, r#"image.info("out/c.jpg")"#), r#"{ width: 160, height: 90, format: "jpeg", alpha: false }"#);
    // Never enlarged; webp and png out.
    assert_eq!(run(&d, r#"image.convert("a.png", "b.webp", { width: 900, format: "webp" })"#), "{ width: 400, height: 300 }");
    assert!(run(&d, r#"image.info("b.webp")"#).contains("\"webp\""));
    // A crop outside the image, a file that is not one, a path outside the folder.
    assert!(run(&d, r#"image.convert("a.png", "x.jpg", { crop: { x: 300, y: 0, w: 200, h: 100 } })"#).contains("outside the 400×300 image"));
    std::fs::write(d.join("not.png"), "hello").unwrap();
    assert!(run(&d, r#"image.info("not.png")"#).starts_with("err("), "{}", run(&d, r#"image.info("not.png")"#));
    let mut it = Interp::new(&d);
    assert!(it.load_source(r#"image.info("../a.png")"#, "t.qwe").is_err());
    // A file claiming to be enormous is refused before anything is allocated.
    std::fs::write(d.join("bomb.png"), claimed_png(20_000, 20_000)).unwrap();
    assert!(run(&d, r#"image.info("bomb.png")"#).starts_with("err("), "{}", run(&d, r#"image.info("bomb.png")"#));
    assert!(run(&d, r#"image.convert("bomb.png", "x.jpg", {})"#).starts_with("err("));
    let _ = std::fs::remove_dir_all(&d);
}
