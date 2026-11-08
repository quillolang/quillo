//! What whoever runs Quillo lets a program do (FEEDBACK41.md, Q-478; branch feedback40-43).
//!
//! ```toml
//! # /etc/quillo/policy.toml: the host's, for every program on the server
//! [plugins]
//! build = false                  # never run cargo when loading: prebuilt libraries only
//! allow = ["csv@1", "qr", "jwt@2.1.0"]
//!
//! [effects]
//! allow = ["db", "http", "fs", "time", "random"]
//!
//! [http]
//! private = false                # no loopback, private or link-local addresses
//! hosts = ["*"]
//!
//! [env]
//! names = ["QUILLO_SECRET", "DATABASE_URL"]
//!
//! [limits]
//! workers = 8
//! max_compute = "10s"
//! max_body = "10MB"
//! max_tool_calls = 60            # a minute, for each caller of /_quillo/mcp
//! ```
//!
//! The host's file comes first, then any `--policy FILE`, then `quillo.policy` beside the
//! program. Each is a layer: a later one can only narrow, never widen, as each list is a
//! condition every layer must meet. What one refuses says which file refused it.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The host's policy, always read when it is there.
pub const HOST_FILE: &str = "/etc/quillo/policy.toml";
/// The program's own, beside its main file.
pub const PROGRAM_FILE: &str = "quillo.policy";

#[derive(Debug, Clone)]
pub struct Policy {
    /// The files read, in order.
    pub files: Vec<PathBuf>,
    /// May cargo run when a plugin is used (false in any layer: never).
    pub build: bool,
    /// Where a build was forbidden.
    pub build_from: Option<PathBuf>,
    /// Each layer's plugins: `name` or `name@version`, the version a prefix of the plugin's.
    pub plugins: Vec<(PathBuf, Vec<String>)>,
    /// The effects every layer allows.
    pub effects: u8,
    /// The first layer that took effects away.
    pub effects_from: Option<PathBuf>,
    /// May `http` reach loopback, private and link-local addresses.
    pub private: bool,
    pub private_from: Option<PathBuf>,
    /// Each layer's hosts: `api.stripe.com`, `*.mapbox.com`, `*`.
    pub hosts: Vec<(PathBuf, Vec<String>)>,
    /// Each layer's environment variables `env()` may read.
    pub env: Vec<(PathBuf, Vec<String>)>,
    pub workers: Option<usize>,
    pub max_compute: Option<Duration>,
    pub max_body: Option<usize>,
    /// Tool calls a minute each caller may make at the MCP door, as `--max-tool-calls` says it
    /// (FEEDBACK56.md, C.4, Q-860; branch feedback56).
    pub max_tool_calls: Option<usize>,
    /// The file each limit came from, for the messages that hold a program to it (FEEDBACK55.md,
    /// Q-849; branch feedback55).
    pub max_compute_from: Option<PathBuf>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            files: Vec::new(),
            build: true,
            build_from: None,
            plugins: Vec::new(),
            effects: crate::ast::ALL_EFFECTS,
            effects_from: None,
            private: true,
            private_from: None,
            hosts: Vec::new(),
            env: Vec::new(),
            workers: None,
            max_compute: None,
            max_body: None,
            max_tool_calls: None,
            max_compute_from: None,
        }
    }
}

static POLICY: OnceLock<Policy> = OnceLock::new();

/// The policy this process runs under: none until `set`.
pub fn get() -> &'static Policy {
    static NONE: OnceLock<Policy> = OnceLock::new();
    POLICY.get().unwrap_or_else(|| NONE.get_or_init(Policy::default))
}

/// Sets the policy, once, before the program loads.
pub fn set(p: Policy) {
    let _ = POLICY.set(p);
}

/// The effects nothing in the program may go past.
pub fn ceiling() -> u8 {
    get().effects
}

/// The host's file, then `extra` (`--policy`), then the program's: each read that is there.
pub fn load(main: &Path, extra: &[PathBuf]) -> Result<Policy, String> {
    let mut p = Policy::default();
    let host = PathBuf::from(HOST_FILE);
    let program = main.parent().unwrap_or(Path::new(".")).join(PROGRAM_FILE);
    let mut files = Vec::new();
    // Only a host's file that is not there is no policy: one that cannot be read is an error, not
    // a policy skipped without a word.
    match std::fs::symlink_metadata(&host) {
        Ok(_) => files.push(host),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot read the policy {}: {}", host.display(), e)),
    }
    files.extend(extra.iter().cloned());
    if program.is_file() {
        files.push(program);
    }
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("cannot read the policy {}: {}", f.display(), e))?;
        p.add(&f, &text)?;
    }
    Ok(p)
}

