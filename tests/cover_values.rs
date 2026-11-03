//! The corners of names, values and numbers (src/sym.rs, src/value.rs, src/numeric.rs) no
//! other test reaches, each by what its caller sees.

use quillo::sym::{Name, Sym};
use quillo::value::{self, Place, Scope, Value};
use std::cmp::Ordering;
use std::sync::Arc;

/// `r` after the program runs, as `print` shows it, or the error.
fn r_of(src: &str) -> String {
    let mut it = quillo::Interp::new(".");
    match it.load_source(src, "main.qwe") {
        Ok(_) => value::lookup(&it.globals, "r").map_or("no r".to_string(), |v| v.display()),
        Err(e) => format!("line {}: {}", e.line, e.msg),
    }
}

#[test]
fn names_sort_by_their_text_not_by_where_they_are_kept() {
    // Made in the opposite order to their text's, so an order by address would differ.
    let z = Sym::new("cover-values-zebra");
    let a = Sym::new("cover-values-aardvark");
    let m = Sym::new("cover-values-mole");
    assert_eq!(a.partial_cmp(&z), Some(Ordering::Less));
    assert_eq!(z.cmp(&a), Ordering::Greater);
    assert_eq!(m.cmp(&m), Ordering::Equal);
    let mut all = vec![z, m, a];
    all.sort();
    assert_eq!(all.iter().map(|s| s.as_str()).collect::<Vec<_>>(), ["cover-values-aardvark", "cover-values-mole", "cover-values-zebra"]);
    assert!(a < m && m < z);
}

