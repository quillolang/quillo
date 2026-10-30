//! `image.convert`'s options and `image.info`'s arguments: what each takes, and
//! what it says of the rest.

use quillo::Interp;
use std::path::{Path, PathBuf};

fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-image-options-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    image::RgbImage::from_fn(40, 30, |x, y| image::Rgb([x as u8, y as u8, 128])).save(d.join("a.png")).unwrap();
    d
}

fn run(dir: &Path, src: &str) -> String {
    let mut it = Interp::new(dir);
    it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

#[track_caller]
fn refused(dir: &Path, src: &str, wanted: &str) {
    let mut it = Interp::new(dir);
    match it.load_source(src, "t.qwe") {
        Ok(_) => panic!("expected an error\n{}", src),
        Err(e) => assert!(e.to_string().contains(wanted), "{}\n  wanted: {}\n  got: {}", src, wanted, e),
    }
}

#[test]
fn a_gif_is_read_and_comes_out_as_something_else() {
    let d = folder("gif");
    image::RgbaImage::from_pixel(12, 8, image::Rgba([10, 20, 30, 255])).save(d.join("a.gif")).unwrap();
    assert_eq!(run(&d, r#"image.info("a.gif")"#), r#"{ width: 12, height: 8, format: "gif", alpha: true }"#);
    assert_eq!(run(&d, r#"image.convert("a.gif", "b.png", { format: "png" })"#), "{ width: 12, height: 8 }");
    assert_eq!(run(&d, r#"image.info("b.png")"#), r#"{ width: 12, height: 8, format: "png", alpha: true }"#);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn other_image_kinds_are_refused() {
    let d = folder("bmp");
    std::fs::write(d.join("a.bmp"), b"BM\x46\0\0\0\0\0\0\0\x36\0\0\0").unwrap();
    assert_eq!(run(&d, r#"image.info("a.bmp")"#), r#"err("image: not a JPEG, PNG, WebP or GIF image")"#);
    assert_eq!(run(&d, r#"image.convert("a.bmp", "b.png")"#), r#"err("image: not a JPEG, PNG, WebP or GIF image")"#);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn paths_are_strs_inside_the_folder() {
    let d = folder("paths");
    refused(&d, "image.info(5)", "`image.info` expects a path, got int");
    refused(&d, "image.info()", "`image.info` expects a path, got nothing");
    refused(&d, "image.convert(\"a.png\")", "`image.convert` expects a path, got nothing");
    refused(&d, "image.convert(\"a.png\", \"../b.png\")", "`image.convert`: \"../b.png\" is not a path inside the program's folder");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_file_too_wide_to_read_says_so() {
    let d = folder("wide");
    // A GIF header: its reader knows the size before it looks at limits.
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&40_000u16.to_le_bytes());
    gif.extend_from_slice(&10u16.to_le_bytes());
    gif.extend_from_slice(&[0, 0, 0]);
    gif.extend_from_slice(&[0x2C, 0, 0, 0, 0]);
    gif.extend_from_slice(&40_000u16.to_le_bytes());
    gif.extend_from_slice(&10u16.to_le_bytes());
    gif.extend_from_slice(&[0, 2, 0, 0, 0x3B]);
    std::fs::write(d.join("wide.gif"), gif).unwrap();
    let out = run(&d, r#"image.info("wide.gif")"#);
    assert!(out.starts_with("err(\"image: "), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn options_may_be_left_out() {
    let d = folder("options");
    assert_eq!(run(&d, r#"image.convert("a.png", "plain.jpg")"#), "{ width: 40, height: 30 }");
    assert_eq!(run(&d, r#"image.info("plain.jpg")"#), r#"{ width: 40, height: 30, format: "jpeg", alpha: false }"#);
    assert_eq!(run(&d, r#"image.convert("a.png", "nil.jpg", null)"#), "{ width: 40, height: 30 }");
    assert_eq!(run(&d, r#"image.convert("a.png", "nil.png", { crop: null, format: null, format: "png" })"#), "{ width: 40, height: 30 }");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn whole_floats_count_as_pixels() {
    let d = folder("floats");
    assert_eq!(run(&d, r#"image.convert("a.png", "c.jpg", { crop: { x: 2.0, y: 1.4, w: 20.0, h: 10.0 }, width: 10.0 })"#), "{ width: 10, height: 5 }");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_wrong_option_is_a_mistake_in_the_program() {
    let d = folder("wrong");
    let cases = [
        ("5", "`image.convert` expects options as a map, got int"),
        ("{ crop: 5 }", "`image.convert`: crop is a map, got int"),
        ("{ crop: { x: 0, y: 0, w: 0, h: 5 } }", "`image.convert`: crop is { x, y, w, h }, whole pixels, w and h above 0"),
        ("{ crop: { x: -1, y: 0, w: 5, h: 5 } }", "crop is { x, y, w, h }"),
        ("{ crop: { x: 0, y: 0, w: 5 } }", "crop is { x, y, w, h }"),
        ("{ width: 0 }", "`image.convert`: width 0 is not from 1 to 12000"),
        ("{ width: 12001 }", "`image.convert`: width 12001 is not from 1 to 12000"),
        ("{ format: \"tiff\" }", "`image.convert`: format is jpeg, png or webp, got \"tiff\""),
        ("{ format: 5 }", "`image.convert`: format is a str, got int"),
    ];
    for (opts, wanted) in cases {
        refused(&d, &format!("image.convert(\"a.png\", \"x.jpg\", {})", opts), wanted);
    }
    assert!(!d.join("x.jpg").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn quality_is_kept_between_one_and_a_hundred() {
    let d = folder("quality");
    assert_eq!(run(&d, r#"image.convert("a.png", "low.jpg", { quality: -5 })"#), "{ width: 40, height: 30 }");
    assert_eq!(run(&d, r#"image.convert("a.png", "high.jpg", { quality: 500 })"#), "{ width: 40, height: 30 }");
    let (low, high) = (std::fs::metadata(d.join("low.jpg")).unwrap().len(), std::fs::metadata(d.join("high.jpg")).unwrap().len());
    assert!(low < high, "{} {}", low, high);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn nothing_is_left_behind_when_the_file_cannot_be_written() {
    let d = folder("nowhere");
    std::fs::write(d.join("taken"), "a file").unwrap();
    let out = run(&d, r#"image.convert("a.png", "taken/b.png", { format: "png" })"#);
    assert!(out.starts_with("err(\"image: "), "{}", out);
    assert_eq!(std::fs::read_to_string(d.join("taken")).unwrap(), "a file");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_shrunk_image_is_never_less_than_a_pixel_high() {
    let d = folder("thin");
    image::RgbImage::new(400, 2).save(d.join("thin.png")).unwrap();
    assert_eq!(run(&d, r#"image.convert("thin.png", "t.png", { width: 4, format: "png" })"#), "{ width: 4, height: 1 }");
    let _ = std::fs::remove_dir_all(&d);
}
