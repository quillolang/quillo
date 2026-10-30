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
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
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
        let d = r.into_decoder().map_err(|e| e.to_string())?;
        let (w, h) = d.dimensions();
        if w > MOST_SIDE || h > MOST_SIDE {
            return Err(format!("{}×{} pixels is larger than {} a side", w, h, MOST_SIDE));
        }
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

fn job_of(opts: Option<&Value>, line: u32) -> Result<Job, Ctrl> {
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
    let format = match o.get("format") {
        None | Some(Value::Nil) => ImageFormat::Jpeg,
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
    let mut img: DynamicImage = reader(src)?.decode().map_err(|e| e.to_string())?;
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
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Written beside, then renamed: a request never sees half a file.
    let tmp = dest.with_extension(format!("tmp{}", std::process::id()));
    let out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut w = std::io::BufWriter::new(out);
    let written = match job.format {
        ImageFormat::Jpeg => {
            let rgb = img.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, job.quality).encode_image(&rgb).map_err(|e| e.to_string())
        }
        ImageFormat::WebP => img.to_rgba8().write_with_encoder(image::codecs::webp::WebPEncoder::new_lossless(&mut w)).map_err(|e| e.to_string()),
        _ => img.write_with_encoder(image::codecs::png::PngEncoder::new(&mut w)).map_err(|e| e.to_string()),
    };
    drop(w);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
    Ok((img.width(), img.height()))
}

/// Most images converted at once: each may take up to `MOST_BYTES` to
/// decode, so many uploads at once must not take that many times over.
fn most_converting() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 4)
}

/// Most conversions waiting for their turn; past that, an error at once.
const MOST_WAITING: usize = 64;

/// `f` once fewer than `most_converting()` conversions run, or an error when
/// too many already wait.
fn in_turn<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use std::sync::{Condvar, Mutex};
    // (running, waiting)
    static AT_ONCE: Mutex<(usize, usize)> = Mutex::new((0, 0));
    static FREED: Condvar = Condvar::new();
    let mut n = AT_ONCE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if n.0 >= most_converting() && n.1 >= MOST_WAITING {
        return Err("too many images are being converted at once; try again shortly".into());
    }
    n.1 += 1;
    while n.0 >= most_converting() {
        n = FREED.wait(n).unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    n.1 -= 1;
    n.0 += 1;
    drop(n);
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            AT_ONCE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).0 -= 1;
            FREED.notify_one();
        }
    }
    let _done = Done;
    f()
}

fn convert(it: &mut Interp, args: Vec<Value>, line: u32) -> R {
    let src = path_arg(it, &args, 0, "image.convert", line)?;
    let dest = path_arg(it, &args, 1, "image.convert", line)?;
    let job = job_of(args.get(2), line)?;
    Ok(match it.unlocked(move || in_turn(|| make(&src, &dest, job))) {
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
                    in_turn(|| {
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
}
