//! An installable web app from `app.toml` (FEEDBACK67.md, B.1; branch pwa): the file beside the
//! program's main file that the phone builds already read (mobile/app-config.sh). With one, Quillo
//! serves `/manifest.webmanifest`, the icons made from its `icon`, and `/apple-touch-icon.png`, and
//! adds three tags to the head of each page. Without one, nothing here runs: what a request pays
//! is one check of `Site::pwa`, set at load.
//!
//! ```text
//! name = "Corner Shop"          # shared with the phone builds: 1 to 30 characters
//! icon = "icon.png"             # shared: PNG, JPEG or WebP, 512 × 512 or more (default: Quillo's)
//! short_name = "Shop"           # default: the name cut at a word, at most 12 characters
//! theme_color = "#2e7d32"       # the title bar; also <meta name="theme-color">
//! background_color = "#ffffff"  # the splash screen, and behind the icons that cannot be clear
//! display = "standalone"        # the default; or fullscreen, minimal-ui, browser
//! start_url = "/"               # default: the program's base ("/", or --base's)
//! description = "Groceries in ten minutes."
//! ```
//!
//! Its phone keys (`id`, `version`, `build`, `main`, `site`, `env.NAME`) are the phone builds'.
//! `id` is the phone's bundle id, never the manifest's, whose `id` is the start URL.

use crate::interp::Interp;
use crate::server::{HttpRequest, HttpResponse};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The file's name, in the program's folder.
pub const FILE: &str = "app.toml";

/// Quillo's icon, the phone builds' default too (mobile/assets/icon.png, 1024 × 1024).
static QUILLO_ICON: &[u8] = include_bytes!("../mobile/assets/icon.png");

/// One `key = value` line of app.toml.
pub struct Entry {
    /// Dotted keys joined: `env.API_URL`.
    pub key: String,
    /// A string's text, escapes read; a bare value (a number, true, a word) as written.
    pub value: String,
    pub line: u32,
}

/// What could not be read, at its line: an error, or a warning where the phone builds read the
/// file all the same (a `[table]` line, a key given twice, a key with no value), so that `quillo
/// check` and `quillo run` refuse no file the phone builds take (FEEDBACK67.md, B.1, review;
/// branch pwa).
#[derive(Debug)]
pub struct Problem {
    pub line: u32,
    pub message: String,
    pub warning: bool,
}

/// app.toml as read: every `key = value` line in order, a key given twice or with no value
/// included, and what could not be read. `get` gives a key's first line, as the phone builds'
/// `app_value` reads it; the phone's `env()` takes an `env.NAME`'s last (`mobile::app_env`).
#[derive(Default)]
pub struct AppToml {
    pub entries: Vec<Entry>,
    pub problems: Vec<Problem>,
}

impl AppToml {
    /// The file in `dir`: none when there is none, the error when it cannot be read.
    pub fn read(dir: &Path) -> Option<Result<AppToml, String>> {
        let path = dir.join(FILE);
        match std::fs::read(&path) {
            Ok(bytes) => Some(match String::from_utf8(bytes) {
                Ok(text) => Ok(AppToml::parse(&text)),
                Err(_) => Err("not UTF-8 text".into()),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(e.to_string())),
        }
    }

    /// The flat form the phone builds read: `key = "value"`, `'value'`, a bare number, boolean or
    /// word, `#` comments, blank lines and dotted keys. Lenient as they are: a string never closed
    /// is what it holds, and said. mobile/app-config.sh's `app_value` reads the phone's keys the
    /// same way.
    pub fn parse(text: &str) -> AppToml {
        let mut out = AppToml::default();
        let mut said = |line: u32, message: String, warning: bool| out.problems.push(Problem { line, message, warning });
        let mut entries: Vec<Entry> = Vec::new();
        for (n, raw) in text.strip_prefix('\u{FEFF}').unwrap_or(text).lines().enumerate() {
            let line = n as u32 + 1;
            let t = raw.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            if t.starts_with('[') {
                said(line, format!("`{}`: app.toml is flat, one `key = value` a line; the keys after it are read as if the line were not there, by the phone builds and Quillo alike (env.API_URL = \"...\" for env)", t), true);
                continue;
            }
            let Some((k, v)) = t.split_once('=') else {
                said(line, format!("`{}` is not `key = value`", t), false);
                continue;
            };
            let Some(key) = key_of(k) else {
                said(line, format!("`{}` is not a key: letters, digits, _ and -, parts joined by dots (env.API_URL)", k.trim()), false);
                continue;
            };
            let (value, problem) = value_of(v.trim_start());
            match problem {
                // As the phone builds read it: empty, so the default.
                Some(NO_VALUE) => said(line, format!("`{}` has no value after `=`: as if it were not given", key), true),
                Some(p) => said(line, format!("`{}`: {}", key, p), false),
                None => {}
            }
            if let Some(first) = entries.iter().find(|e| e.key == key) {
                let which = if key.starts_with("env.") { "the phone's env() takes the last of them".to_string() } else { format!("line {} is the one read, by the phone builds too", first.line) };
                said(line, format!("`{}` is given twice (line {} first): {}", key, first.line, which), true);
            }
            entries.push(Entry { key, value, line });
        }
        out.entries = entries;
        out
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.key == key)
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.get(key).map(|e| e.value.as_str())
    }
}

/// `env.API_URL`, `"quoted".part`: the parts trimmed, quotes taken off, joined by dots.
fn key_of(k: &str) -> Option<String> {
    let mut parts = Vec::new();
    for p in k.trim().split('.') {
        let p = p.trim();
        let quoted = p.len() >= 2 && ((p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\'')));
        if quoted {
            parts.push(&p[1..p.len() - 1]);
        } else if !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
            parts.push(p);
        } else {
            return None;
        }
    }
    Some(parts.join("."))
}

const NO_VALUE: &str = "no value after `=`";

/// A value and what is wrong with it: a basic string with TOML's escapes, a literal one, or a bare
/// value up to its comment.
fn value_of(v: &str) -> (String, Option<&'static str>) {
    let after = |rest: &str| {
        let rest = rest.trim();
        (!rest.is_empty() && !rest.starts_with('#')).then_some("text after the value (a comment starts with #)")
    };
    if let Some(q) = v.strip_prefix('"') {
        let mut s = String::new();
        let mut chars = q.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return (s, after(&q[i + 1..])),
                '\\' => match chars.next() {
                    Some((_, 'n')) => s.push('\n'),
                    Some((_, 't')) => s.push('\t'),
                    Some((_, 'r')) => s.push('\r'),
                    Some((_, 'b')) => s.push('\u{8}'),
                    Some((_, 'f')) => s.push('\u{c}'),
                    Some((j, u @ ('u' | 'U'))) => {
                        let len = if u == 'u' { 4 } else { 8 };
                        match q.get(j + 1..j + 1 + len).and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32) {
                            Some(ch) => {
                                s.push(ch);
                                for _ in 0..len {
                                    chars.next();
                                }
                            }
                            None => s.push(u),
                        }
                    }
                    Some((_, o)) => s.push(o),
                    None => {}
                },
                c => s.push(c),
            }
        }
        return (s, Some("a string never closed (a \" missing at the end of the line)"));
    }
    if let Some(q) = v.strip_prefix('\'') {
        return match q.split_once('\'') {
            Some((s, rest)) => (s.to_string(), after(rest)),
            None => (q.to_string(), Some("a string never closed (a ' missing at the end of the line)")),
        };
    }
    let bare = v.split('#').next().unwrap_or("").trim();
    (bare.to_string(), bare.is_empty().then_some(NO_VALUE))
}

