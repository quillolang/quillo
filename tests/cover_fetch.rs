//! `http` past the usual path (src/fetch.rs): answers after an interim (1xx) one read as HTTP
//! frames them, chunked bodies that go wrong, TLS with a CA file and a client certificate of the
//! program's, the same TLS settings given twice, a proxy from the environment that is skipped,
//! and a redirect a policy follows. Every server is one this test starts, on this machine.

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn ev(src: &str) -> String {
    match quillo::eval_str(src) {
        Ok(v) => v.repr(),
        Err(e) => format!("ERROR: {}", e),
    }
}

fn dir(name: &str) -> PathBuf {
    support::temp_dir(&format!("cover-fetch-{}", name))
}

/// The head of a request: read up to its blank line.
fn read_head(s: &mut impl Read) -> String {
    let (mut buf, mut b) = (Vec::new(), [0u8; 1]);
    while !buf.ends_with(b"\r\n\r\n") {
        match s.read(&mut b) {
            Ok(1) => buf.push(b[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// A server answering each request with what `f` gives for its path, then closing.
fn raw_server(f: impl Fn(&str) -> Vec<u8> + Send + Sync + 'static) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let f = Arc::new(f);
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let f = f.clone();
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let head = read_head(&mut s);
                let path = head.split(' ').nth(1).unwrap_or("/").to_string();
                let _ = s.write_all(&f(&path));
                let _ = s.flush();
            });
        }
    });
    port
}

const EARLY: &[u8] = b"HTTP/1.1 103 Early Hints\r\nLink: </a.css>; rel=preload\r\n\r\n";

fn after_early(rest: &[u8]) -> Vec<u8> {
    let mut v = EARLY.to_vec();
    v.extend_from_slice(rest);
    v
}

fn gzip(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(b).unwrap();
    e.finish().unwrap()
}

fn chunked(body: &[u8], size: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for c in body.chunks(size) {
        v.extend_from_slice(format!("{:x}\r\n", c.len()).as_bytes());
        v.extend_from_slice(c);
        v.extend_from_slice(b"\r\n");
    }
    v.extend_from_slice(b"0\r\n\r\n");
    v
}

