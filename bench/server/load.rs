// A small HTTP/1.1 load generator for run.js: keep-alive connections, each
// sending a request when its last answer is in, spread over threads.
//
//   rustc -O load.rs && ./load URL CONNECTIONS THREADS SECONDS
//
// Prints JSON: requests a second, latency percentiles (ms), errors, and a
// sample answer's status and length.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn read_answer(s: &mut TcpStream, buf: &mut Vec<u8>) -> std::io::Result<(u16, usize)> {
    buf.clear();
    let mut chunk = [0u8; 65536];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = s.read(&mut chunk)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_ascii_lowercase();
    let status: u16 = head.get(9..12).and_then(|x| x.parse().ok()).unwrap_or(0);
    let header = |name: &str| head.lines().find_map(|l| l.strip_prefix(name).map(|v| v.trim().to_string()));
    if let Some(len) = header("content-length:").and_then(|v| v.parse::<usize>().ok()) {
        while buf.len() < head_end + len {
            let n = s.read(&mut chunk)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        return Ok((status, len));
    }
    if header("transfer-encoding:").map_or(false, |v| v.contains("chunked")) {
        // Until the last chunk's "0\r\n\r\n".
        while !buf[head_end..].ends_with(b"0\r\n\r\n") {
            let n = s.read(&mut chunk)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        return Ok((status, buf.len() - head_end));
    }
    Ok((status, 0))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let url = &a[1];
    let conns: usize = a[2].parse().unwrap();
    let threads: usize = a[3].parse().unwrap();
    let secs: f64 = a[4].parse().unwrap();
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_at(rest.find('/').unwrap());
    let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nAccept: */*\r\n\r\n", path, host).into_bytes();
    let stop = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    let mut handles = Vec::new();
    for t in 0..threads {
        let n = conns / threads + usize::from(t < conns % threads);
        let (host, req, stop) = (host.to_string(), req.clone(), stop.clone());
        handles.push(std::thread::spawn(move || {
            let mut socks: Vec<TcpStream> = (0..n).map(|_| { let s = TcpStream::connect(&host).unwrap(); s.set_nodelay(true).unwrap(); s }).collect();
            let mut lat: Vec<u32> = Vec::with_capacity(1 << 20);
            let (mut errors, mut sample) = (0u64, (0u16, 0usize));
            let mut buf = Vec::with_capacity(1 << 16);
            // One request in flight on each connection; served in turn.
            let mut sent: Vec<Instant> = Vec::with_capacity(n);
            for s in socks.iter_mut() {
                s.write_all(&req).unwrap();
                sent.push(Instant::now());
            }
            while !stop.load(Ordering::Relaxed) {
                for (i, s) in socks.iter_mut().enumerate() {
                    match read_answer(s, &mut buf) {
                        Ok(r) => {
                            lat.push(sent[i].elapsed().as_micros() as u32);
                            if r.0 != 200 { errors += 1; }
                            sample = r;
                        }
                        Err(_) => { errors += 1; }
                    }
                    if s.write_all(&req).is_err() { errors += 1; }
                    sent[i] = Instant::now();
                }
            }
            (lat, errors, sample)
        }));
    }
    std::thread::sleep(Duration::from_secs_f64(secs));
    stop.store(true, Ordering::Relaxed);
    let elapsed = start.elapsed().as_secs_f64();
    let (mut lat, mut errors, mut sample) = (Vec::new(), 0, (0, 0));
    for h in handles {
        let (l, e, s) = h.join().unwrap();
        lat.extend(l);
        errors += e;
        sample = s;
    }
    lat.sort_unstable();
    let p = |q: f64| if lat.is_empty() { 0.0 } else { lat[((lat.len() as f64 - 1.0) * q) as usize] as f64 / 1000.0 };
    println!(
        "{{\"rps\": {:.0}, \"p50\": {:.3}, \"p99\": {:.3}, \"errors\": {}, \"status\": {}, \"bytes\": {}}}",
        lat.len() as f64 / elapsed, p(0.5), p(0.99), errors, sample.0, sample.1
    );
}