/// The name cut for a home screen: whole words of it, at most 12 characters; the name whole when
/// it fits, or when its first word is longer (never cut mid-word: the catalog's plugin made "Quillo
/// Corne" of "Quillo Corner Shop", FEEDBACK67.md's smaller notes).
pub fn short_name(name: &str) -> String {
    let name = name.trim();
    if name.chars().count() <= 12 {
        return name.to_string();
    }
    // A space that is the 13th character or before: what comes before it is 12 or fewer.
    let cut = name.char_indices().take(13).filter(|(_, c)| c.is_whitespace()).map(|(at, _)| at).last();
    cut.map(|at| name[..at].trim_end()).filter(|s| !s.is_empty()).unwrap_or(name).to_string()
}

const DISPLAYS: [&str; 4] = ["standalone", "fullscreen", "minimal-ui", "browser"];

/// CSS's named colours, each with its value.
const NAMED: &str = "aliceblue f0f8ff antiquewhite faebd7 aqua 00ffff aquamarine 7fffd4 azure f0ffff beige f5f5dc bisque ffe4c4 \
black 000000 blanchedalmond ffebcd blue 0000ff blueviolet 8a2be2 brown a52a2a burlywood deb887 cadetblue 5f9ea0 chartreuse 7fff00 \
chocolate d2691e coral ff7f50 cornflowerblue 6495ed cornsilk fff8dc crimson dc143c cyan 00ffff darkblue 00008b darkcyan 008b8b \
darkgoldenrod b8860b darkgray a9a9a9 darkgreen 006400 darkgrey a9a9a9 darkkhaki bdb76b darkmagenta 8b008b darkolivegreen 556b2f \
darkorange ff8c00 darkorchid 9932cc darkred 8b0000 darksalmon e9967a darkseagreen 8fbc8f darkslateblue 483d8b darkslategray 2f4f4f \
darkslategrey 2f4f4f darkturquoise 00ced1 darkviolet 9400d3 deeppink ff1493 deepskyblue 00bfff dimgray 696969 dimgrey 696969 \
dodgerblue 1e90ff firebrick b22222 floralwhite fffaf0 forestgreen 228b22 fuchsia ff00ff gainsboro dcdcdc ghostwhite f8f8ff \
gold ffd700 goldenrod daa520 gray 808080 green 008000 greenyellow adff2f grey 808080 honeydew f0fff0 hotpink ff69b4 \
indianred cd5c5c indigo 4b0082 ivory fffff0 khaki f0e68c lavender e6e6fa lavenderblush fff0f5 lawngreen 7cfc00 \
lemonchiffon fffacd lightblue add8e6 lightcoral f08080 lightcyan e0ffff lightgoldenrodyellow fafad2 lightgray d3d3d3 \
lightgreen 90ee90 lightgrey d3d3d3 lightpink ffb6c1 lightsalmon ffa07a lightseagreen 20b2aa lightskyblue 87cefa \
lightslategray 778899 lightslategrey 778899 lightsteelblue b0c4de lightyellow ffffe0 lime 00ff00 limegreen 32cd32 linen faf0e6 \
magenta ff00ff maroon 800000 mediumaquamarine 66cdaa mediumblue 0000cd mediumorchid ba55d3 mediumpurple 9370db \
mediumseagreen 3cb371 mediumslateblue 7b68ee mediumspringgreen 00fa9a mediumturquoise 48d1cc mediumvioletred c71585 \
midnightblue 191970 mintcream f5fffa mistyrose ffe4e1 moccasin ffe4b5 navajowhite ffdead navy 000080 oldlace fdf5e6 \
olive 808000 olivedrab 6b8e23 orange ffa500 orangered ff4500 orchid da70d6 palegoldenrod eee8aa palegreen 98fb98 \
paleturquoise afeeee palevioletred db7093 papayawhip ffefd5 peachpuff ffdab9 peru cd853f pink ffc0cb plum dda0dd \
powderblue b0e0e6 purple 800080 rebeccapurple 663399 red ff0000 rosybrown bc8f8f royalblue 4169e1 saddlebrown 8b4513 \
salmon fa8072 sandybrown f4a460 seagreen 2e8b57 seashell fff5ee sienna a0522d silver c0c0c0 skyblue 87ceeb slateblue 6a5acd \
slategray 708090 slategrey 708090 snow fffafa springgreen 00ff7f steelblue 4682b4 tan d2b48c teal 008080 thistle d8bfd8 \
tomato ff6347 turquoise 40e0d0 violet ee82ee wheat f5deb3 white ffffff whitesmoke f5f5f5 yellow ffff00 yellowgreen 9acd32";

/// A colour as RGB, laid on white when it is partly clear: `#rgb`, `#rgba`, `#rrggbb`,
/// `#rrggbbaa`, or a CSS name in any case. None when it is none of these.
pub fn colour(c: &str) -> Option<[u8; 3]> {
    let c = c.trim();
    let hex = match c.strip_prefix('#') {
        Some(h) if h.bytes().all(|b| b.is_ascii_hexdigit()) => match h.len() {
            3 | 4 => h.chars().flat_map(|x| [x, x]).collect::<String>(),
            6 | 8 => h.to_string(),
            _ => return None,
        },
        Some(_) => return None,
        None => {
            let name = c.to_ascii_lowercase();
            let mut words = NAMED.split_ascii_whitespace();
            let found = std::iter::from_fn(|| Some((words.next()?, words.next()?))).find(|(n, _)| *n == name).map(|(_, v)| v.to_string());
            match found {
                Some(v) => v,
                None if name == "transparent" => "ffffff00".into(),
                None => return None,
            }
        }
    };
    let b = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    let a = if hex.len() == 8 { b(6) as u32 } else { 255 };
    let on_white = |x: u8| ((x as u32 * a + 255 * (255 - a)) / 255) as u8;
    Some([on_white(b(0)), on_white(b(2)), on_white(b(4))])
}

/// The icons Quillo makes: the manifest's three, and Apple's.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// As it is, transparency kept.
    Any(u32),
    /// Scaled into the safe zone (80%), centred on the background, so that Android's shapes do not
    /// cut it.
    Maskable(u32),
    /// The whole square on the background: iOS shows no transparency.
    Apple(u32),
}

/// The bytes the icons are made from: Quillo's, or the file `icon` names.
#[derive(Clone)]
enum Source {
    Quillo,
    File(Arc<[u8]>),
}

impl Source {
    fn bytes(&self) -> &[u8] {
        match self {
            Source::Quillo => QUILLO_ICON,
            Source::File(b) => b,
        }
    }
}

