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