/// A value of the file's small TOML.
#[derive(Debug, Clone, PartialEq)]
enum V {
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<String>),
}

/// `[section]` and `key = value` lines: booleans, integers, strings, and lists of strings
/// (which may span lines); `#` begins a comment.
fn parse(text: &str) -> Result<Vec<(String, String, V, usize)>, String> {
    // A byte-order mark is no part of the first line, as in `.qwe` and `.env` files (FEEDBACK55.md,
    // Q-852; branch feedback55).
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut out = Vec::new();
    let mut section = String::new();
    let mut lines = text.lines().enumerate().peekable();
    while let Some((n, raw)) = lines.next() {
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        if let Some(s) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.trim().to_string();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { return Err(format!("line {}: expected `key = value` or `[section]`", n + 1)) };
        let mut v = v.trim().to_string();
        // A list may go on over lines.
        while v.starts_with('[') && !v.ends_with(']') {
            match lines.next() {
                Some((_, more)) => {
                    v.push(' ');
                    v.push_str(strip_comment(more).trim());
                }
                None => return Err(format!("line {}: a list with no `]`", n + 1)),
            }
        }
        let value = value(&v).ok_or_else(|| format!("line {}: `{}` is not true, false, a number, \"text\" or a list of \"text\"", n + 1, v))?;
        out.push((section.clone(), k.trim().to_string(), value, n + 1));
    }
    Ok(out)
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

fn value(v: &str) -> Option<V> {
    match v {
        "true" => return Some(V::Bool(true)),
        "false" => return Some(V::Bool(false)),
        _ => {}
    }
    if let Some(s) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return (!s.contains('"')).then(|| V::Str(s.to_string()));
    }
    if let Some(items) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let mut out = Vec::new();
        for item in items.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match value(item)? {
                V::Str(s) => out.push(s),
                _ => return None,
            }
        }
        return Some(V::List(out));
    }
    v.replace('_', "").parse().ok().map(V::Int)
}

/// `"10s"`, `"500ms"`, `"2m"`.
fn duration(s: &str) -> Option<Duration> {
    let (n, unit) = s.find(|c: char| !c.is_ascii_digit()).map_or((s, "s"), |i| (&s[..i], &s[i..]));
    let n: u64 = n.parse().ok()?;
    // In milliseconds that fit, as `--max-compute` reads a time (FEEDBACK55.md, Q-849; branch
    // feedback55).
    n.checked_mul(match unit.trim() {
        "ms" => 1,
        "s" => 1000,
        "m" => 60_000,
        _ => return None,
    })
    .map(Duration::from_millis)
}

/// `"10MB"`, `"512KB"`, `1048576`.
fn bytes(v: &V) -> Option<usize> {
    match v {
        V::Int(n) if *n >= 0 => Some(*n as usize),
        V::Str(s) => size(s),
        _ => None,
    }
}

/// A size as people write one: `10MB`, `512KB`, `2GB`, `100B` or plain bytes, in any case
/// (KB, MB and GB are 1024, 1024² and 1024³ bytes). `--max-body` takes the same (branch
/// feedback40-43).
pub fn size(s: &str) -> Option<usize> {
    let s = s.trim().to_ascii_uppercase();
    let (n, mul) = if let Some(n) = s.strip_suffix("GB") {
        (n, 1usize << 30)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1 << 20)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, 1 << 10)
    } else {
        (s.strip_suffix('B').unwrap_or(&s), 1)
    };
    n.trim().parse::<usize>().ok().and_then(|n| n.checked_mul(mul))
}