/// What a program with an app.toml serves, worked out at load.
pub struct Web {
    /// The tags each page's head gets; empty when a route of the program's is at the manifest's
    /// path.
    head: String,
    manifest: Vec<u8>,
    manifest_etag: String,
    /// Whether Quillo answers `/manifest.webmanifest` and `/apple-touch-icon.png`: not where a
    /// route of the program's is at that very path. One that takes any path (`/*rest`, `/:page`)
    /// answers first, and Quillo only where it answers 404 (FEEDBACK67.md, B.1, review; branch
    /// pwa).
    serve_manifest: bool,
    serve_apple: bool,
    source: Source,
    /// The file `icon` names, watched under --watch (there or not).
    icon_file: Option<std::path::PathBuf>,
    background: [u8; 3],
    /// What the icons are made of (the image's bytes and the background): the four are made
    /// together, once.
    set: String,
    /// Each icon's address (`/_quillo/i/<hash>.png`) and kind, in `KINDS`' order.
    icons: Vec<(String, Kind)>,
}

/// The icons Quillo makes, in this order: the manifest's three, then Apple's.
const KINDS: [Kind; 4] = [Kind::Any(192), Kind::Any(512), Kind::Maskable(512), Kind::Apple(180)];

/// The icon sets made, or being made, by what they are made of, for every instance in the
/// process: the first request for one makes all four from one decode, and the requests that come
/// meanwhile wait for it. Sixteen at once, each decoding a 4000 × 4000 PNG, took 1.8 GB that was
/// never given back, and the addresses are public (FEEDBACK67.md, B.1, review; branch pwa). The
/// oldest sets are dropped past a few (a --watch session that changes its icon).
type Set = Arc<std::sync::OnceLock<Vec<Arc<[u8]>>>>;
static SETS: Mutex<Option<indexmap::IndexMap<String, Set>>> = Mutex::new(None);
const KEPT: usize = 4;

/// Said once a process: a line `--instances` would print for each instance.
fn say_once(line: String) {
    static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !said.contains(&line) {
        eprintln!("{}", line);
        said.push(line);
    }
}

