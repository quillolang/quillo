//! `platform()`: where a program runs (branch mobile). The operating system and its
//! version, the kind of device and its hardware model, the processor, and how the
//! program runs: in a phone app, in `quillo app`'s window, or as a server.
//!
//! On a server or a desktop the runtime reads these from the system. In a phone
//! app the launcher knows more (the model, the screen, the app's version, whether
//! it is a simulator) and hands it over as JSON when it starts the program
//! (`mobile::start`); what it gives is laid over what the runtime read.

use crate::interp::Ctrl;
use crate::value::*;
use std::sync::OnceLock;

static RUNTIME: OnceLock<&'static str> = OnceLock::new();
static LAUNCHER: OnceLock<serde_json::Value> = OnceLock::new();
static INFO: OnceLock<Value> = OnceLock::new();

/// How the program runs: "mobile", "app" (`quillo app`), or "server" (the default).
pub fn set_runtime(r: &'static str) {
    let _ = RUNTIME.set(r);
}

/// What a phone app's launcher knows of the device, as a JSON object. Ignored
/// when it is not one.
pub fn set_launcher(json: &str) {
    if let Ok(v @ serde_json::Value::Object(_)) = serde_json::from_str::<serde_json::Value>(json) {
        let _ = LAUNCHER.set(v);
    }
}

fn os() -> &'static str {
    if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        std::env::consts::OS
    }
}

/// A `sysctl` value as text, on Apple's systems and the BSDs.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
fn sysctl(name: &str) -> Option<String> {
    let c = std::ffi::CString::new(name).ok()?;
    let mut len: libc::size_t = 0;
    if unsafe { libc::sysctlbyname(c.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) } != 0 || len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    if unsafe { libc::sysctlbyname(c.as_ptr(), buf.as_mut_ptr() as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0) } != 0 {
        return None;
    }
    let s = String::from_utf8_lossy(&buf[..len]).trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(unix)]
fn kernel_release() -> Option<String> {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return None;
    }
    let s = unsafe { std::ffi::CStr::from_ptr(u.release.as_ptr()) }.to_string_lossy().into_owned();
    (!s.is_empty()).then_some(s)
}

#[cfg(not(unix))]
fn kernel_release() -> Option<String> {
    None
}

/// /etc/os-release's NAME and VERSION_ID, on Linux: ("Ubuntu", "24.04").
#[cfg_attr(any(target_os = "macos", target_os = "ios", target_os = "freebsd"), allow(dead_code))]
fn os_release() -> Option<(String, String)> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    let get = |k: &str| {
        text.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('='))).map(|v| v.trim_matches('"').to_string())
    };
    Some((get("NAME")?, get("VERSION_ID").unwrap_or_default()))
}

/// The system's name for itself, its version and the hardware model, as each
/// system tells them.
#[cfg(target_os = "macos")]
fn system_names(_os: &str) -> (String, Option<String>, Option<String>) {
    ("macOS".into(), sysctl("kern.osproductversion"), sysctl("hw.model"))
}

#[cfg(target_os = "ios")]
fn system_names(_os: &str) -> (String, Option<String>, Option<String>) {
    ("iOS".into(), sysctl("kern.osproductversion"), sysctl("hw.machine"))
}

#[cfg(target_os = "freebsd")]
fn system_names(_os: &str) -> (String, Option<String>, Option<String>) {
    ("FreeBSD".into(), kernel_release(), sysctl("hw.model"))
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd")))]
fn system_names(os: &str) -> (String, Option<String>, Option<String>) {
    let model = std::fs::read_to_string("/sys/devices/virtual/dmi/id/product_name").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    match os_release() {
        Some((name, version)) => (name, Some(version).filter(|v| !v.is_empty()), model),
        None if os == "windows" => ("Windows".into(), None, None),
        None => (os.to_string(), None, model),
    }
}

/// What the runtime reads of the system it runs on.
fn read_system() -> serde_json::Map<String, serde_json::Value> {
    use serde_json::json;
    let os = os();
    let (os_name, os_version, model) = system_names(os);
    let runtime = *RUNTIME.get().unwrap_or(&"server");
    let device = match (runtime, os) {
        (_, "ios") | (_, "android") => "phone",
        ("app", _) => "desktop",
        _ => "server",
    };
    let time_zone = jiff::tz::TimeZone::system().iana_name().map(str::to_string);
    // An empty one counts as unset, as for every program that reads them
    // (FEEDBACK16.md, Q-129; branch feedback20-fixes).
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).map(|l| l.split('.').next().unwrap_or("").replace('_', "-")).filter(|l| !l.is_empty() && l != "C" && l != "POSIX");
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    match json!({
        "os": os,
        "os_name": os_name,
        "os_version": os_version,
        "kernel": kernel_release(),
        "device": device,
        "model": model,
        "arch": std::env::consts::ARCH,
        "cpus": cpus,
        "simulator": false,
        "runtime": runtime,
        "locale": locale,
        "time_zone": time_zone,
        "app": null,
        "screen": null,
        "quillo": crate::version(),
    }) {
        serde_json::Value::Object(m) => m,
        _ => unreachable!(),
    }
}

/// The map `platform()` gives: what the runtime read, with what a phone's
/// launcher said laid over it. Read once: it does not change while the program runs.
pub fn info() -> Value {
    INFO.get_or_init(|| {
        let m = read_system();
        let m = match LAUNCHER.get() {
            Some(serde_json::Value::Object(given)) => overlay(m, given),
            _ => m,
        };
        Value::from_json(&serde_json::Value::Object(m))
    })
    .clone()
}

/// What the system said, with what the launcher gave laid over it; a null
/// from the launcher does not hide what the system said.
fn overlay(mut m: serde_json::Map<String, serde_json::Value>, given: &serde_json::Map<String, serde_json::Value>) -> serde_json::Map<String, serde_json::Value> {
    for (k, v) in given {
        if !v.is_null() {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}

pub fn platform(_: &mut crate::interp::Interp, _: Vec<Value>, _: u32) -> Result<Value, Ctrl> {
    Ok(info())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_system_is_read_and_what_a_launcher_says_wins() {
        let m = super::read_system();
        assert_eq!(m["os"], std::env::consts::OS);
        assert_eq!(m["arch"], std::env::consts::ARCH);
        assert!(m["quillo"].as_str().unwrap().starts_with(env!("CARGO_PKG_VERSION")));
        #[cfg(target_os = "macos")]
        assert!(m["os_version"].as_str().is_some_and(|v| v.contains('.')), "{:?}", m["os_version"]);
        // Tested apart from `info`, whose launcher facts are set once per
        // process, as another test (mobile.rs) sets them too (branch
        // feedback18-fixes).
        let given: serde_json::Value = serde_json::from_str(r#"{ "model": "iPhone17,3", "screen": { "width": 402, "height": 874, "scale": 3 }, "os_version": null }"#).unwrap();
        let o = super::overlay(m.clone(), given.as_object().unwrap());
        assert_eq!(o["model"], "iPhone17,3");
        assert_eq!(o["screen"]["scale"], 3);
        // A null from the launcher does not hide what the system said.
        assert_eq!(o["os_version"], m["os_version"]);
        assert!(matches!(super::info(), crate::value::Value::Map(_)));
    }
}
