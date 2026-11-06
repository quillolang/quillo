//! `image`: what an image file is, and new files made from it.
//!
//! ```text
//! image.info("uploads/a.jpg")            // { width, height, format, alpha }, or an error
//! image.convert("uploads/a.jpg", "cache/a-800.jpg", {
//!     crop: { x: 100, y: 0, w: 1200, h: 675 },   // in the image's pixels
//!     width: 800,                                 // shrunk to it (never enlarged)
//!     format: "jpeg", quality: 82,                // jpeg, png or webp (lossless)
//! })                                              // { width, height }, or an error
//! ```
//!
//! Paths are inside the program's folder. Decoding is limited (12,000
//! pixels a side, 512 MiB) so that a small file claiming to be a huge image
//! cannot take the server's memory, and it runs off the event loop, a few
//! conversions at a time (see `in_turn`).

use crate::interp::{fail, Ctrl, Interp};
use crate::value::*;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Write;
use std::path::{Path, PathBuf};

type R = Result<Value, Ctrl>;

pub fn install(m: &mut Map) {
    let fns: &[(&'static str, BuiltinFn)] = &[("image.info", info), ("image.convert", convert)];
    for (name, f) in fns {
        m.insert(name.trim_start_matches("image.").into(), Value::Builtin(name, *f));
    }
}

const MOST_SIDE: u32 = 12_000;
const MOST_BYTES: u64 = 512 << 20;

fn limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(MOST_SIDE);
    l.max_image_height = Some(MOST_SIDE);
    l.max_alloc = Some(MOST_BYTES);
    l
}

fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Png => "png",
        ImageFormat::WebP => "webp",
        ImageFormat::Gif => "gif",
        _ => "other",
    }
}

fn reader(path: &Path) -> Result<ImageReader<std::io::BufReader<std::fs::File>>, String> {
    let mut r = ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    match r.format() {
        Some(ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif) => {}
        _ => return Err("not a JPEG, PNG, WebP or GIF image".into()),
    }
    r.limits(limits());
    Ok(r)
}

/// Whether an orientation turns the image a quarter, so that its width is its height.
fn turned(o: Orientation) -> bool {
    matches!(o, Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH)
}

/// The image decoded and turned as its EXIF orientation says (FEEDBACK55.md, Q-825; branch
/// feedback55): `decode` alone left a phone's photo on its side, with no EXIF in what was
/// written for a browser to turn it by.
fn decode_upright(path: &Path) -> Result<DynamicImage, String> {
    let mut d = reader(path)?.into_decoder().map_err(|e| e.to_string())?;
    let o = d.orientation().unwrap_or(Orientation::NoTransforms);
    // As `ImageReader::decode` does: the pixels taken from the budget, and the decoder's own
    // buffers held to what is left, so both stay within `MOST_BYTES` (found in review).
    let mut l = limits();
    l.reserve(d.total_bytes()).map_err(|e| e.to_string())?;
    d.set_limits(l).map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(d).map_err(|e| e.to_string())?;
    img.apply_orientation(o);
    Ok(img)
}

fn path_arg(it: &Interp, args: &[Value], i: usize, f: &str, line: u32) -> Result<PathBuf, Ctrl> {
    match args.get(i) {
        Some(Value::Str(p)) => match crate::builtins::local_path(it, p) {
            Some(full) => Ok(full),
            None => fail(line, format!("`{}`: {:?} is not a path inside the program's folder", f, &**p)),
        },
        other => fail(line, format!("`{}` expects a path, got {}", f, other.map_or("nothing", |v| v.type_name()))),
    }
}

fn err_value(e: String) -> Value {
    Value::err(Value::str(format!("image: {}", e)))
}

/// Size and kind, from the file's header (the pixels are not decoded).
fn info(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let path = path_arg(it, &args, 0, "image.info", line)?;
    let found = it.unlocked(move || -> Result<(u32, u32, &'static str, bool), String> {
        let r = reader(&path)?;
        let format = format_name(r.format().unwrap_or(ImageFormat::Png));
        let mut d = r.into_decoder().map_err(|e| e.to_string())?;
        let (w, h) = d.dimensions();
        if w > MOST_SIDE || h > MOST_SIDE {
            return Err(format!("{}×{} pixels is larger than {} a side", w, h, MOST_SIDE));
        }
        // The size it is shown at: a phone's photo taken upright is stored on its side, with an
        // EXIF orientation that says to turn it (FEEDBACK55.md, Q-825; branch feedback55).
        let (w, h) = if turned(d.orientation().unwrap_or(Orientation::NoTransforms)) { (h, w) } else { (w, h) };
        Ok((w, h, format, d.color_type().has_alpha()))
    });
    Ok(match found {
        Ok((w, h, format, alpha)) => {
            let mut m = Map::new();
            m.insert("width".into(), Value::Int(w as i64));
            m.insert("height".into(), Value::Int(h as i64));
            m.insert("format".into(), Value::str(format));
            m.insert("alpha".into(), Value::Bool(alpha));
            Value::map(m)
        }
        Err(e) => err_value(e),
    })
}