fn hash_of(parts: impl std::hash::Hash) -> String {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

/// Reads `dir`'s app.toml, when there is one, into what `it` serves (see `Web`), and works out
/// its service worker when it has routes tagged `offline` (see `Offline`). Called once a program
/// has loaded, by `quillo run` and by each reload, never for a mounted program, a service or the
/// phone shells. Nothing is set without the file and the tag.
pub fn attach(it: &mut Interp, dir: &Path) {
    let name = installable(it, dir);
    // Before the worker, which shows notifications for a program that sends them (FEEDBACK67.md,
    // B.3; branch pwa).
    let notification = crate::push::used_by(it);
    if it.site.notification != notification {
        Arc::make_mut(&mut it.site).notification = notification;
    }
    if notification {
        crate::push::warn_without_secret();
    }
    attach_offline(it, name.as_deref());
}

/// The app.toml part of `attach`: the app's name, when the file gives one.
fn installable(it: &mut Interp, dir: &Path) -> Option<String> {
    let app = match AppToml::read(dir) {
        None => return None,
        Some(Ok(app)) => app,
        Some(Err(e)) => {
            say_once(format!("quillo: {}: {}", dir.join(FILE).display(), e));
            return None;
        }
    };
    let base = it.base.clone();
    let path = |p: &str| format!("{}{}", base, p);
    // Only a route at the very path wins outright: one that takes any path (a last `route ANY
    // /*rest` for a 404 page, `files("site", path)`) is tried first, and Quillo answers where it
    // gives a 404 (FEEDBACK67.md, B.1, review; branch pwa).
    let (manifest_route, apple_route) = (crate::server::route_at(it, "/manifest.webmanifest"), crate::server::route_at(it, "/apple-touch-icon.png"));
    let text = |k: &str| app.text(k).map(str::trim).filter(|s| !s.is_empty());
    // The name and icon as the phone builds take them: as written, an empty one the default.
    let name = app.text("name").filter(|s| !s.is_empty()).unwrap_or("Quillo").to_string();
    let short = text("short_name").map_or_else(|| short_name(&name), str::to_string);
    let theme = text("theme_color").filter(|c| colour(c).is_some());
    let background_given = text("background_color").filter(|c| colour(c).is_some());
    let background = background_given.and_then(colour).unwrap_or([255, 255, 255]);
    let display = text("display").filter(|d| DISPLAYS.contains(d)).unwrap_or("standalone");
    let start = match text("start_url") {
        Some(s) if s.starts_with('/') && !s.starts_with("//") => path(s),
        _ => path("/"),
    };
    let icon = app.text("icon").filter(|s| !s.is_empty());
    let icon_file = icon.map(|i| dir.join(i));
    let source = match icon {
        None => Source::Quillo,
        Some(icon) => {
            let file = dir.join(icon);
            match std::fs::read(&file) {
                Ok(b) if reader(&b).is_ok() => Source::File(Arc::from(b)),
                Ok(_) => {
                    say_once(format!("quillo: {}: icon {} is not a PNG, JPEG or WebP image: Quillo's icon is used", FILE, file.display()));
                    Source::Quillo
                }
                Err(e) => {
                    say_once(format!("quillo: {}: icon {}: {}: Quillo's icon is used", FILE, file.display(), e));
                    Source::Quillo
                }
            }
        }
    };
    // Named by what they are made of: the image's bytes, the kind and size, and the background.
    let made_of = hash_of(source.bytes());
    let icons: Vec<(String, Kind)> = KINDS.into_iter().map(|k| (hash_of((1u8, &made_of, format!("{:?}", k), background)), k)).collect();
    let set = hash_of((&made_of, background));
    let mut m = serde_json::Map::new();
    m.insert("name".into(), name.clone().into());
    m.insert("short_name".into(), short.into());
    if let Some(d) = text("description") {
        m.insert("description".into(), d.into());
    }
    m.insert("start_url".into(), start.into());
    m.insert("scope".into(), path("/").into());
    // The scope, not the start URL: a start URL changed later would make it another app, installed
    // twice (FEEDBACK67.md, B.1, review; branch pwa).
    m.insert("id".into(), path("/").into());
    m.insert("display".into(), display.into());
    if let Some(t) = theme {
        m.insert("theme_color".into(), t.into());
    }
    if let Some(b) = background_given {
        m.insert("background_color".into(), b.into());
    }
    let list: Vec<serde_json::Value> = icons
        .iter()
        .filter_map(|(h, k)| {
            let (size, purpose) = match k {
                Kind::Any(n) => (n, "any"),
                Kind::Maskable(n) => (n, "maskable"),
                Kind::Apple(_) => return None,
            };
            Some(serde_json::json!({ "src": path(&format!("/_quillo/i/{}.png", h)), "sizes": format!("{}x{}", size, size), "type": "image/png", "purpose": purpose }))
        })
        .collect();
    m.insert("icons".into(), list.into());
    let manifest = serde_json::to_vec(&serde_json::Value::Object(m)).unwrap_or_default();
    let manifest_etag = format!("\"{}\"", hash_of(&manifest));
    let mut head = String::new();
    if !manifest_route {
        head.push_str(&format!("<link rel=\"manifest\" href=\"{}\">", attr(&path("/manifest.webmanifest"))));
        if let Some(t) = theme {
            head.push_str(&format!("<meta name=\"theme-color\" content=\"{}\">", attr(t)));
        }
        if !apple_route {
            head.push_str(&format!("<link rel=\"apple-touch-icon\" href=\"{}\">", attr(&path("/apple-touch-icon.png"))));
        }
    }
    let web = Web { head, manifest, manifest_etag, serve_manifest: !manifest_route, serve_apple: !apple_route, source, icon_file, background, set, icons };
    Arc::make_mut(&mut it.site).pwa = Some(Arc::new(web));
    app.text("name").filter(|s| !s.is_empty()).map(str::to_string)
}

impl Web {
    /// The file `icon` names, there or not.
    pub fn icon_file(&self) -> Option<&Path> {
        self.icon_file.as_deref()
    }
}

/// `GET /manifest.webmanifest` and `GET /apple-touch-icon.png`, where no route of the program's is
/// at that very path: revalidated each time (no-cache and an ETag, as Quillo's own files are). None for
/// anything else, which the routes answer. Cold, as everything a request may call here: kept out
/// of the request's own code, which a program without app.toml runs alone.
#[cold]
pub fn answer(it: &mut Interp, path: &str, req: &HttpRequest) -> Option<HttpResponse> {
    if !matches!(path, "/manifest.webmanifest" | "/apple-touch-icon.png") || !matches!(req.method.as_str(), "GET" | "HEAD") {
        return None;
    }
    let w = it.site.pwa.clone()?;
    if path == "/manifest.webmanifest" && w.serve_manifest {
        return Some(HttpResponse {
            status: 200,
            headers: vec![("content-type".into(), "application/manifest+json".into()), ("cache-control".into(), "no-cache".into()), ("etag".into(), w.manifest_etag.clone())],
            body: w.manifest.clone(),
        });
    }
    if path == "/apple-touch-icon.png" && w.serve_apple {
        let (hash, kind) = w.icons.last()?.clone();
        let png = made(it, &w, kind);
        return Some(HttpResponse {
            status: 200,
            headers: vec![("content-type".into(), "image/png".into()), ("cache-control".into(), "no-cache".into()), ("etag".into(), format!("\"{}\"", hash))],
            body: png.to_vec(),
        });
    }
    None
}

/// What an app adds once a route answered: at the manifest's or Apple icon's path, a route that
/// takes any path keeps the request only with a real answer, a 2xx that is not HTML (a real
/// public/manifest.webmanifest that `files()` serves) or a 3xx; anything else is Quillo's. A last
/// `route ANY /*rest` for a 404 page, a front end's `route GET /*rest = file("dist/index.html")`
/// (HTML for every path), a guarded group's 401 to the browser's cookieless fetch and a 500 would
/// each have stopped the install. A route at the very path keeps it whatever it answers (`answer`
/// gives none then). And the tags in the head of a page a route made, a HEAD's too, whose length
/// is the GET's (FEEDBACK31.md, Q-393's rule). Out of the request's own code: a program without
/// app.toml never calls it (FEEDBACK67.md, B.1 and its review; branch pwa).
#[cold]
#[inline(never)]
pub fn after_route(it: &mut Interp, req: &HttpRequest, path: &str, resp: &mut HttpResponse) {
    let real = matches!(resp.status, 200..=299) && !crate::server::is_html(resp) || matches!(resp.status, 300..=399);
    if !real {
        if let Some(own) = answer(it, path, req) {
            *resp = own;
            return;
        }
    }
    // A page a route made, not a file as it is on disk (`as_written`).
    if crate::server::is_html(resp) && !crate::server::as_written(resp) {
        if let Some(w) = it.site.pwa.clone() {
            add_tags(&mut resp.body, &w);
        }
    }
}

/// `/_quillo/i/<hash>.png`: an icon the manifest names, kept for a year (its address changes with
/// what it is made of); one this program does not name is not there.
#[cold]
pub fn icon_answer(it: &mut Interp, hash: &str) -> HttpResponse {
    let Some((w, kind)) = it.site.pwa.clone().and_then(|w| w.icons.iter().find(|(h, _)| h == hash).map(|(_, k)| *k).map(|k| (w, k))) else {
        return crate::server::plain(404, "not found\n");
    };
    let png = made(it, &w, kind);
    HttpResponse {
        status: 200,
        headers: vec![("content-type".into(), "image/png".into()), ("cache-control".into(), "public, max-age=31536000, immutable".into())],
        body: png.to_vec(),
    }
}

/// The icon of `kind`, made with the other three on the first request for any of them, off the
/// event loop: about 40 ms for the four from Quillo's 1024 × 1024 (measured), which made at load
/// every start, every instance and every reload under --watch would wait for, and which most
/// visits never ask for. One decode at a time for a set: a request that comes while it is made
/// waits for it (see `SETS`). An image that cannot be made into icons gives Quillo's.
fn made(it: &mut Interp, w: &Web, kind: Kind) -> Arc<[u8]> {
    let i = KINDS.iter().position(|k| *k == kind).unwrap_or(0);
    let set: Set = {
        let mut sets = SETS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let sets = sets.get_or_insert_with(Default::default);
        match sets.get(&w.set) {
            Some(s) => s.clone(),
            None => {
                let s = Set::default();
                sets.insert(w.set.clone(), s.clone());
                while sets.len() > KEPT {
                    sets.shift_remove_index(0);
                }
                s
            }
        }
    };
    if let Some(all) = set.get() {
        return all[i].clone();
    }
    let (source, background) = (w.source.clone(), w.background);
    it.unlocked(move || {
        set.get_or_init(|| {
            make_all(source.bytes(), background).unwrap_or_else(|e| {
                say_once(format!("quillo: {}: icon: {}: Quillo's icon is used", FILE, e));
                make_all(QUILLO_ICON, background).unwrap_or_else(|_| vec![Arc::from(Vec::new()); KINDS.len()])
            })
        })[i]
            .clone()
    })
}

/// A reader of what `bytes` hold, by their first bytes: PNG, JPEG or WebP, none other.
fn reader(bytes: &[u8]) -> Result<image::ImageReader<std::io::Cursor<&[u8]>>, String> {
    let r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    match r.format() {
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP) => Ok(r),
        _ => Err("not a PNG, JPEG or WebP image".into()),
    }
}

/// The image decoded, within the bounds `image.convert` keeps, and turned as its EXIF says.
fn decode(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    use image::ImageDecoder;
    let mut r = reader(bytes)?;
    r.limits(crate::imaging::limits());
    let mut d = r.into_decoder().map_err(|e| e.to_string())?;
    let o = d.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(d).map_err(|e| e.to_string())?;
    img.apply_orientation(o);
    Ok(img)
}

