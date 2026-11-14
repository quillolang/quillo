//! HTML escaping (`quillo::interp::escape`), whose search for the next byte to change goes
//! sixteen bytes at a time with NEON or SSE2 (branch speed-fortunes): the same text as the
//! byte-by-byte rule, wherever the bytes fall against those sixteen.

use quillo::interp::escape;

/// The rule, a byte at a time.
fn slowly(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\r' => out.push_str("&#13;"),
            '\0' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

const SPECIAL: [char; 7] = ['&', '<', '>', '"', '\'', '\r', '\0'];

#[test]
fn each_special_byte_at_each_place_around_sixteen() {
    for len in 0..70 {
        for at in 0..len {
            for &c in &SPECIAL {
                let mut s: Vec<char> = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-_.,:; ".chars().cycle().take(len).collect();
                s[at] = c;
                let s: String = s.into_iter().collect();
                assert_eq!(escape(&s), slowly(&s), "{:?} at {} of {}", c, at, len);
            }
        }
    }
}

#[test]
fn runs_of_special_bytes_and_none() {
    for len in 0..50 {
        for &c in &SPECIAL {
            let s: String = std::iter::repeat(c).take(len).collect();
            assert_eq!(escape(&s), slowly(&s), "{} of {:?}", len, c);
        }
        let plain = "x".repeat(len);
        assert_eq!(escape(&plain), plain);
    }
    // Every special byte, in every order of two, at the end of a block and across it.
    for pad in 12..20 {
        for &a in &SPECIAL {
            for &b in &SPECIAL {
                let s = format!("{}{}{}tail", "p".repeat(pad), a, b);
                assert_eq!(escape(&s), slowly(&s));
            }
        }
    }
}

#[test]
fn text_of_many_bytes_a_character_is_kept_whole() {
    // Bytes of 0x80 and over (UTF-8's), among them those whose low bits are a special byte's
    // (0xA6 is '&' | 0x80, 0xBC '<' | 0x80): none is changed.
    let parts = ["é", "フレーム", "😀", "<", "ü&", "¦", "¼", "\u{2028}", "'", "a\r\nb", "\0"];
    let mut seed = 7u64;
    for _ in 0..3000 {
        let mut s = String::new();
        for _ in 0..(seed % 40) {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            s.push_str(parts[(seed >> 33) as usize % parts.len()]);
        }
        assert_eq!(escape(&s), slowly(&s), "{:?}", s);
    }
}

#[test]
fn the_fortunes_page_s_text() {
    let s = "<script>alert(\"This should not be displayed in a browser alert box.\");</script>";
    assert_eq!(escape(s), "&lt;script&gt;alert(&quot;This should not be displayed in a browser alert box.&quot;);&lt;/script&gt;");
    assert_eq!(escape("A computer scientist is someone who fixes things that aren't broken."), "A computer scientist is someone who fixes things that aren&#39;t broken.");
    assert_eq!(escape("フレームワークのベンチマーク"), "フレームワークのベンチマーク");
    assert_eq!(escape(""), "");
}

/// `push_short_bytes` (and `push_short`, which `escape` uses): the bytes appended, every length
/// to past its 64, onto a vector of any length and with or without room (branch speed-fortunes).
#[test]
fn short_copies_append_exactly_what_they_are_given() {
    let src: Vec<u8> = (0..200u32).map(|i| (i * 7 + 3) as u8).collect();
    for n in 0..=130 {
        for before in [0usize, 1, 5, 15, 16, 63, 100] {
            for room in [false, true] {
                let mut v: Vec<u8> = (0..before).map(|i| i as u8 ^ 0x55).collect();
                if room {
                    v.reserve(200);
                } else {
                    v.shrink_to_fit();
                }
                let mut want = v.clone();
                want.extend_from_slice(&src[..n]);
                quillo::interp::push_short_bytes(&mut v, &src[..n]);
                assert_eq!(v, want, "{} after {} (room: {})", n, before, room);
            }
        }
    }
}