struct Job {
    crop: Option<(u32, u32, u32, u32)>,
    width: Option<u32>,
    format: ImageFormat,
    quality: u8,
}

fn int_of(m: &Map, k: &str) -> Option<i64> {
    match m.get(k) {
        Some(Value::Int(n)) => Some(*n),
        Some(Value::Float(f)) => Some(f.round() as i64),
        _ => None,
    }
}

/// The format a target's name says: `.jpg`/`.jpeg`, `.png`, `.webp`, any case.
fn format_of_name(dest: &std::path::Path) -> Option<ImageFormat> {
    match dest.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some(ImageFormat::Jpeg),
        "png" => Some(ImageFormat::Png),
        "webp" => Some(ImageFormat::WebP),
        _ => None,
    }
}

fn job_of(opts: Option<&Value>, dest: &std::path::Path, line: u32) -> Result<Job, Ctrl> {
    let empty = Map::new();
    let o = match opts {
        None | Some(Value::Nil) => &empty,
        Some(Value::Map(m)) => &**m,
        Some(v) => return fail(line, format!("`image.convert` expects options as a map, got {}", v.type_name())),
    };
    let crop = match o.get("crop") {
        None | Some(Value::Nil) => None,
        Some(Value::Map(c)) => match (int_of(c, "x"), int_of(c, "y"), int_of(c, "w"), int_of(c, "h")) {
            (Some(x), Some(y), Some(w), Some(h)) if x >= 0 && y >= 0 && w > 0 && h > 0 => Some((x as u32, y as u32, w as u32, h as u32)),
            _ => return fail(line, "`image.convert`: crop is { x, y, w, h }, whole pixels, w and h above 0"),
        },
        Some(v) => return fail(line, format!("`image.convert`: crop is a map, got {}", v.type_name())),
    };
    let width = match int_of(o, "width") {
        None => None,
        Some(w) if (1..=MOST_SIDE as i64).contains(&w) => Some(w as u32),
        Some(w) => return fail(line, format!("`image.convert`: width {} is not from 1 to {}", w, MOST_SIDE)),
    };
    // Unless said, the target's name says it, where it was JPEG whatever the name, and
    // `out.png` held a JPEG without its transparency (FEEDBACK53.md, Q-684; branch main).
    let format = match o.get("format") {
        // `convert` gave an error value for a name that says none.
        None | Some(Value::Nil) => format_of_name(dest).unwrap_or(ImageFormat::Jpeg),
        Some(Value::Str(f)) => match &**f {
            "jpeg" | "jpg" => ImageFormat::Jpeg,
            "png" => ImageFormat::Png,
            "webp" => ImageFormat::WebP,
            other => return fail(line, format!("`image.convert`: format is jpeg, png or webp, got {:?}", other)),
        },
        Some(v) => return fail(line, format!("`image.convert`: format is a str, got {}", v.type_name())),
    };
    let quality = int_of(o, "quality").unwrap_or(82).clamp(1, 100) as u8;
    Ok(Job { crop, width, format, quality })
}

fn make(src: &Path, dest: &Path, job: Job) -> Result<(u32, u32), String> {
    let mut img = decode_upright(src)?;
    if let Some((x, y, w, h)) = job.crop {
        if x.saturating_add(w) > img.width() || y.saturating_add(h) > img.height() {
            return Err(format!("the crop {}×{} at {},{} is outside the {}×{} image", w, h, x, y, img.width(), img.height()));
        }
        img = img.crop_imm(x, y, w, h);
    }
    if let Some(w) = job.width {
        if w < img.width() {
            let h = ((img.height() as u64 * w as u64) / img.width() as u64).max(1) as u32;
            img = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
        }
    }
    // A folder is no file to write, said before anything is written beside it.
    if dest.is_dir() {
        return Err(format!("{}: Is a directory", dest.display()));
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Written beside, then renamed: a request never sees half a file.
    // The whole name in it: `a.png` and `a.webp` made at once shared `a.tmp<pid>` (found in
    // review of FEEDBACK53.md, Q-684). And a number of each call's own: two made to one name at
    // once wrote one temp file, and the second's rename found none (FEEDBACK55.md, Q-824; branch
    // feedback55). The name cut to 128 bytes, so that what is added fits in a name.
    // Random, and made only where nothing is: two containers sharing a folder are each PID 1, with
    // counters of their own (found in review).
    static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = dest.file_name().map_or(std::borrow::Cow::Borrowed("image"), |n| n.to_string_lossy());
    let cut = (0..=name.len().min(128)).rev().find(|i| name.is_char_boundary(*i)).unwrap_or(0);
    let (tmp, out) = loop {
        let n = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dest.with_file_name(format!("{}.tmp{}-{}-{:08x}", &name[..cut], std::process::id(), n, fastrand::u32(..)));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(out) => break (tmp, out),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };
    let mut w = std::io::BufWriter::new(out);
    let encoded = match job.format {
        ImageFormat::Jpeg => {
            let rgb = on_white(&img);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, job.quality).encode_image(&rgb).map_err(|e| e.to_string())
        }
        ImageFormat::WebP => img.to_rgba8().write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(&mut w)).map_err(|e| e.to_string()),
        _ => img.write_with_encoder(image::codecs::png::PngEncoder::new(&mut w)).map_err(|e| e.to_string()),
    };
    // The last bytes wait in `w`: on a full disk only this flush says so, and a drop would swallow
    // it (FEEDBACK16.md, Q-144; branch feedback20-fixes).
    let written = encoded.and_then(|()| w.flush().map_err(|e| e.to_string()));
    drop(w);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, dest) {
        // (FEEDBACK16.md, Q-145; branch feedback20-fixes)
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok((img.width(), img.height()))
}