impl Policy {
    /// One more layer, read from `file`: it narrows what is allowed.
    pub fn add(&mut self, file: &Path, text: &str) -> Result<(), String> {
        let at = |line: usize, why: String| format!("{}:{}: {}", file.display(), line, why);
        self.files.push(file.to_path_buf());
        for (section, key, v, line) in parse(text).map_err(|e| format!("{}: {}", file.display(), e))? {
            let list = |v: &V| match v {
                V::List(l) => Ok(l.clone()),
                _ => Err(at(line, format!("`{}` is a list: [\"...\", ...]", key))),
            };
            match (section.as_str(), key.as_str(), &v) {
                ("plugins", "build", V::Bool(b)) => {
                    if !b && self.build {
                        self.build = false;
                        self.build_from = Some(file.to_path_buf());
                    }
                }
                ("plugins", "allow", _) => self.plugins.push((file.to_path_buf(), list(&v)?)),
                ("effects", "allow", _) => {
                    let mut mask = 0;
                    for e in list(&v)? {
                        match crate::ast::effect_bit(&e) {
                            0 => return Err(at(line, format!("`{}` is not an effect (they are {})", e, crate::ast::EFFECTS.join(", ")))),
                            b => mask |= b,
                        }
                    }
                    if self.effects & !mask != 0 && self.effects_from.is_none() {
                        self.effects_from = Some(file.to_path_buf());
                    }
                    self.effects &= mask;
                }
                ("http", "private", V::Bool(b)) => {
                    if !b && self.private {
                        self.private = false;
                        self.private_from = Some(file.to_path_buf());
                    }
                }
                ("http", "hosts", _) => self.hosts.push((file.to_path_buf(), list(&v)?)),
                ("env", "names", _) => self.env.push((file.to_path_buf(), list(&v)?)),
                // The command line's ranges: 0 is no limit to nginx and refused by `--max-body 0`,
                // and was taken as one that refused every body, or ended every request at once
                // (FEEDBACK55.md, Q-849; branch feedback55).
                ("limits", "workers", V::Int(n)) if *n > 0 => self.workers = Some(self.workers.map_or(*n as usize, |w| w.min(*n as usize))),
                ("limits", "workers", V::Int(_)) => return Err(at(line, "`workers` is a positive number, as `--workers` takes".into())),
                ("limits", "max_compute", V::Str(s)) => {
                    let d = duration(s).ok_or_else(|| at(line, format!("`{}` is not a time such as \"10s\" or \"500ms\"", s)))?;
                    if d.is_zero() {
                        return Err(at(line, format!("`max_compute` is a time longer than 0, as `--max-compute` takes, not \"{}\"", s)));
                    }
                    if self.max_compute.map_or(true, |m| d < m) {
                        self.max_compute = Some(d);
                        self.max_compute_from = Some(file.to_path_buf());
                    }
                }
                ("limits", "max_body", _) => {
                    let b = bytes(&v).ok_or_else(|| at(line, "`max_body` is a size such as \"10MB\"".into()))?;
                    if b == 0 {
                        return Err(at(line, "`max_body` is a size larger than 0, as `--max-body` takes, such as \"10MB\"".into()));
                    }
                    self.max_body = Some(self.max_body.map_or(b, |m| m.min(b)));
                }
                ("limits", "max_tool_calls", V::Int(n)) if *n > 0 => self.max_tool_calls = Some(self.max_tool_calls.map_or(*n as usize, |m| m.min(*n as usize))),
                ("limits", "max_tool_calls", V::Int(_)) => return Err(at(line, "`max_tool_calls` is a positive number, the calls a minute, as `--max-tool-calls` takes".into())),
                (s, k, _) => {
                    let known = ["plugins.build", "plugins.allow", "effects.allow", "http.private", "http.hosts", "env.names", "limits.workers", "limits.max_compute", "limits.max_body", "limits.max_tool_calls"];
                    let name = format!("{}.{}", s, k);
                    return Err(at(line, if known.contains(&name.as_str()) { format!("`{}` has the wrong kind of value", name) } else { format!("`{}` is not something a policy says (it says {})", name, known.join(", ")) }));
                }
            }
        }
        Ok(())
    }

    fn where_(&self, f: &Option<PathBuf>) -> String {
        f.as_ref().map_or_else(|| "the policy".to_string(), |p| p.display().to_string())
    }

    /// Why plugin `name` (at `version`, its `//! version:`) may not be used, if it may not.
    pub fn plugin(&self, name: &str, version: Option<&str>) -> Result<(), String> {
        for (file, allow) in &self.plugins {
            let ok = allow.iter().any(|a| match a.split_once('@') {
                None => a == name,
                Some((n, v)) => n == name && version.is_some_and(|have| have == v || have.starts_with(&format!("{}.", v))),
            });
            if !ok {
                let said = version.map_or(String::new(), |v| format!(" {}", v));
                return Err(format!("the plugin `{}`{} is not in the plugins {} allows: {}", name, said, file.display(), allow.join(", ")));
            }
        }
        Ok(())
    }

    /// Why `env()` may not read `name`, if it may not.
    pub fn env_name(&self, name: &str) -> Result<(), String> {
        for (file, names) in &self.env {
            if !names.iter().any(|n| n == name) {
                return Err(format!("`{}` is not among the variables {} lets env() read", name, file.display()));
            }
        }
        Ok(())
    }

    /// Whether `http` is held to hosts or kept from private addresses.
    pub fn limits_http(&self) -> bool {
        !self.hosts.is_empty() || !self.private
    }

