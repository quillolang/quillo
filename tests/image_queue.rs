//! `image.convert` when too many wait their turn. Its own test file: the queue
//! is the process's, and another conversion running beside this would be refused.

use quillo::value::{lookup, Value};
use quillo::Interp;
use std::sync::{Arc, Barrier};

#[test]
fn past_those_who_may_wait_a_conversion_is_refused_at_once() {
    let d = std::env::temp_dir().join(format!("quillo-image-queue-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // Noise, slow to read: the conversions pile up behind the few that run.
    let mut seed = 1u32;
    let noise = image::RgbImage::from_fn(1500, 1500, |_, _| {
        seed = seed.wrapping_mul(2654435761).wrapping_add(7);
        image::Rgb([(seed >> 24) as u8, (seed >> 16) as u8, (seed >> 8) as u8])
    });
    noise.save(d.join("a.png")).unwrap();
    let n = 200;
    let start = Arc::new(Barrier::new(n));
    let threads: Vec<_> = (0..n)
        .map(|i| {
            let (d, start) = (d.clone(), start.clone());
            std::thread::spawn(move || {
                let mut it = Interp::new(&d);
                it.load_source(&format!("fn go() = image.convert(\"a.png\", \"out/{}.jpg\", {{ width: 100 }})\n", i), "t.qwe").unwrap();
                let go: Value = lookup(&it.globals, "go").unwrap();
                start.wait();
                match it.call(&go, vec![], 0) {
                    Ok(v) => v.repr(),
                    Err(_) => panic!("the conversion stopped the program"),
                }
            })
        })
        .collect();
    let answers: Vec<String> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let done = answers.iter().filter(|a| *a == "{ width: 100, height: 100 }").count();
    let refused = answers.iter().filter(|a| *a == "err(\"image: too many images are being converted at once; try again shortly\")").count();
    assert_eq!(done + refused, n, "{:?}", answers.iter().find(|a| !a.contains("width") && !a.contains("too many")));
    assert!(refused > 0, "all {} waited", n);
    assert!(done >= 64, "only {} were done", done);
    let _ = std::fs::remove_dir_all(&d);
}
