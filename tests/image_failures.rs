//! `image.info` and `image.convert` when the files don't cooperate: a path
//! with nothing there, a folder for a file, a folder it can't write in, a
//! place to write that stops taking bytes. What the program gets is an error
//! value saying why, and no half-made file is left.

mod support;

use quillo::Interp;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn folder(name: &str) -> PathBuf {
    let d = support::temp_dir(&format!("image-failures-{}", name));
    image::RgbImage::from_fn(8, 8, |x, y| image::Rgb([x as u8, y as u8, 128])).save(d.join("small.png")).unwrap();
    d
}

/// Far larger when encoded than a pipe holds, in every format: noise.
fn noisy(d: &Path) {
    let mut r = fastrand::Rng::with_seed(7);
    image::RgbImage::from_fn(500, 500, |_, _| image::Rgb([r.u8(..), r.u8(..), r.u8(..)])).save(d.join("noise.png")).unwrap();
}

fn run(dir: &Path, src: &str) -> String {
    let mut it = Interp::new(dir);
    it.load_source(&format!("let out = {}\n", src), "t.qwe").unwrap_or_else(|e| panic!("{}", e));
    quillo::value::lookup(&it.globals, "out").unwrap().repr()
}

fn says(got: String, what: &str) {
    assert!(got.starts_with("err(\"image: ") && got.contains(what), "wanted {:?} in: {}", what, got);
}

#[test]
fn a_file_that_is_not_there_says_so() {
    let d = folder("missing");
    says(run(&d, r#"image.info("gone.png")"#), "No such file or directory");
    says(run(&d, r#"image.convert("gone.png", "out.png")"#), "No such file or directory");
    assert!(!d.join("out.png").exists());
}

#[test]
fn a_file_it_may_not_read_says_so() {
    let d = folder("shut");
    std::fs::set_permissions(d.join("small.png"), std::fs::Permissions::from_mode(0o000)).unwrap();
    says(run(&d, r#"image.info("small.png")"#), "Permission denied");
    std::fs::set_permissions(d.join("small.png"), std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn a_folder_is_not_an_image() {
    let d = folder("a-folder");
    std::fs::create_dir(d.join("uploads")).unwrap();
    says(run(&d, r#"image.info("uploads")"#), "Is a directory");
    says(run(&d, r#"image.convert("uploads", "out.png")"#), "Is a directory");
}

#[test]
fn a_folder_it_may_not_write_in_says_so() {
    let d = folder("read-only");
    std::fs::create_dir(d.join("cache")).unwrap();
    std::fs::set_permissions(d.join("cache"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let got = run(&d, r#"image.convert("small.png", "cache/out.png")"#);
    std::fs::set_permissions(d.join("cache"), std::fs::Permissions::from_mode(0o755)).unwrap();
    says(got, "Permission denied");
    assert_eq!(std::fs::read_dir(d.join("cache")).unwrap().count(), 0);
}

#[test]
fn a_file_where_the_folder_should_be_says_so() {
    let d = folder("a-file");
    std::fs::write(d.join("cache"), "a file").unwrap();
    says(run(&d, r#"image.convert("small.png", "cache/out.png")"#), "exists");
}

#[test]
fn a_finished_image_that_can_not_take_the_place_of_a_folder_says_so() {
    let d = folder("over-a-folder");
    std::fs::create_dir_all(d.join("out.png/inside")).unwrap();
    says(run(&d, r#"image.convert("small.png", "out.png")"#), "Is a directory");
    assert!(d.join("out.png/inside").is_dir(), "the folder is left as it was");
}

#[test]
#[ignore = "finding: image.convert leaves its temporary file behind when the rename fails"]
fn a_finished_image_that_has_nowhere_to_go_leaves_nothing_behind() {
    let d = folder("tmp-left");
    std::fs::create_dir_all(d.join("out.png/inside")).unwrap();
    says(run(&d, r#"image.convert("small.png", "out.png")"#), "Is a directory");
    let left: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains("tmp")).collect();
    assert!(left.is_empty(), "{:?}", left);
}

/// The place `image.convert` writes to before the rename, as a pipe whose
/// reader goes away at once: each write past what a pipe holds then fails.
fn stops_taking_bytes(dest: &Path) -> std::thread::JoinHandle<()> {
    let tmp = dest.with_extension(format!("tmp{}", std::process::id()));
    let c = CString::new(tmp.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o644) }, 0);
    std::thread::spawn(move || drop(std::fs::File::open(tmp).unwrap()))
}

fn a_write_that_fails_ends_the_conversion(format: &str) {
    let d = folder(&format!("broken-pipe-{}", format));
    noisy(&d);
    let dest = d.join(format!("out.{}", format));
    let reader = stops_taking_bytes(&dest);
    let got = run(&d, &format!(r#"image.convert("noise.png", "out.{}", {{ format: "{}" }})"#, format, format));
    reader.join().unwrap();
    says(got, "Broken pipe");
    assert!(!dest.exists());
    let left: Vec<_> = std::fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains("tmp")).collect();
    assert!(left.is_empty(), "{:?}", left);
}

#[test]
fn a_jpeg_that_can_not_be_written_says_why() {
    a_write_that_fails_ends_the_conversion("jpeg");
}

#[test]
fn a_webp_that_can_not_be_written_says_why() {
    a_write_that_fails_ends_the_conversion("webp");
}

#[test]
fn a_png_that_can_not_be_written_says_why() {
    a_write_that_fails_ends_the_conversion("png");
}

#[test]
#[ignore = "finding: image.convert never checks its last flush: on a full disk a small image gives a success and a cut-off file"]
fn an_image_cut_off_by_a_full_disk_is_not_a_success() {
    let d = folder("file-limit");
    std::fs::write(d.join("app.qwe"), "print(image.convert(\"small.png\", \"out.jpeg\", { format: \"jpeg\" }))\n").unwrap();
    // Files past 512 bytes refused, as on a full disk (a coverage file wouldn't fit either).
    let script = format!("trap '' XFSZ; ulimit -f 1; exec '{}' run app.qwe", env!("CARGO_BIN_EXE_quillo"));
    let out = Command::new("sh").args(["-c", &script]).current_dir(&d).env_remove("LLVM_PROFILE_FILE").stdin(Stdio::null()).output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains("image: "), "{}\n{}", said, String::from_utf8_lossy(&out.stderr));
    assert!(!d.join("out.jpeg").exists(), "{} bytes", std::fs::metadata(d.join("out.jpeg")).unwrap().len());
}
