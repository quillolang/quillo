//! Files sent with a form (`multipart/form-data`). Fields go into
//! `request.form` like any form's; each file is a record in `request.files`
//! (`{ field, filename, type, size, start, end }`), its bytes staying in the
//! request's body until `save_upload` writes them where the program says.

use crate::value::{Key, Map, Value};

/// Most parts one request may have.
const MAX_PARTS: usize = 1000;

/// The boundary a multipart content type names, if it is one.
pub fn boundary(ctype: &str) -> Option<String> {
    let mut parts = ctype.split(';');
    if !parts.next()?.trim().eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    for p in parts {
        let (k, v) = p.split_once('=')?;
        if k.trim().eq_ignore_ascii_case("boundary") {
            let b = v.trim().trim_matches('"');
            return (!b.is_empty() && b.len() <= 70).then(|| b.to_string());
        }
    }
    None
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    let first = needle[0];
    let mut i = from;
    while i + needle.len() <= hay.len() {
        match hay[i..hay.len() - needle.len() + 1].iter().position(|&b| b == first) {
            None => return None,
            Some(p) => {
                i += p;
                if &hay[i..i + needle.len()] == needle {
                    return Some(i);
                }
                i += 1;
            }
        }
    }
    None
}

/// A `name="value"` parameter of a Content-Disposition header. A quoted value is read to its
/// closing quote, so a `;` inside it stays, and the three escapes HTML's multipart encoding
/// writes, `%22` for `"`, `%0D` and `%0A`, are undone (FEEDBACK30.md, Q-373; branch
/// feedback27-35-fixes).
fn param(header: &str, name: &str) -> Option<String> {
    let mut rest = header.split_once(';')?.1;
    loop {
        let (k, after) = rest.split_once('=')?;
        let after = after.trim_start();
        let (v, next) = match after.strip_prefix('"') {
            Some(q) => {
                let mut v = String::new();
                let mut chars = q.char_indices();
                let mut end = q.len();
                // No backslash escapes: browsers send `\` as it is (a Windows path), and a quote
                // as `%22`.
                for (i, c) in chars.by_ref() {
                    match c {
                        '"' => {
                            end = i + 1;
                            break;
                        }
                        c => v.push(c),
                    }
                }
                let next = q[end..].split_once(';').map_or("", |(_, n)| n);
                (v, next)
            }
            None => match after.split_once(';') {
                Some((v, n)) => (v.trim().to_string(), n),
                None => (after.trim().to_string(), ""),
            },
        };
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(v.replace("%22", "\"").replace("%0D", "\r").replace("%0A", "\n"));
        }
        if next.is_empty() {
            return None;
        }
        rest = next;
    }
}

/// A Content-Disposition that makes a browser save the answer as `name`: the name in plain
/// ASCII for old readers, and whole in RFC 5987's form (FEEDBACK30.md; branch feedback27-35-fixes).
pub fn attachment(name: &str) -> String {
    let base = base_name(name);
    let plain: String = base.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' && c != '%' || c == ' ' { c } else { '_' }).collect();
    let mut enc = String::new();
    for b in base.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            enc.push(b as char);
        } else {
            enc.push_str(&format!("%{:02X}", b));
        }
    }
    format!("attachment; filename=\"{}\"; filename*=UTF-8''{}", plain, enc)
}

/// A browser's file name without any folders it sent along.
fn base_name(name: &str) -> String {
    name.rsplit(['/', '\\']).next().unwrap_or("").trim().to_string()
}

/// The form's fields and files. A field sent twice keeps the last value; a
/// file field sent with several files is a list of them.
pub fn parse(boundary: &str, body: &[u8]) -> (Map, Map) {
    let mut form = Map::new();
    let mut files = Map::new();
    let delim = format!("--{}", boundary).into_bytes();
    let Some(mut at) = find(body, &delim, 0) else { return (form, files) };
    let next_delim = format!("\r\n--{}", boundary).into_bytes();
    for _ in 0..MAX_PARTS {
        at += delim.len();
        // `--` after the boundary ends the body.
        if body.get(at..at + 2) == Some(b"--") {
            break;
        }
        // The part's headers, up to an empty line.
        let Some(head_end) = find(body, b"\r\n\r\n", at) else { break };
        let head = String::from_utf8_lossy(&body[at..head_end]).into_owned();
        let start = head_end + 4;
        let Some(end) = find(body, &next_delim, start) else { break };
        let mut disposition = String::new();
        let mut ctype = String::from("application/octet-stream");
        for line in head.split("\r\n") {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim().eq_ignore_ascii_case("content-disposition") {
                    disposition = v.trim().to_string();
                } else if k.trim().eq_ignore_ascii_case("content-type") {
                    ctype = v.trim().to_string();
                }
            }
        }
        if let Some(name) = param(&disposition, "name") {
            match param(&disposition, "filename") {
                Some(filename) => {
                    let filename = base_name(&filename);
                    // An empty file input sends a part with no name and no bytes.
                    if !(filename.is_empty() && end == start) {
                        let mut f = Map::new();
                        f.insert("field".into(), Value::str(&name));
                        f.insert("filename".into(), Value::str(&filename));
                        f.insert("type".into(), Value::str(&ctype));
                        f.insert("size".into(), Value::Int((end - start) as i64));
                        f.insert("start".into(), Value::Int(start as i64));
                        f.insert("end".into(), Value::Int(end as i64));
                        let f = Value::map(f);
                        let key = Key::from(name.as_str());
                        let v = match files.shift_remove(&key) {
                            None => f,
                            Some(Value::List(l)) => {
                                let mut l = (*l).clone();
                                l.push(f);
                                Value::list(l)
                            }
                            Some(one) => Value::list(vec![one, f]),
                        };
                        files.insert(key, v);
                    }
                }
                None => {
                    form.insert(name.as_str().into(), Value::str(String::from_utf8_lossy(&body[start..end])));
                }
            }
        }
        at = end + 2;
    }
    (form, files)
}

/// The bytes of upload record `file` in `body`.
pub fn bytes<'a>(file: &Value, body: &'a [u8]) -> Result<&'a [u8], String> {
    let Value::Map(m) = file else { return Err("expects a file from request.files".into()) };
    let (Some(Value::Int(s)), Some(Value::Int(e))) = (m.get("start"), m.get("end")) else {
        return Err("expects a file from request.files".into());
    };
    let (s, e) = (*s as usize, *e as usize);
    body.get(s..e).ok_or_else(|| "this file is not part of the current request".to_string())
}