    /// Why `http` may not reach `url`, if it may not: a host no layer allows, or (with `private =
    /// false`) an address on this machine or its network, as the name resolves now.
    pub fn http(&self, url: &str) -> Result<(), String> {
        if self.hosts.is_empty() && self.private {
            return Ok(());
        }
        let Ok(u) = url::Url::parse(url) else { return Ok(()) };
        let host = u.host_str().unwrap_or("").trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
        for (file, hosts) in &self.hosts {
            if !hosts.iter().any(|h| host_matches(h, &host)) {
                return Err(format!("{} is not among the hosts {} allows: {}", host, file.display(), hosts.join(", ")));
            }
        }
        if !self.private {
            let port = u.port_or_known_default().unwrap_or(80);
            let addrs: Vec<IpAddr> = match host.parse::<IpAddr>() {
                Ok(ip) => vec![ip],
                Err(_) => std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port)).map(|a| a.map(|s| s.ip()).collect()).unwrap_or_default(),
            };
            if let Some(ip) = addrs.into_iter().find(|ip| private_ip(*ip)) {
                let what = if host == ip.to_string() { host.clone() } else { format!("{} ({})", host, ip) };
                return Err(format!("{} is an address of this machine or its network, which {} does not let a program reach", what, self.where_(&self.private_from)));
            }
        }
        Ok(())
    }

    /// What `quillo check --policy` says of it, beside the program's own findings.
    pub fn summary(&self) -> String {
        let mut out = Vec::new();
        if self.effects != crate::ast::ALL_EFFECTS {
            out.push(format!("effects: {}", crate::ast::effect_names(self.effects).join(", ")));
        }
        if !self.build {
            out.push("no builds".into());
        }
        if !self.private {
            out.push("no private addresses".into());
        }
        // Its limits, so a request held to one is seen to be (FEEDBACK55.md, Q-849; branch
        // feedback55).
        if let Some(w) = self.workers {
            out.push(format!("workers {}", w));
        }
        if let Some(c) = self.max_compute {
            out.push(format!("max_compute {}ms", c.as_millis()));
        }
        if let Some(b) = self.max_body {
            out.push(format!("max_body {} byte{}", b, if b == 1 { "" } else { "s" }));
        }
        if let Some(n) = self.max_tool_calls {
            out.push(format!("max_tool_calls {} a minute", n));
        }
        format!("policy {}{}", self.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(" + "), if out.is_empty() { String::new() } else { format!(" ({})", out.join("; ")) })
    }

    /// Where effects were taken away, for a message.
    pub fn effects_source(&self) -> String {
        self.where_(&self.effects_from)
    }
}

/// `*` any host, `*.example.com` its subdomains (not itself), else the host itself.
fn host_matches(pattern: &str, host: &str) -> bool {
    let p = pattern.trim().to_ascii_lowercase();
    match p.strip_prefix("*.") {
        _ if p == "*" => true,
        Some(base) => host.ends_with(&format!(".{}", base)),
        None => p == host,
    }
}