/// The pixels for a format with no transparency: what is see-through laid on white, as a browser
/// shows it on a page; `to_rgb8` dropped the alpha, so a transparent corner came out black
/// (FEEDBACK55.md, Q-826; branch feedback55).
fn on_white(img: &DynamicImage) -> image::RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let mix = |c: u8, a: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32) + 127) / 255) as u8;
    let lay = |rgba: &image::RgbaImage| image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let image::Rgba([r, g, b, a]) = *rgba.get_pixel(x, y);
        image::Rgb([mix(r, a), mix(g, a), mix(b, a)])
    });
    match img {
        DynamicImage::ImageRgba8(rgba) => lay(rgba),
        other => lay(&other.to_rgba8()),
    }
}

/// Most images converted at once: each may take up to `MOST_BYTES` to
/// decode, so many uploads at once must not take that many times over.
fn most_converting() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 4)
}

/// Most conversions waiting for their turn; past that, an error at once.
const MOST_WAITING: usize = 64;

/// Most pixels an image converted may have. The decode's own cap leaves its copies out
/// (`to_rgb8`, the resize): a 12,000 × 12,000 PNG of 420 KB took about 0.8 GB (FEEDBACK51.md,
/// Q-674; branch feedback51-fixes).
const MOST_PIXELS: u64 = 40_000_000;

/// Most memory one conversion is thought to take (see `cost_of`), and all of those running at
/// once; one alone may take up to `MOST_BYTES_ONE`. Pixels alone did not bound it: a 40
/// megapixel PNG resized to 6,399 wide took 975 MB, as the resize works in 16 bytes a pixel
/// (found in review of FEEDBACK51.md, Q-674).
const MOST_BYTES_ONE: u64 = 640 << 20;
const BYTES_AT_ONCE: u64 = 1 << 30;

