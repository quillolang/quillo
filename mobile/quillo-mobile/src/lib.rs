//! The Quillo runtime for phone apps: one call to start a program, for the
//! launcher in mobile/ios (Swift, through C) and mobile/android (Kotlin,
//! through JNI). See `quillo::mobile` for what it does.

use std::ffi::{c_char, CStr};
use std::path::Path;

fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Copies the program bundled at `bundle` into `<data>/app`, starts it, and
/// writes the URL the web view opens into `out` (at most `len` bytes, with a
/// NUL). `device` is a JSON object of what the app knows of the device, for
/// `platform()`. Returns 0; on failure, 1 with the reason in `out`.
///
/// # Safety
/// `bundle`, `data`, `main` and `device` are NUL-terminated strings or null;
/// `out` points to `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn quillo_start(bundle: *const c_char, data: *const c_char, main: *const c_char, device: *const c_char, out: *mut c_char, len: usize) -> i32 {
    let (code, said) = match quillo::mobile::start(Path::new(&text(bundle)), Path::new(&text(data)), &text(main), &text(device)) {
        Ok(s) => (0, s.url),
        Err(e) => (1, e),
    };
    if !out.is_null() && len > 0 {
        let bytes = said.as_bytes();
        let n = bytes.len().min(len - 1);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
    code
}

/// `org.quillolang.app.Quillo.start(bundle, data, main, device)`: the URL to open,
/// or "error: ..." (Android).
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_org_quillolang_app_Quillo_start<'l>(
    mut env: jni::JNIEnv<'l>,
    _this: jni::objects::JObject<'l>,
    bundle: jni::objects::JString<'l>,
    data: jni::objects::JString<'l>,
    main: jni::objects::JString<'l>,
    device: jni::objects::JString<'l>,
) -> jni::sys::jstring {
    let get = |env: &mut jni::JNIEnv<'l>, s: &jni::objects::JString<'l>| -> String { env.get_string(s).map(|s| s.into()).unwrap_or_default() };
    let (b, d, m, dev) = (get(&mut env, &bundle), get(&mut env, &data), get(&mut env, &main), get(&mut env, &device));
    let said = match quillo::mobile::start(Path::new(&b), Path::new(&d), &m, &dev) {
        Ok(s) => s.url,
        Err(e) => format!("error: {}", e),
    };
    env.new_string(said).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

// ---- the program with no socket (FEEDBACK39.md, Q-465 to Q-470; branch feedback36-39) ----
//
// `quillo_open` starts the program in the app's process, and nothing listens: the web view's
// scheme handler gives each request to `quillo_request`, and the page's bridge messages (for
// connections, and bodies the scheme cannot carry) to `quillo_bridge`; what the program sends
// back through the bridge is taken, one message at a time, with `quillo_bridge_next`.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

static PROGRAM: OnceLock<quillo::embed::Program> = OnceLock::new();
static OUTBOX: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static ARRIVED: Condvar = Condvar::new();

fn open(bundle: &str, data: &str, main: &str, device: &str) -> Result<(), String> {
    if PROGRAM.get().is_some() {
        return Ok(());
    }
    let p = quillo::embed::Program::open(Path::new(bundle), Path::new(data), main, device, "mobile")?;
    let _ = PROGRAM.set(p);
    Ok(())
}

fn to_page() -> Arc<dyn Fn(String) + Send + Sync> {
    Arc::new(|json: String| {
        OUTBOX.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push_back(json);
        ARRIVED.notify_one();
    })
}

fn next_message(timeout_ms: u64) -> Option<String> {
    let mut q = OUTBOX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if q.is_empty() {
        q = ARRIVED.wait_timeout(q, std::time::Duration::from_millis(timeout_ms)).unwrap_or_else(std::sync::PoisonError::into_inner).0;
    }
    q.pop_front()
}

fn headers_of(json: &str) -> Vec<(String, String)> {
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json).map(|m| m.into_iter().map(|(k, v)| (k, v.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default()
}

/// An answer whole: its status, its headers as a JSON object (no `Set-Cookie`: the jar keeps
/// those), and its body.
#[repr(C)]
pub struct QuilloAnswer {
    pub status: i32,
    pub headers: *mut c_char,
    pub body: *mut u8,
    pub len: usize,
}

fn answer(method: &str, url: &str, headers: &str, body: Vec<u8>) -> QuilloAnswer {
    let r = match PROGRAM.get() {
        Some(p) => p.request_whole(quillo::embed::request_of(method, url, headers_of(headers), body)),
        None => quillo::server::HttpResponse { status: 503, headers: Vec::new(), body: b"the program is not open\n".to_vec() },
    };
    let hs: serde_json::Map<String, serde_json::Value> = r.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("set-cookie")).map(|(k, v)| (k.clone(), v.clone().into())).collect();
    let headers = std::ffi::CString::new(serde_json::Value::Object(hs).to_string()).unwrap_or_default().into_raw();
    let mut body = r.body.into_boxed_slice();
    let (ptr, len) = (body.as_mut_ptr(), body.len());
    std::mem::forget(body);
    QuilloAnswer { status: r.status as i32, headers, body: ptr, len }
}

fn put(out: *mut c_char, len: usize, said: &str) {
    if out.is_null() || len == 0 {
        return;
    }
    let bytes = said.as_bytes();
    let n = bytes.len().min(len - 1);
    // SAFETY: the caller gives `len` writable bytes at `out`.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, n);
        *out.add(n) = 0;
    }
}

/// Opens the program bundled at `bundle` (copied into `<data>/app`), with no socket. Returns 0;
/// on failure, 1 with the reason in `err`.
///
/// # Safety
/// The strings are NUL-terminated or null; `err` points to `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn quillo_open(bundle: *const c_char, data: *const c_char, main: *const c_char, device: *const c_char, err: *mut c_char, len: usize) -> i32 {
    match open(&text(bundle), &text(data), &text(main), &text(device)) {
        Ok(()) => 0,
        Err(e) => {
            put(err, len, &e);
            1
        }
    }
}