/// Loopback, private, link-local (the cloud's metadata at 169.254.169.254), unspecified,
/// carrier-grade NAT, and IPv6's unique-local and link-local, with IPv4 in IPv6 judged as IPv4.
pub fn private_ip(ip: IpAddr) -> bool {
    // What `http`'s `public_only` refuses (NAT64, 6to4 and IPv4-compatible addresses too).
    if !crate::fetch::public_ip(ip) {
        return true;
    }
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || (o[0] == 100 && (64..128).contains(&o[1])) || o[0] == 0
        }
        IpAddr::V6(v) => match v.to_ipv4_mapped() {
            Some(v4) => private_ip(IpAddr::V4(v4)),
            None => {
                let s = v.segments();
                v.is_loopback() || v.is_unspecified() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(layers: &[&str]) -> Result<Policy, String> {
        let mut p = Policy::default();
        for (i, t) in layers.iter().enumerate() {
            p.add(Path::new(&format!("layer{}", i)), t)?;
        }
        Ok(p)
    }

    #[test]
    fn a_layer_only_narrows() {
        let p = policy(&[
            "[effects]\nallow = [\"db\", \"http\", \"fs\"]\n[plugins]\nallow = [\"csv@1\", \"qr\"]\n[http]\nhosts = [\"*\"]\n",
            "[effects]\nallow = [\"db\", \"env\"] # env: not given by the first\n[plugins]\nallow = [\"csv@1.4\", \"pdf\"]\nbuild = false\n[http]\nhosts = [\"api.stripe.com\", \"*.mapbox.com\"]\n",
        ])
        .unwrap();
        assert_eq!(p.effects, crate::ast::effect_bit("db"));
        assert_eq!(p.effects_source(), "layer0");
        assert!(!p.build);
        assert!(p.plugin("csv", Some("1.4.2")).is_ok());
        assert!(p.plugin("csv", Some("1.5.0")).unwrap_err().contains("layer1"));
        assert!(p.plugin("csv", Some("10.4")).is_err(), "1 is not 10");
        assert!(p.plugin("pdf", None).unwrap_err().contains("not in the plugins layer0 allows: csv@1, qr"));
        assert!(p.plugin("qr", None).unwrap_err().contains("layer1"));
        assert!(p.http("https://api.stripe.com/v1").is_ok());
        assert!(p.http("https://tiles.mapbox.com/x").is_ok());
        assert!(p.http("https://mapbox.com/x").unwrap_err().contains("not among the hosts layer1 allows"));
        assert!(p.http("not a url").is_ok());
    }

    #[test]
    fn private_addresses_and_names() {
        let p = policy(&["[http]\nprivate = false\n[env]\nnames = [\"A\", \"B\"]\n", "[env]\nnames = [\"B\"]\n"]).unwrap();
        for u in ["http://127.0.0.1:8080/", "http://169.254.169.254/latest/meta-data", "http://10.0.0.5/", "http://[::1]/", "http://[::ffff:192.168.1.1]/", "http://localhost/", "http://100.64.0.1/", "http://[fd00::1]/", "http://0.0.0.0/"] {
            assert!(p.http(u).unwrap_err().contains("an address of this machine or its network"), "{}", u);
        }
        assert!(p.http("http://93.184.216.34/").is_ok());
        assert!(p.env_name("B").is_ok());
        assert!(p.env_name("A").unwrap_err().contains("layer1"));
        assert!(p.env_name("PATH").unwrap_err().contains("layer0"));
        assert!(Policy::default().env_name("PATH").is_ok());
    }

    #[test]
    fn limits_take_the_least() {
        let p = policy(&["[limits]\nworkers = 8\nmax_compute = \"10s\"\nmax_body = \"10MB\"\n", "[limits]\nworkers = 16\nmax_compute = \"500ms\"\nmax_body = 1_000\n"]).unwrap();
        assert_eq!((p.workers, p.max_compute, p.max_body), (Some(8), Some(Duration::from_millis(500)), Some(1000)));
        assert_eq!(bytes(&V::Str("512kb".into())), Some(512 << 10));
        assert_eq!(bytes(&V::Str("2GB".into())), Some(2 << 30));
        assert_eq!(duration("2m"), Some(Duration::from_secs(120)));
        assert_eq!(duration("5"), Some(Duration::from_secs(5)));
        assert_eq!(duration("5h"), None);
        assert_eq!(size("50MB"), Some(50 << 20));
        assert_eq!(size("50mb"), Some(50 << 20));
        assert_eq!(size(" 2 GB "), Some(2 << 30));
        assert_eq!(size("512KB"), Some(512 << 10));
        assert_eq!(size("100B"), Some(100));
        assert_eq!(size("52428800"), Some(52428800));
        assert_eq!(size("1.5GB"), None);
        assert_eq!(size("lots"), None);
        assert_eq!(size(&format!("{}GB", usize::MAX)), None, "too large is no size, not a wrapped one");
    }

    #[test]
    fn what_a_policy_does_not_say_is_refused() {
        assert!(policy(&["[effects]\nallow = [\"disk\"]\n"]).unwrap_err().contains("layer0:2: `disk` is not an effect"));
        assert!(policy(&["[plugins]\nsigned = true\n"]).unwrap_err().contains("`plugins.signed` is not something a policy says"));
        assert!(policy(&["[plugins]\nbuild = \"no\"\n"]).unwrap_err().contains("wrong kind of value"));
        assert!(policy(&["[plugins]\nallow = \"csv\"\n"]).unwrap_err().contains("is a list"));
        assert!(policy(&["just words\n"]).unwrap_err().contains("expected `key = value`"));
        assert!(policy(&["[env]\nnames = [\"A\",\n"]).unwrap_err().contains("a list with no `]`"));
        assert!(policy(&["[limits]\nmax_compute = \"soon\"\n"]).unwrap_err().contains("not a time"));
        // A list over lines, comments, and a `#` inside text.
        let p = policy(&["# the host's\n[env]\nnames = [\n  \"A#1\", # first\n  \"B\",\n]\n"]).unwrap();
        assert_eq!(p.env[0].1, ["A#1", "B"]);
        assert!(policy(&["[limits]\nworkers = 0\n"]).is_err());
    }
}