/// `f` once fewer than `most_converting()` conversions run, with `bytes` more fitting in
/// `BYTES_AT_ONCE`, or an error when too many already wait.
fn in_turn<T>(bytes: u64, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use std::sync::{Condvar, Mutex};
    // (running, waiting, their pixels)
    static AT_ONCE: Mutex<(usize, usize, u64)> = Mutex::new((0, 0, 0));
    static FREED: Condvar = Condvar::new();
    let mut n = AT_ONCE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let full = |n: &(usize, usize, u64)| n.0 >= most_converting() || n.0 > 0 && n.2 + bytes > BYTES_AT_ONCE;
    if full(&n) && n.1 >= MOST_WAITING {
        return Err("too many images are being converted at once; try again shortly".into());
    }
    n.1 += 1;
    while full(&n) {
        n = FREED.wait(n).unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    n.1 -= 1;
    n.0 += 1;
    n.2 += bytes;
    drop(n);
    struct Done(u64);
    impl Drop for Done {
        fn drop(&mut self) {
            let mut n = AT_ONCE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            n.0 -= 1;
            n.2 -= self.0;
            // Each waiting one: a large one gone may leave room for several.
            FREED.notify_all();
        }
    }
    let _done = Done(bytes);
    f()
}

/// The memory converting the image at `src` as `job` says is thought to take, from its header:
/// the image decoded, the resize's own (16 bytes a pixel, the width it has by the height it
/// will have) and what it gives, and the copy encoded. An error past `MOST_PIXELS` or
/// `MOST_BYTES_ONE`.
fn cost_of(src: &Path, job: &Job) -> Result<u64, String> {
    let mut d = reader(src)?.into_decoder().map_err(|e| e.to_string())?;
    let (w, h) = d.dimensions();
    let bpp = d.color_type().bytes_per_pixel() as u64;
    let alpha = d.color_type().has_alpha();
    if w as u64 * h as u64 > MOST_PIXELS {
        return Err(format!("the image is {}×{}, more than the {} megapixels converted", w, h, MOST_PIXELS / 1_000_000));
    }
    // Turned a quarter, it is turned into a copy, and cropped as it is shown (FEEDBACK55.md, Q-825;
    // branch feedback55).
    let quarter = turned(d.orientation().unwrap_or(Orientation::NoTransforms));
    let (w, h) = if quarter { (h, w) } else { (w, h) };
    let (cw, ch) = job.crop.map_or((w as u64, h as u64), |(_, _, cw, ch)| (cw.min(w) as u64, ch.min(h) as u64));
    let mut bytes = w as u64 * h as u64 * bpp * if quarter { 2 } else { 1 };
    let (ow, oh) = match job.width {
        Some(nw) if (nw as u64) < cw && cw > 0 => {
            let nh = (ch * nw as u64 / cw).max(1);
            bytes += cw * nh * 16 + nw as u64 * nh * bpp;
            (nw as u64, nh)
        }
        _ => (cw, ch),
    };
    bytes += ow * oh * 4;
    // Laid on white for a JPEG, through a copy with its alpha (FEEDBACK55.md, Q-826; branch
    // feedback55).
    if alpha && job.format == ImageFormat::Jpeg {
        bytes += ow * oh * 4;
    }
    if bytes > MOST_BYTES_ONE {
        return Err(format!("converting the {}×{} image so would take about {} MiB, more than the {} converted at once", w, h, bytes >> 20, MOST_BYTES_ONE >> 20));
    }
    Ok(bytes)
}

fn convert(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let src = path_arg(it, &args, 0, "image.convert", line)?;
    let dest = path_arg(it, &args, 1, "image.convert", line)?;
    // Not over the program's own files, as `write_file` will not: `.env`, `main.qwe` and the
    // database were written over (FEEDBACK51.md, Q-658; branch feedback51-fixes).
    if let Some(why) = crate::builtins::own_file(it, &dest, false) {
        return Ok(err_value(format!("will not write {}: {}", args.get(1).map(|v| v.display()).unwrap_or_default(), why)));
    }
    let job = job_of(args.get(2), std::path::Path::new(&dest), line)?;
    let said = matches!(args.get(2), Some(Value::Map(m)) if !matches!(m.get("format"), None | Some(Value::Nil)));
    // A folder is said as the write's own failure.
    if !said && !std::path::Path::new(&dest).is_dir() && format_of_name(std::path::Path::new(&dest)).is_none() {
        return Ok(err_value(format!(
            "{}: its name says no format to write: end it in .jpg, .png or .webp, or give format: \"jpeg\", \"png\" or \"webp\"",
            args.get(1).map(|v| v.display()).unwrap_or_default()
        )));
    }
    Ok(match it.unlocked(move || cost_of(&src, &job).and_then(|bytes| in_turn(bytes, || make(&src, &dest, job)))) {
        Ok((w, h)) => {
            let mut m = Map::new();
            m.insert("width".into(), Value::Int(w as i64));
            m.insert("height".into(), Value::Int(h as i64));
            Value::map(m)
        }
        Err(e) => err_value(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_few_images_are_converted_at_once() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        // What the review saw: as many conversions at once as uploads.
        static NOW: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);
        let threads: Vec<_> = (0..12)
            .map(|_| {
                std::thread::spawn(|| {
                    in_turn(1, || {
                        let n = NOW.fetch_add(1, SeqCst) + 1;
                        MOST.fetch_max(n, SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        NOW.fetch_sub(1, SeqCst);
                        Ok(())
                    })
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap().unwrap();
        }
        assert!(MOST.load(SeqCst) <= most_converting(), "{} at once", MOST.load(SeqCst));
    }

    #[test]
    fn large_images_take_turns_by_their_memory() {
        use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
        // Three of 400 MiB: two at once fit in 1 GiB, three do not (Q-674).
        static NOW: AtomicU64 = AtomicU64::new(0);
        static MOST: AtomicU64 = AtomicU64::new(0);
        let threads: Vec<_> = (0..3)
            .map(|_| {
                std::thread::spawn(|| {
                    in_turn(400 << 20, || {
                        let n = NOW.fetch_add(400 << 20, SeqCst) + (400 << 20);
                        MOST.fetch_max(n, SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(60));
                        NOW.fetch_sub(400 << 20, SeqCst);
                        Ok(())
                    })
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap().unwrap();
        }
        assert!(MOST.load(SeqCst) <= BYTES_AT_ONCE, "{} bytes at once", MOST.load(SeqCst));
        // One larger than the budget alone still goes, when nothing else runs.
        assert_eq!(in_turn(BYTES_AT_ONCE + 1, || Ok(7)), Ok(7));
    }
}