/// The four icons, as PNG, in `KINDS`' order, from one decode of the image: made square (centred,
/// the rest clear), then sized as each kind says.
fn make_all(bytes: &[u8], background: [u8; 3]) -> Result<Vec<Arc<[u8]>>, String> {
    use image::imageops::{overlay, resize, FilterType};
    use image::{Rgba, RgbaImage};
    let img = decode(bytes)?.into_rgba8();
    let (w, h) = img.dimensions();
    let side = w.max(h);
    let square = if w == h {
        img
    } else {
        let mut c = RgbaImage::from_pixel(side, side, Rgba([0, 0, 0, 0]));
        overlay(&mut c, &img, ((side - w) / 2) as i64, ((side - h) / 2) as i64);
        c
    };
    // At most the largest icon's size before the four are made from it: one pass over a large
    // image, not four, and its pixels let go of.
    let square = if side > 512 { resize(&square, 512, 512, FilterType::Lanczos3) } else { square };
    // Laid on the background, which is opaque: no alpha channel is left.
    let on = |size: u32, inner: u32| -> image::DynamicImage {
        let mut c = RgbaImage::from_pixel(size, size, Rgba([background[0], background[1], background[2], 255]));
        let off = ((size - inner) / 2) as i64;
        overlay(&mut c, &resize(&square, inner, inner, FilterType::Lanczos3), off, off);
        image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(c).to_rgb8())
    };
    let one = |kind: Kind| -> Result<Arc<[u8]>, String> {
        let out = match kind {
            Kind::Any(n) => {
                let r = resize(&square, n, n, FilterType::Lanczos3);
                // No alpha channel where none is clear: a smaller file.
                if r.pixels().all(|p| p.0[3] == 255) {
                    image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(r).to_rgb8())
                } else {
                    image::DynamicImage::ImageRgba8(r)
                }
            }
            Kind::Maskable(n) => on(n, n * 4 / 5),
            Kind::Apple(n) => on(n, n),
        };
        // The PNG encoder alone: `write_to` brings in every format's (100 KB more in the binary, measured).
        let mut png = Vec::new();
        out.write_with_encoder(image::codecs::png::PngEncoder::new(&mut png)).map_err(|e| e.to_string())?;
        Ok(Arc::from(png))
    };
    KINDS.into_iter().map(one).collect()
}

/// The app's tags put in a page's head: before `</head>`, after `<head>` when it is not closed, or
/// after `<html>` where the page has no head, which the browser then starts with them. None on a
/// page that has no `<html>` or `<head>` (a part of a page), nor on one that links a manifest of
/// its own; a theme colour or an Apple icon the page has is kept, ours left out.
#[cold]
pub fn add_tags(body: &mut Vec<u8>, w: &Web) {
    if w.head.is_empty() {
        return;
    }
    let find = |needle: &[u8]| tag_at(body, needle);
    let at = match find(b"</head") {
        Some(i) => Some(i),
        None => find(b"<head").or_else(|| find(b"<html")).and_then(|i| body[i..].iter().position(|&c| c == b'>').map(|e| i + e + 1)),
    };
    let Some(at) = at else { return };
    // What the head has of its own: up to the body, or all of a page that has none.
    let head_end = tag_at(body, b"<body").unwrap_or(body.len()).max(at.min(body.len()));
    let (manifest, theme, apple) = own_tags(&body[..head_end]);
    if manifest {
        return;
    }
    let mut tags = std::borrow::Cow::Borrowed(w.head.as_str());
    if theme || apple {
        let mut t = String::with_capacity(w.head.len());
        for tag in w.head.split_inclusive('>') {
            let skip = (theme && tag.starts_with("<meta name=\"theme-color\"")) || (apple && tag.starts_with("<link rel=\"apple-touch-icon\""));
            if !skip {
                t.push_str(tag);
            }
        }
        tags = std::borrow::Cow::Owned(t);
    }
    let len = body.len();
    body.splice(at.min(len)..at.min(len), tags.bytes());
}

/// Where a tag named `needle` (`<head`, `</head`) starts, any case, the name whole: `<header>` is
/// no `<head>`.
fn tag_at(body: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, _) = needle.split_first()?;
    memchr::memchr_iter(first, body).find(|&i| {
        body.len() - i > needle.len() && body[i..i + needle.len()].eq_ignore_ascii_case(needle) && matches!(body[i + needle.len()], b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
    })
}

/// Whether `head` links a manifest, has a theme colour, links an Apple icon, by its `<link>` and
/// `<meta>` elements.
fn own_tags(head: &[u8]) -> (bool, bool, bool) {
    let (mut manifest, mut theme, mut apple) = (false, false, false);
    for i in memchr::memchr_iter(b'<', head) {
        let rest = &head[i + 1..];
        let is = |name: &[u8]| rest.len() > name.len() && rest[..name.len()].eq_ignore_ascii_case(name) && rest[name.len()].is_ascii_whitespace();
        let (link, meta) = (is(b"link"), is(b"meta"));
        if !link && !meta {
            continue;
        }
        let end = memchr::memchr(b'>', rest).unwrap_or(rest.len());
        let tag = String::from_utf8_lossy(&rest[..end]).to_ascii_lowercase();
        if link {
            let rel = attr_value(&tag, "rel").unwrap_or_default();
            manifest |= rel.split_ascii_whitespace().any(|r| r == "manifest");
            apple |= rel.split_ascii_whitespace().any(|r| r == "apple-touch-icon" || r == "apple-touch-icon-precomposed");
        } else {
            theme |= attr_value(&tag, "name").is_some_and(|n| n.trim() == "theme-color");
        }
    }
    (manifest, theme, apple)
}

/// An attribute's value in a tag's text (lower case), quoted or not.
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = tag[from..].find(name).map(|i| from + i) {
        from = i + name.len();
        let before = tag[..i].chars().last();
        if !before.is_some_and(|c| c.is_ascii_whitespace()) {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(v) = rest.strip_prefix('=') else { continue };
        let v = v.trim_start();
        return Some(match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or("").to_string(),
            _ => v.split(|c: char| c.is_ascii_whitespace() || c == '/').next().unwrap_or("").to_string(),
        });
    }
    None
}

/// The keys Quillo reads, then the phone builds' (and their `env.NAME`).
const KNOWN: [&str; 13] = ["name", "icon", "short_name", "theme_color", "background_color", "display", "start_url", "description", "id", "version", "build", "main", "site"];

/// The known key a mistyped one is likely to be: the same once `-` is `_`, `colour` is `color` and
/// case is set aside, or one or two letters off.
fn meant(key: &str) -> Option<&'static str> {
    let norm = |k: &str| k.to_ascii_lowercase().replace('-', "_").replace("colour", "color");
    let key = norm(key);
    let far = |a: &str, b: &str| {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.iter().enumerate() {
            let mut prev = row[0];
            row[0] = i + 1;
            for (j, cb) in b.iter().enumerate() {
                let here = row[j + 1];
                row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(here + 1);
                prev = here;
            }
        }
        row[b.len()]
    };
    KNOWN.iter().copied().map(|k| (far(&key, k), k)).filter(|(d, k)| *d == 0 || (*d <= 2 && k.len() > 3)).min_by_key(|(d, _)| *d).map(|(_, k)| k)
}