fn interim_peer() -> u16 {
    raw_server(|path| match path {
        "/cookies" => after_early(b"HTTP/1.1 200 OK\r\nSet-Cookie: a=1\r\nSet-Cookie: b=2\r\nX-Two: one\r\nX-Two: two\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"),
        "/nothing" => after_early(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"),
        "/to-close" => after_early(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nuntil the end"),
        "/big-chunk" => {
            let mut v = after_early(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
            v.extend(chunked(&vec![b'q'; 40_000], 20_000));
            v
        }
        "/chunked-gz" => {
            let mut v = after_early(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\n");
            v.extend(chunked(&gzip(b"zipped in chunks after hints"), 7));
            v
        }
        "/long-size" => {
            let mut v = after_early(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
            v.extend(vec![b'1'; 5000]);
            v.extend_from_slice(b"\r\nx\r\n0\r\n\r\n");
            v
        }
        "/cut-chunk" => after_early(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n20\r\nonly part of it"),
        _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    })
}

#[test]
fn an_answer_after_an_interim_one_keeps_each_cookie_and_joins_repeated_headers() {
    let p = interim_peer();
    let got = ev(&format!("let r = http.get(\"http://127.0.0.1:{}/cookies\", {{}}, {{ timeout: 5 }})\n[r.status, r.body, r.cookies, r.headers[\"x-two\"], r.headers[\"set-cookie\"]]", p));
    assert_eq!(got, "[200, \"ok\", [\"a=1\", \"b=2\"], \"one, two\", \"a=1\\nb=2\"]");
}

#[test]
fn a_204_after_an_interim_answer_has_no_body() {
    let p = interim_peer();
    assert_eq!(ev(&format!("let r = http.get(\"http://127.0.0.1:{}/nothing\", {{}}, {{ timeout: 5 }})\n[r.status, r.body]", p)), "[204, \"\"]");
}

#[test]
fn a_body_of_no_stated_length_after_an_interim_answer_runs_until_the_connection_closes() {
    let p = interim_peer();
    assert_eq!(ev(&format!("http.get(\"http://127.0.0.1:{}/to-close\", {{}}, {{ timeout: 5 }}).body", p)), "\"until the end\"");
}

#[test]
fn chunks_after_an_interim_answer_are_put_together_and_bad_ones_are_errors() {
    let p = interim_peer();
    assert_eq!(ev(&format!("len(http.get(\"http://127.0.0.1:{}/big-chunk\", {{}}, {{ timeout: 5 }}).body)", p)), "40000", "chunks larger than one read");
    assert_eq!(ev(&format!("http.get(\"http://127.0.0.1:{}/chunked-gz\", {{}}, {{ timeout: 5 }}).body", p)), "\"zipped in chunks after hints\"");
    let long = ev(&format!("http.get(\"http://127.0.0.1:{}/long-size\", {{}}, {{ timeout: 5 }})", p));
    assert!(long.contains("a chunk's size line is too long"), "{}", long);
    let cut = ev(&format!("http.get(\"http://127.0.0.1:{}/cut-chunk\", {{}}, {{ timeout: 5 }})", p));
    assert!(cut.starts_with("err(") && !cut.contains("only part"), "a chunk cut short is an error, not half a body: {}", cut);
    // The same, read as it arrives.
    let streamed = ev(&format!("let n = 0\nlet r = http.stream(\"http://127.0.0.1:{}/big-chunk\", (t) => {{ n = n + len(t) }}, {{ timeout: 5 }})\n[n, r.status]", p));
    assert_eq!(streamed, "[40000, 200]");
}

// ---- TLS ---------------------------------------------------------------------------------

struct Pki {
    dir: PathBuf,
    server: rustls::ServerConfig,
}

/// A CA, a server certificate for 127.0.0.1 and a client certificate, both signed by it.
fn pki(name: &str) -> Pki {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let d = dir(name);
    let ca_key = rcgen::KeyPair::generate().unwrap();
    let mut ca = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca.distinguished_name.push(rcgen::DnType::CommonName, "cover fetch ca");
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    std::fs::write(d.join("ca.pem"), ca_cert.pem()).unwrap();
    let ca_der: CertificateDer<'static> = ca_cert.der().clone();
    let issuer = rcgen::Issuer::new(ca, ca_key);
    let server_key = rcgen::KeyPair::generate().unwrap();
    let server_cert = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()]).unwrap().signed_by(&server_key, &issuer).unwrap();
    let client_key = rcgen::KeyPair::generate().unwrap();
    let mut cp = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    cp.distinguished_name.push(rcgen::DnType::CommonName, "the program");
    let client_cert = cp.signed_by(&client_key, &issuer).unwrap();
    std::fs::write(d.join("client.pem"), client_cert.pem()).unwrap();
    std::fs::write(d.join("client.key"), client_key.serialize_pem()).unwrap();
    std::fs::write(d.join("empty.pem"), "").unwrap();
    std::fs::write(d.join("junk.key"), "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n").unwrap();

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_der).unwrap();
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone()).allow_unauthenticated().build().unwrap();
    let server = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![server_cert.der().clone()], PrivateKeyDer::Pkcs8(server_key.serialize_der().into()))
        .unwrap();
    Pki { dir: d, server }
}

/// An HTTPS server on 127.0.0.1 saying whether the client showed a certificate; its port.
fn https_server(cfg: rustls::ServerConfig) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let cfg = Arc::new(cfg);
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { return };
            let cfg = cfg.clone();
            std::thread::spawn(move || {
                s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let Ok(conn) = rustls::ServerConnection::new(cfg) else { return };
                let mut tls = rustls::StreamOwned::new(conn, s);
                let _ = read_head(&mut tls);
                let who = if tls.conn.peer_certificates().is_some_and(|c| !c.is_empty()) { "with a certificate" } else { "anonymous" };
                let _ = write!(tls, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", who.len(), who);
                tls.conn.send_close_notify();
                let _ = tls.flush();
            });
        }
    });
    port
}

fn p(d: &Path, f: &str) -> String {
    d.join(f).display().to_string()
}

#[test]
fn a_ca_file_is_trusted_and_the_same_settings_twice_are_one_client() {
    let k = pki("ca");
    let port = https_server(k.server);
    let src = format!("http.get(\"https://127.0.0.1:{}/\", {{}}, {{ ca: {:?}, timeout: 5 }}).body", port, p(&k.dir, "ca.pem"));
    assert_eq!(ev(&src), "\"anonymous\"");
    assert_eq!(ev(&src), "\"anonymous\"", "the same CA again");
    // Without it, the usual public CAs do not know this one.
    let plain = ev(&format!("http.get(\"https://127.0.0.1:{}/\", {{}}, {{ timeout: 5 }})", port));
    assert!(plain.starts_with("err(") && !plain.contains("anonymous"), "{}", plain);
}

#[test]
fn a_client_certificate_is_presented_and_a_bad_one_is_said() {
    let k = pki("mtls");
    let d = k.dir.clone();
    let port = https_server(k.server);
    let call = |cert: &str, key: &str| ev(&format!("http.get(\"https://127.0.0.1:{}/\", {{}}, {{ ca: {:?}, cert: {:?}, key: {:?}, timeout: 5 }})", port, p(&d, "ca.pem"), p(&d, cert), p(&d, key)));
    assert!(call("client.pem", "client.key").contains("body: \"with a certificate\""), "{}", call("client.pem", "client.key"));
    let empty = call("empty.pem", "client.key");
    assert!(empty.contains("no certificate in it (a PEM file is wanted)") && empty.contains("empty.pem"), "{}", empty);
    let missing = call("client.pem", "missing.key");
    assert!(missing.contains("key ") && missing.contains("missing.key"), "{}", missing);
    let junk = call("client.pem", "junk.key");
    assert!(junk.contains("cert and key: "), "{}", junk);
    // A CA file with no certificate in it.
    let none = ev(&format!("http.get(\"https://127.0.0.1:{}/\", {{}}, {{ ca: {:?}, timeout: 5 }})", port, p(&d, "empty.pem")));
    assert!(none.contains("no certificate in it"), "{}", none);
}