#[test]
fn a_name_compares_with_its_text_from_either_side() {
    let n = Sym::new("cover-values-text");
    let text: &str = "cover-values-text";
    assert!(text == n);
    // Called through a pointer, as a caller elsewhere would, not folded into this test.
    let eq: fn(&&'static str, &Sym) -> bool = <&'static str as PartialEq<Sym>>::eq;
    let eq = std::hint::black_box(eq);
    assert!(eq(&text, &n));
    assert!(!eq(&"cover-values-other", &n));
    assert!(!("cover-values-other" == n));
    assert!(<str as PartialEq<Sym>>::eq(text, &n));
    assert!(!<str as PartialEq<Sym>>::eq("cover-values-other", &n));
    assert!(String::from("cover-values-text") == n);
}

#[test]
fn a_name_becomes_its_text() {
    let n = Sym::new("cover-values-owned");
    let s: String = n.into();
    assert_eq!(s, "cover-values-owned");
    let r: &str = n.as_ref();
    assert_eq!(r, "cover-values-owned");
    assert_eq!(r.as_ptr(), n.as_str().as_ptr(), "the same text, not a copy");
}

#[test]
fn a_name_made_from_a_reference_to_a_str_reference_is_that_name() {
    let text = "cover-values-made-twice-removed";
    assert_eq!(Sym::known(text), None, "not made yet");
    let made = Name::make(&text);
    assert_eq!(made.as_str(), text);
    assert_eq!(Sym::known(text), Some(made));
    // Made again, it is the same name.
    assert_eq!(Name::make(&text), made);
    assert_eq!(Name::known(&text), Some(made));
    // And from a reference to a String reference.
    let owned = String::from("cover-values-made-from-a-string");
    let from_string = Name::make(&&owned);
    assert_eq!(from_string.as_str(), "cover-values-made-from-a-string");
    assert_eq!(Name::known(&&owned), Some(from_string));
}

#[test]
fn a_place_with_a_file_and_no_line_is_the_file() {
    let f: Arc<str> = Arc::from("cover-values/lib.qwe");
    assert_eq!(Place::new(Some(&f), 0).to_string(), "cover-values/lib.qwe");
    assert_eq!(Place::new(Some(&f), 12).to_string(), "cover-values/lib.qwe:12");
    assert_eq!(Place::new(Some(&f), 0).before("bad"), "cover-values/lib.qwe: bad");
    assert_eq!(Place::new(None, 7).to_string(), "line 7");
    assert_eq!(Place::NONE.before("bad"), "bad");
}

#[test]
fn a_name_no_scope_has_cannot_be_taken() {
    let root = Scope::root();
    let local = Scope::child(&root);
    value::define(&local, "cover_values_here", Value::Int(1));
    assert!(value::takable(&local, Sym::new("cover_values_here")));
    assert!(!value::takable(&local, Sym::new("cover_values_nowhere")), "found in no scope");
    assert!(!value::takable(&root, Sym::new("cover_values_nowhere")));
    // A global, outside a program's loading, is not the code's to take.
    value::define(&root, "cover_values_global", Value::Int(2));
    assert!(!value::takable(&local, Sym::new("cover_values_global")));
}

#[test]
fn only_a_list_by_an_int_or_a_map_is_updated_inside() {
    let mut n = Value::Int(5);
    assert!(!value::update_inside(&mut n, &Value::Int(0), None, vec![Value::Int(1)]));
    assert_eq!(n.repr(), "5");
    let mut s = Value::str("text");
    assert!(!value::update_inside(&mut s, &Value::Int(0), None, vec![Value::Int(1)]));
    assert_eq!(s.repr(), "\"text\"");
    // A list by a key that is not an int: left as it was.
    let mut l = Value::list(vec![Value::list(vec![Value::Int(1)])]);
    assert!(!value::update_inside(&mut l, &Value::str("0"), None, vec![Value::Int(0), Value::Int(9)]));
    assert_eq!(l.repr(), "[[1]]");
    // By an int, it is.
    assert!(value::update_inside(&mut l, &Value::Int(0), None, vec![Value::Int(0), Value::Int(9)]));
    assert_eq!(l.repr(), "[[9]]");
}

#[test]
fn globals_written_from_two_threads_at_once_keep_every_write() {
    // The globals' scope is guarded by a flag each access waits on: two threads at it
    // together make one wait for the other.
    let root = Scope::root();
    let names: Vec<Vec<Sym>> = (0..2).map(|t| (0..64).map(|i| Sym::new(&format!("cover_values_t{}_{}", t, i))).collect()).collect();
    std::thread::scope(|s| {
        for (t, ns) in names.iter().enumerate() {
            let root = &root;
            s.spawn(move || {
                for round in 0..20_000i64 {
                    let n = ns[round as usize % ns.len()];
                    value::define(root, n, Value::Int(round * 2 + t as i64));
                    assert!(value::lookup(root, n).is_some());
                }
            });
        }
    });
    for (t, ns) in names.iter().enumerate() {
        for (i, n) in ns.iter().enumerate() {
            // The last round that wrote name i.
            let last = ((20_000 - 1 - i) / 64 * 64 + i) as i64;
            assert_eq!(value::lookup(&root, *n).map(|v| v.repr()), Some((last * 2 + t as i64).to_string()), "{}", n);
        }
    }
}

#[test]
fn an_exact_sum_whose_last_partial_cancels_to_zero_is_the_rest() {
    // 1e100 and -1e100 cancel to a partial of 0, the 1 is left: the sum is exact.
    assert_eq!(r_of("let r = math.fsum([1e100, 1.0, -1e100])\n"), "1.0");
    assert_eq!(r_of("let r = math.fsum([1e100, 1.0, -1e100, 2.0, 1e100, -1e100])\n"), "3.0");
    assert_eq!(r_of("let r = math.fsum([-1e100, -1.5, 1e100])\n"), "-1.5");
    // Summed in chunks of 32768: 1e100 in the first, 1 and -1e100 in the second. The
    // chunks' partials cancel to 0 only when added together.
    let chunks = "let r = math.fsum(range(0, 32770) | map((i) => i == 0 ? 1e100 : (i == 32768 ? 1.0 : (i == 32769 ? -1e100 : 0.0))))\n";
    assert_eq!(r_of(chunks), "1.0");
    let chunks3 = "let r = math.fsum(range(0, 32771) | map((i) => i == 0 ? 1e100 : (i == 32768 ? 2.5 : (i == 32769 ? -1e100 : (i == 32770 ? 0.25 : 0.0)))))\n";
    assert_eq!(r_of(chunks3), "2.75");
}