/// An icon's size, decoded whole within the bounds the run decodes it in, so that a file cut short
/// or one too large to decode is found here and not by the first visitor: by file, length and
/// time, as an editor's checker asks again at each keystroke.
fn icon_size(file: &Path, bytes: &[u8]) -> Result<(u32, u32), String> {
    type Key = (std::path::PathBuf, usize, Option<std::time::SystemTime>);
    static SEEN: Mutex<Option<std::collections::HashMap<Key, Result<(u32, u32), String>>>> = Mutex::new(None);
    let key = (file.to_path_buf(), bytes.len(), std::fs::metadata(file).and_then(|m| m.modified()).ok());
    if let Some(r) = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|m| m.get(&key).cloned()) {
        return r;
    }
    let r = decode(bytes).map(|img| (img.width(), img.height()));
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let seen = seen.get_or_insert_with(Default::default);
    if seen.len() >= 16 {
        seen.clear();
    }
    seen.insert(key, r.clone());
    r
}

/// What `quillo check` says of the app.toml in `dir`, shown as `shown`: what cannot be read, and
/// the rules of what it says (the phone builds' for `name`; the catalog's `pwa_check` for the
/// rest), each at its line. What the phone builds take is never an error here (FEEDBACK67.md, B.1,
/// review; branch pwa).
pub fn check(dir: &Path, shown: &str) -> Vec<crate::check::Diagnostic> {
    use crate::check::{Diagnostic, Severity};
    let mut out = Vec::new();
    let mut say = |line: u32, severity: Severity, message: String| out.push(Diagnostic { file: shown.to_string(), line, severity, message });
    let app = match AppToml::read(dir) {
        None => return Vec::new(),
        Some(Ok(app)) => app,
        Some(Err(e)) => {
            say(0, Severity::Error, format!("cannot read it: {}", e));
            return out;
        }
    };
    let level = |warning: bool| if warning { Severity::Warning } else { Severity::Error };
    let mut lines: Vec<(u32, Severity, String)> = app.problems.iter().map(|p| (p.line, level(p.warning), p.message.clone())).collect();
    let said_at = |line: u32| app.problems.iter().any(|p| p.line == line);
    if let Some(e) = app.get("name") {
        let n = e.value.chars().count();
        if n > 30 {
            lines.push((e.line, Severity::Error, format!("name must be 1 to 30 characters, as the phone builds take it: this one has {}", n)));
        } else if n == 0 && !said_at(e.line) {
            lines.push((e.line, Severity::Warning, "name is empty: the app is called Quillo, as the phone builds call it".into()));
        }
    }
    if let Some(e) = app.get("icon").filter(|e| !e.value.is_empty()) {
        let file = dir.join(&e.value);
        // Known by what it holds, as the run knows it, not by its name.
        let read = if file.is_file() { Some(std::fs::read(&file)) } else { None };
        match read {
            None => lines.push((e.line, Severity::Error, format!("icon \"{}\": no such file in the program's folder", e.value))),
            Some(Err(why)) => lines.push((e.line, Severity::Error, format!("icon \"{}\" cannot be read: {}", e.value, why))),
            Some(Ok(b)) => match reader(&b).map(|_| icon_size(&file, &b)) {
                Err(not) => lines.push((e.line, Severity::Error, format!("icon \"{}\" is {}", e.value, not))),
                Ok(Err(why)) => lines.push((e.line, Severity::Error, format!("icon \"{}\" cannot be read as an image: {}", e.value, why))),
                Ok(Ok((w, h))) if w.min(h) < 512 => lines.push((e.line, Severity::Warning, format!("icon \"{}\" is {}×{}: Chrome wants 512×512 to offer to install the app, so it is enlarged; give one of 512×512 or more", e.value, w, h))),
                Ok(Ok((w, h))) if w.max(h) > 4096 => lines.push((e.line, Severity::Warning, format!("icon \"{}\" is {}×{}: it is decoded whole, {} MB, to make the icons; 1024×1024 is plenty", e.value, w, h, (w as u64 * h as u64 * 4) >> 20))),
                Ok(Ok(_)) => {}
            },
        }
    }
    if let Some(e) = app.get("display").filter(|e| !e.value.is_empty()) {
        if e.value == "browser" {
            lines.push((e.line, Severity::Warning, "display = \"browser\" opens the app in a tab: browsers do not offer to install it (standalone is the default)".into()));
        } else if !DISPLAYS.contains(&e.value.as_str()) {
            lines.push((e.line, Severity::Error, format!("display is standalone, fullscreen, minimal-ui or browser, not \"{}\"", e.value)));
        }
    }
    for k in ["theme_color", "background_color"] {
        if let Some(e) = app.get(k).filter(|e| !e.value.is_empty() && colour(&e.value).is_none()) {
            lines.push((e.line, Severity::Error, format!("{} \"{}\" is not a CSS colour: #rgb, #rrggbb, #rrggbbaa or a name such as teal", k, e.value)));
        }
    }
    if let Some(e) = app.get("start_url").filter(|e| !e.value.is_empty() && (!e.value.starts_with('/') || e.value.starts_with("//"))) {
        lines.push((e.line, Severity::Error, format!("start_url is a path of the program's, starting with / (like /app), not \"{}\"", e.value)));
    }
    // A key read by no one, likely mistyped: said once, at its first line.
    let mut seen: Vec<&str> = Vec::new();
    for e in &app.entries {
        if KNOWN.contains(&e.key.as_str()) || e.key.strip_prefix("env.").is_some_and(|k| !k.is_empty()) || seen.contains(&e.key.as_str()) {
            continue;
        }
        seen.push(&e.key);
        let message = match meant(&e.key) {
            Some(k) => format!("`{}` is read by no one: `{}`?", e.key, k),
            None => format!("`{}` is read by no one: app.toml takes name, icon, short_name, theme_color, background_color, display, start_url and description, and the phone builds' id, version, build, main, site and env.NAME", e.key),
        };
        lines.push((e.line, Severity::Warning, message));
    }
    lines.sort_by_key(|l| l.0);
    for (line, severity, message) in lines {
        say(line, severity, message);
    }
    out
}

// ---- B.2: Quillo's service worker, for routes tagged `offline` ----------------------------------

/// Quillo's service worker, for a program with routes tagged `offline` (FEEDBACK67.md, B.2;
/// branch pwa): `<base>/_quillo/sw.js`, and the script that registers it on those routes' pages.
/// Worked out at load, never under --watch, in the phone shells or in `quillo app`: none for a
/// program without the tag, which is all a request checks (`Site::offline`).
pub struct Offline {
    /// `/_quillo/sw.js` as served, and its validator.
    worker: Vec<u8>,
    etag: String,
    /// The `<script>` an offline route's page gets, and its text, for a page's content policy.
    pub register: String,
    pub register_js: String,
}

/// The worker (src/client/sw.js); the one that takes itself away (src/client/sw-watch.js), served
/// under --watch and to a program with no offline route; and the gate the worker puts first in a
/// visitor's kept page (src/client/sw-gate.js), whose text is the same everywhere, so that a
/// page's content policy names it by one hash.
static WORKER: &str = include_str!("client/sw.js");
static WATCH_WORKER: &str = include_str!("client/sw-watch.js");
/// The worker of a program that uses `notification` with no offline route, and under --watch: it
/// keeps nothing, and shows notifications with sw.js's part for them (FEEDBACK67.md, B.3; branch
/// pwa).
static PUSH_WORKER: &str = include_str!("client/sw-push.js");