/// One request of the web view's, answered whole. Free the answer with `quillo_free_answer`.
///
/// # Safety
/// The strings are NUL-terminated or null; `body` points to `len` bytes or is null.
#[no_mangle]
pub unsafe extern "C" fn quillo_request(method: *const c_char, url: *const c_char, headers_json: *const c_char, body: *const u8, len: usize) -> *mut QuilloAnswer {
    let body = if body.is_null() || len == 0 { Vec::new() } else { std::slice::from_raw_parts(body, len).to_vec() };
    Box::into_raw(Box::new(answer(&text(method), &text(url), &text(headers_json), body)))
}

/// # Safety
/// `a` came from `quillo_request`, once.
#[no_mangle]
pub unsafe extern "C" fn quillo_free_answer(a: *mut QuilloAnswer) {
    if a.is_null() {
        return;
    }
    let a = Box::from_raw(a);
    if !a.headers.is_null() {
        drop(std::ffi::CString::from_raw(a.headers));
    }
    if !a.body.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(a.body, a.len)));
    }
}

/// A message from the page's bridge (the shell script's).
///
/// # Safety
/// `message` is NUL-terminated or null.
#[no_mangle]
pub unsafe extern "C" fn quillo_bridge(message: *const c_char) {
    if let Some(p) = PROGRAM.get() {
        quillo::embed::bridge_message(p, &text(message), to_page());
    }
}

/// The next message for the page (JSON, for `window.__quilloWire.receive`), waiting up to
/// `timeout_ms`; null when none came. Free it with `quillo_free_text`.
#[no_mangle]
pub extern "C" fn quillo_bridge_next(timeout_ms: u32) -> *mut c_char {
    match next_message(timeout_ms as u64) {
        Some(m) => std::ffi::CString::new(m).map(|c| c.into_raw()).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    }
}

/// # Safety
/// `t` came from `quillo_bridge_next`, once.
#[no_mangle]
pub unsafe extern "C" fn quillo_free_text(t: *mut c_char) {
    if !t.is_null() {
        drop(std::ffi::CString::from_raw(t));
    }
}