// ---- proxies and policies, in a process of their own ---------------------------------------

/// `quillo` with these arguments, given 60 seconds at most.
fn quillo(d: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quillo"));
    c.args(args).current_dir(d).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for k in ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"] {
        c.env_remove(k);
    }
    c.envs(env.iter().copied());
    let child = c.spawn().unwrap();
    support::stop_with_tests(&child);
    let id = child.id();
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let d2 = done.clone();
    std::thread::spawn(move || {
        let until = std::time::Instant::now() + Duration::from_secs(60);
        while !d2.load(Ordering::SeqCst) {
            if std::time::Instant::now() > until {
                let _ = Command::new("kill").args(["-KILL", &id.to_string()]).status();
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    let o = child.wait_with_output().unwrap();
    done.store(true, Ordering::SeqCst);
    o
}

/// A proxy that answers every request itself, counting them.
fn proxy() -> (u16, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    let port = raw_server(move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
        b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nproxied".to_vec()
    });
    (port, n)
}

#[test]
fn a_proxy_is_skipped_for_every_host_and_for_this_machines_ipv6_address() {
    let (pp, used) = proxy();
    // A server on [::1], reached directly although a proxy is set.
    let l = TcpListener::bind("[::1]:0").unwrap();
    let v6 = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let _ = read_head(&mut s);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\ndirect");
        }
    });
    let d = dir("proxy");
    std::fs::write(
        d.join("main.qwe"),
        format!(
            "print(http.get(\"http://[::1]:{}/\", {{}}, {{ timeout: 5 }}).body)\nprint(error(http.get(\"http://10.255.255.1/\", {{}}, {{ public_only: true, timeout: 2 }})))\n",
            v6
        ),
    )
    .unwrap();
    let proxy_url = format!("http://127.0.0.1:{}", pp);
    // NO_PROXY "*": no host goes through it.
    let o = quillo(&d, &["run", "main.qwe"], &[("HTTP_PROXY", &proxy_url), ("NO_PROXY", "*")]);
    let out = String::from_utf8_lossy(&o.stdout);
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("direct"), "{}\n{}", out, String::from_utf8_lossy(&o.stderr));
    assert!(lines.next().is_some_and(|l| l.contains("10.255.255.1")), "{}", out);
    // Through the proxy, `public_only` looks the address up first, here, and refuses it.
    let o = quillo(&d, &["run", "main.qwe"], &[("HTTP_PROXY", &proxy_url)]);
    let out = String::from_utf8_lossy(&o.stdout);
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("direct"), "[::1] is this machine: never through the proxy\n{}", out);
    let refused = lines.next().unwrap_or("");
    assert!(refused.contains("refused by public_only: 10.255.255.1 is not on the public internet"), "{}\n{}", out, String::from_utf8_lossy(&o.stderr));
    assert_eq!(used.load(Ordering::SeqCst), 0, "the proxy was never asked");
}

#[test]
fn under_a_policy_a_post_redirected_by_a_303_is_followed_as_a_get_with_no_body() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let s2 = seen.clone();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let head = read_head(&mut s);
            let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
            let mut body = vec![0u8; len];
            let _ = s.read_exact(&mut body);
            let first = head.lines().next().unwrap_or("").to_string();
            let ctype = head.lines().any(|l| l.to_ascii_lowercase().starts_with("content-type:"));
            s2.lock().unwrap().push(format!("{} body={} ctype={}", first, String::from_utf8_lossy(&body), ctype));
            let reply: &[u8] = if first.contains("/form") { b"HTTP/1.1 303 See Other\r\nLocation: /done\r\nContent-Length: 0\r\nConnection: close\r\n\r\n" } else { b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndone" };
            let _ = s.write_all(reply);
        }
    });
    let d = dir("policy-303");
    std::fs::write(d.join("hosts.toml"), "[http]\nhosts = [\"127.0.0.1\"]\n").unwrap();
    std::fs::write(d.join("main.qwe"), format!("let r = http.post(\"http://127.0.0.1:{}/form\", {{ a: 1 }}, {{}}, {{ timeout: 5 }})\nprint(r.status, r.body)\n", port)).unwrap();
    let o = quillo(&d, &["run", "main.qwe", "--policy", "hosts.toml"], &[]);
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "200 done", "{}", String::from_utf8_lossy(&o.stderr));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "{:?}", seen);
    assert!(seen[0].starts_with("POST /form") && seen[0].contains("body={\"a\":1}"), "{:?}", seen);
    assert!(seen[1].starts_with("GET /done") && seen[1].ends_with("body= ctype=false"), "{:?}", seen);
}

#[allow(dead_code)]
fn unused(_: TcpStream) {}