/// sw.js up to its notifications (from its line `// @push`), and those: the whole for a program
/// that uses `notification`, the first part alone for one that does not.
fn worker_parts() -> (&'static str, &'static str) {
    let at = WORKER.find("\n// @push").map_or(WORKER.len(), |i| i + 1);
    (&WORKER[..at], &WORKER[at..])
}

/// Code as served: without its comments, which say what the guide says.
fn uncommented(code: &str) -> String {
    code.lines().filter(|l| !l.trim_start().starts_with("//")).flat_map(|l| [l, "\n"]).collect()
}
pub static GATE: &str = include_str!("client/sw-gate.js");

/// The headers Quillo marks answers with for its worker: an offline route's page (`1`, or
/// `private` for a page of a session's), and the session the browser holds after an answer.
pub const OFFLINE_HEADER: &str = "x-quillo-offline";
pub const SESSION_HEADER: &str = "x-quillo-session";

fn js(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// The gate's text, as it goes in a page and in a policy's hash: the file's code, its comments
/// and end of line off.
pub fn gate() -> &'static str {
    static CODE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CODE.get_or_init(|| GATE.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n").trim().to_string())
}

/// Sets `it.site.offline` when the program has routes tagged `offline` and runs as a server, not
/// under --watch (see `worker_answer` for what is served otherwise). `name`: app.toml's, for the
/// offline page.
fn attach_offline(it: &mut Interp, name: Option<&str>) {
    let offline: Vec<&crate::ast::RouteDef> = it.routes.iter().filter(|r| r.offline).map(|r| &**r).collect();
    if offline.is_empty() || crate::platform::runtime() != "server" || crate::platform::DEV.load(std::sync::atomic::Ordering::Relaxed) {
        if it.site.offline.is_some() {
            Arc::make_mut(&mut it.site).offline = None;
        }
        return;
    }
    let base = it.base.clone();
    // Each offline route with no parameter in its path, kept on install, once each.
    let mut pages: Vec<String> = Vec::new();
    for r in &offline {
        let mut path = String::new();
        for seg in &r.path {
            match seg {
                crate::ast::Seg::Lit(l) => {
                    path.push('/');
                    path.push_str(l);
                }
                _ => {
                    path.clear();
                    break;
                }
            }
        }
        if path.is_empty() && !r.path.is_empty() {
            continue;
        }
        let full = format!("{}{}", base, if path.is_empty() { "/" } else { &path });
        if !pages.contains(&full) {
            pages.push(full);
        }
    }
    let offline_page = offline_page(name);
    // The version: this build, the program (its files' contents, wherever they are, so that
    // servers with one deploy in different folders agree), its base and its offline routes.
    let mut files: Vec<String> = it.loaded.iter().map(|f| hash_of(std::fs::read(f).unwrap_or_default())).collect();
    files.sort();
    let shown: Vec<String> = offline.iter().map(|r| r.shown()).collect();
    // With the part that shows notifications for a program that sends them (FEEDBACK67.md, B.3;
    // branch pwa).
    let source = if it.site.notification { WORKER } else { worker_parts().0 };
    let version = hash_of((crate::version(), source, GATE, &base, &shown, &files, &offline_page));
    let code = uncommented(source);
    let worker = code
        .replacen("@BASE@", &js(&base), 1)
        .replacen("@V@", &js(&version), 1)
        .replacen("@PAGES@", &serde_json::to_string(&pages).unwrap_or_else(|_| "[]".into()), 1)
        .replacen("@OFFLINE@", &js(&offline_page), 1)
        .replacen("@GATE@", &js(gate()), 1)
        .replacen("@GATE_HASH@", &js(&crate::server::source_hash(gate())), 1);
    // 280 bytes with its tags, on each offline route's page: the worker's scope is the program's
    // base. It also tells Quillo's worker, where it controls the page, which session the browser
    // holds (`quillo_tag`, "-" for none), as the runtime does on pages with components: a sign-out
    // the worker did not see (a redirect, another tab) drops that session's pages there
    // (FEEDBACK67.md, B.2, second review; branch pwa).
    let register_js = format!(
        "(n=>{{if(!n)return;const u={};n.register(u,{{scope:{}}});const c=n.controller,m=/(?:^|;\\s*)quillo_tag=([^;]*)/.exec(document.cookie);c&&c.scriptURL===new URL(u,location).href&&c.postMessage({{quillo:\"session\",who:m?m[1]:\"-\"}})}})(navigator.serviceWorker)",
        js(&format!("{}/_quillo/sw.js", base)),
        js(&format!("{}/", base))
    )
    .replace("</", "<\\/");
    let register = format!("<script>{}</script>", register_js);
    Arc::make_mut(&mut it.site).offline = Some(Arc::new(Offline { worker: worker.into_bytes(), etag: format!("\"{}\"", version), register, register_js }));
}

/// The page the worker gives for a page it has not kept, the network gone: the app's name when
/// app.toml gives one, in plain English (the worker cannot know the language of a page it never
/// saw).
fn offline_page(name: Option<&str>) -> String {
    let esc = |s: &str| crate::interp::escape(s);
    let (title, h1) = match name {
        Some(n) => (esc(n), esc(n)),
        None => ("Offline".to_string(), "You are offline".to_string()),
    };
    format!(
        concat!(
            "<!doctype html><html lang=\"en\" data-quillo-offline><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            "<title>{}</title><style>body{{margin:0;min-height:100vh;display:grid;place-items:center;font:16px/1.5 system-ui,sans-serif;text-align:center;color:#222;background:#fff}}",
            "main{{padding:24px;max-width:28em}}h1{{font-size:1.4em;margin:0 0 .4em}}button{{font:inherit;padding:.4em 1.2em}}",
            "@media (prefers-color-scheme:dark){{body{{color:#eee;background:#121212}}}}</style></head>",
            "<body><main><h1>{}</h1><p>There is no connection, and this page was not kept on this device. It opens again once the connection is back.</p>",
            "<button onclick=\"location.reload()\">Try again</button></main></body></html>"
        ),
        title, h1
    )
}

/// `GET <base>/_quillo/sw.js`: Quillo's worker, revalidated each time (browsers check it on
/// navigations). Without one (no offline route, or --watch), the worker that takes itself away
/// and what Quillo's kept: a browser asks for this address only if it registered Quillo's worker
/// in an earlier run, and a program that dropped its last offline route, as one under --watch,
/// would have left it answering from its cache; by the spec a 404 here leaves it as it is
/// (FEEDBACK67.md, B.2 and its review; branch pwa). None in the phone shells and `quillo app`.
#[cold]
#[inline(never)]
pub fn worker_answer(it: &Interp, req: &HttpRequest) -> Option<HttpResponse> {
    if !matches!(req.method.as_str(), "GET" | "HEAD") || crate::platform::runtime() != "server" {
        return None;
    }
    let (body, etag) = match it.site.offline.as_ref() {
        Some(o) => (o.worker.clone(), o.etag.clone()),
        // A program that sends notifications, with no offline route or under --watch: the worker
        // that shows them and keeps nothing (FEEDBACK67.md, B.3; branch pwa).
        None if it.site.notification => {
            let body = (uncommented(PUSH_WORKER).replacen("@BASE@", &js(&it.base), 1) + &uncommented(worker_parts().1)).into_bytes();
            let etag = format!("\"{}\"", hash_of(&body));
            (body, etag)
        }
        None => {
            let body = WATCH_WORKER.replacen("@BASE@", &js(&it.base), 1).into_bytes();
            let etag = format!("\"{}\"", hash_of(&body));
            (body, etag)
        }
    };
    let headers = vec![
        ("content-type".into(), "text/javascript; charset=utf-8".into()),
        ("cache-control".into(), "no-cache".into()),
        ("service-worker-allowed".into(), format!("{}/", it.base)),
        ("etag".into(), etag.clone()),
    ];
    let fresh = req.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("if-none-match") && v.split(',').any(|t| t.trim() == etag));
    Some(HttpResponse { status: if fresh { 304 } else { 200 }, headers, body: if fresh { Vec::new() } else { body } })
}

/// What the worker may do with an answer, once its headers are set (`mark`).
#[derive(Default, Clone, Copy)]
pub struct Marked {
    /// An offline route's page: it gets the register script.
    pub register: bool,
    /// Kept for one session: the gate goes through the page's content policy.
    pub private: bool,
}

/// What a program with offline routes marks its answers with, once their headers are set
/// (FEEDBACK67.md, B.2 and its reviews; branch pwa):
/// - An offline route's 200 page is kept (`x-quillo-offline`), by what the request read: `1`
///   for one that read no one, `private` for one that read who the visitor is through the session
///   alone (kept for that session, by its name), and not at all where it read them another way
///   (a cookie, a header: `visitor_other`), where it has waiting parts (`defer`: made after its
///   headers, or for a prefetch after the request, what they read is not known), nor where it
///   holds a form key, which a kept copy would send again. Not by `cache-control`, which the program may have set or taken away.
/// - The session's name (`x-quillo-session`) goes on a page of the visitor's and on an answer that
///   sets or ends the session, a server fn's too, and on no answer a shared cache may keep; and
///   `-` (no one) on a page whose request came with no session cookie at all, which any cache may
///   give anyone: a signed-in person given it only has their kept pages dropped. `quillo_tag`, the
///   cookie that names it, is set again or taken away from what is known of the session alone
///   (`session::tag_again`).
/// - Marks the program's own answer carries are taken off first: only Quillo says what its worker
///   keeps.
/// Out of the request's own code: a program without the tag never calls it.
#[cold]
#[inline(never)]
pub fn mark(it: &mut Interp, req: &HttpRequest, offline_route: bool, session_changed: bool, parts: bool, resp: &mut HttpResponse) -> Marked {
    unmarked(resp);
    let html = crate::server::is_html(resp);
    // `-` says no one, and drops the visitor's kept pages: only on a GET's answer, and a server
    // fn's (never a navigation, and only the site's own pages can call one), which the runtime
    // tells the worker of. Another site's form sent here carries no `Lax` cookie, and would have
    // wiped them (FEEDBACK67.md, B.2, third review; branch pwa).
    let get = matches!(req.method.as_str(), "GET" | "HEAD") || req.url.starts_with("/_quillo/rpc/");
    let (mut session_cookie, mut had) = (false, None);
    for (k, v) in req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("cookie")) {
        let _ = k;
        for (name, value) in v.split(';').filter_map(crate::server::cookie_pair) {
            session_cookie |= name == crate::session::COOKIE || name == crate::session::HOST_COOKIE;
            if name == crate::session::TAG_COOKIE && had.is_none() {
                had = Some(value.to_string());
            }
        }
    }
    let unshared = |resp: &HttpResponse| {
        resp.sent("cache-control").is_some_and(|c| {
            c.split(',').any(|d| {
                let d = d.trim().to_ascii_lowercase();
                d == "no-store" || d == "private" || d.starts_with("private=")
            })
        })
    };
    // The session's name: never on an answer a shared cache may keep (the program took `private`
    // away), whose page is then not kept either.
    let tag = if session_changed || (html && it.offline_seen.is_some() && unshared(resp)) { crate::session::offline_tag(it) } else { None };
    // A session cookie the page did not read, which no longer opens (expired, or the key changed):
    // no one, as no cookie is; its kept pages and `quillo_tag` go. Only where it matters, and
    // only then is the cookie checked (FEEDBACK67.md, B.2, third review; branch pwa).
    // Only where the browser holds `quillo_tag`: without it the gate shows that session's copies to
    // no one anyway.
    let ended = html && get && session_cookie && had.is_some() && !session_changed && it.offline_seen.is_none() && crate::session::cookie_opens(it, req) == Some(false);
    let said = match &tag {
        Some(t) => Some(t.clone()),
        None if html && (!session_cookie || ended) && !session_changed => Some("-".into()),
        None => None,
    };
    if let Some(t) = said.filter(|t| get || t != "-") {
        resp.headers.push((SESSION_HEADER.into(), t));
    }
    if html && !session_changed {
        if let Some(line) = crate::session::tag_again(it, had.as_deref(), session_cookie && !ended) {
            // Never on an answer a shared cache may keep: it is the visitor's now.
            if !unshared(resp) {
                resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("cache-control"));
                resp.headers.push(("cache-control".into(), "private".into()));
            }
            resp.headers.push(("set-cookie".into(), line));
        }
    }
    if !(offline_route && html && resp.status == 200) {
        return Marked::default();
    }
    let register = Marked { register: true, private: false };
    // `idempotency::holds_form_key`, by memchr's search: compared window by window it was 9,000
    // instructions on a 2 KB page. An answer that set or ended the session is not kept: it may
    // show the person before it (a goodbye) under the name of the session after it (third review).
    if it.visitor_other || parts || session_changed || memchr::memmem::find(&resp.body, crate::idempotency::FORM_FIELD.as_bytes()).is_some() {
        return register;
    }
    // The session read: the page is that session's. A locale rule alone reads nothing of
    // anyone's that matters here.
    if it.offline_seen.is_some() {
        // A session that cannot be named (too full for an id), or an answer a shared cache may
        // keep: not kept.
        if tag.is_none() {
            return register;
        }
        resp.headers.push((OFFLINE_HEADER.into(), "private".into()));
        return Marked { register: true, private: true };
    }
    resp.headers.push((OFFLINE_HEADER.into(), "1".into()));
    register
}

/// Takes away the marks an answer carries for Quillo's worker: the program's own, or a mounted
/// program's, which only Quillo may set (FEEDBACK67.md, B.2, second and third reviews; branch pwa).
pub fn unmarked(resp: &mut HttpResponse) {
    resp.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(OFFLINE_HEADER) && !k.eq_ignore_ascii_case(SESSION_HEADER));
}