/// The script to run before every page; `bridge` is the page's way to post a string to the app.
/// Free it with `quillo_free_text`.
///
/// # Safety
/// `post` and `bodies` are NUL-terminated or null.
#[no_mangle]
pub unsafe extern "C" fn quillo_shell_js(post: *const c_char, bodies: *const c_char) -> *mut c_char {
    std::ffi::CString::new(quillo::embed::shell_js(&text(post), &text(bodies))).map(|c| c.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// The app went behind the home screen, or came back.
#[no_mangle]
pub extern "C" fn quillo_suspend() {
    if let Some(p) = PROGRAM.get() {
        p.suspend();
    }
}

#[no_mangle]
pub extern "C" fn quillo_resume() {
    if let Some(p) = PROGRAM.get() {
        p.resume();
    }
}

// Android: the same calls, through JNI.

#[cfg(target_os = "android")]
mod android {
    use jni::objects::{JByteArray, JObject, JString};
    use jni::sys::{jbyteArray, jint, jstring};
    use jni::JNIEnv;

    fn get(env: &mut JNIEnv, s: &JString) -> String {
        if s.is_null() {
            return String::new();
        }
        env.get_string(s).map(|s| s.into()).unwrap_or_default()
    }

    /// `Quillo.open(bundle, data, main, device)`: "" when it opened, else why not.
    #[no_mangle]
    pub extern "system" fn Java_org_quillolang_app_Quillo_open<'l>(mut env: JNIEnv<'l>, _: JObject<'l>, bundle: JString<'l>, data: JString<'l>, main: JString<'l>, device: JString<'l>) -> jstring {
        let (b, d, m, dev) = (get(&mut env, &bundle), get(&mut env, &data), get(&mut env, &main), get(&mut env, &device));
        let said = super::open(&b, &d, &m, &dev).err().unwrap_or_default();
        env.new_string(said).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    /// `Quillo.request(method, url, headersJson, body)`: the answer as bytes: its status (4 bytes,
    /// big-endian), the length of its headers' JSON (4 bytes), the JSON, then the body.
    #[no_mangle]
    pub extern "system" fn Java_org_quillolang_app_Quillo_request<'l>(mut env: JNIEnv<'l>, _: JObject<'l>, method: JString<'l>, url: JString<'l>, headers: JString<'l>, body: JByteArray<'l>) -> jbyteArray {
        let (m, u, h) = (get(&mut env, &method), get(&mut env, &url), get(&mut env, &headers));
        let b = if body.is_null() { Vec::new() } else { env.convert_byte_array(&body).unwrap_or_default() };
        let a = super::answer(&m, &u, &h, b);
        // SAFETY: made by `answer` just now, and given up here.
        let (hs, bytes) = unsafe {
            let hs = std::ffi::CString::from_raw(a.headers).into_string().unwrap_or_default();
            (hs, Box::from_raw(std::ptr::slice_from_raw_parts_mut(a.body, a.len)))
        };
        let mut out = Vec::with_capacity(8 + hs.len() + bytes.len());
        out.extend_from_slice(&(a.status as u32).to_be_bytes());
        out.extend_from_slice(&(hs.len() as u32).to_be_bytes());
        out.extend_from_slice(hs.as_bytes());
        out.extend_from_slice(&bytes);
        env.byte_array_from_slice(&out).map(|a| a.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_org_quillolang_app_Quillo_bridge<'l>(mut env: JNIEnv<'l>, _: JObject<'l>, message: JString<'l>) {
        let m = get(&mut env, &message);
        if let Some(p) = super::PROGRAM.get() {
            quillo::embed::bridge_message(p, &m, super::to_page());
        }
    }

    /// `Quillo.bridgeNext(timeoutMs)`: the next message for the page, or null.
    #[no_mangle]
    pub extern "system" fn Java_org_quillolang_app_Quillo_bridgeNext<'l>(env: JNIEnv<'l>, _: JObject<'l>, timeout: jint) -> jstring {
        match super::next_message(timeout.max(0) as u64) {
            Some(m) => env.new_string(m).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
            None => std::ptr::null_mut(),
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_org_quillolang_app_Quillo_shellJs<'l>(mut env: JNIEnv<'l>, _: JObject<'l>, post: JString<'l>, bodies: JString<'l>) -> jstring {
        let (p, b) = (get(&mut env, &post), get(&mut env, &bodies));
        env.new_string(quillo::embed::shell_js(&p, &b)).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_org_quillolang_app_Quillo_suspend<'l>(_: JNIEnv<'l>, _: JObject<'l>) {
        super::quillo_suspend();
    }
}
