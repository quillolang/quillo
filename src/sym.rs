//! Names of variables, each text held once, so two are the same name when they are the
//! same pointer. A scope compares a pointer where it would compare text, and most of a
//! loop's time went there (FEEDBACK29.md; branch feedback29-fixes).
//!
//! A name is kept for good, so only what a program's source says becomes one: looking up
//! text that is not a name yet finds nothing and adds nothing. A script from a visitor
//! (src/script.rs) is parsed `in_script`, where a name the program does not have stands for
//! itself in that script alone, as `$1`, `$2` and so on, which no Quillo name can be: the
//! table grows to the most new names one script has, not with the scripts run.

use crate::value::BuildFx;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

/// A name of a variable: `&str` to everything that reads it.
#[derive(Clone, Copy)]
pub struct Sym(&'static str);

thread_local! {
    /// While a script is parsed: its own names, and what they stand for.
    static SCRIPT: RefCell<Option<HashMap<String, Sym>>> = const { RefCell::new(None) };
}

fn table() -> &'static RwLock<HashSet<&'static str, BuildFx>> {
    static TABLE: OnceLock<RwLock<HashSet<&'static str, BuildFx>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

impl Sym {
    /// The name with this text, made if it is the first.
    pub fn new(s: &str) -> Sym {
        // A script's own first, so that each of its names stands for one thing however the
        // table grows meanwhile.
        if let Some(n) = SCRIPT.with(|sc| sc.borrow().as_ref().and_then(|m| m.get(s).copied())) {
            return n;
        }
        if let Some(n) = Sym::known(s) {
            return n;
        }
        if let Some(n) = SCRIPT.with(|sc| {
            let mut sc = sc.borrow_mut();
            let m = sc.as_mut()?;
            let n = Sym::interned(&format!("${}", m.len() + 1));
            m.insert(s.to_string(), n);
            Some(n)
        }) {
            return n;
        }
        Sym::interned(s)
    }

    fn interned(s: &str) -> Sym {
        if let Some(n) = Sym::known(s) {
            return n;
        }
        let mut t = table().write().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Another thread may have made it since.
        if let Some(n) = t.get(s) {
            return Sym(n);
        }
        let n: &'static str = Box::leak(s.to_string().into_boxed_str());
        t.insert(n);
        Sym(n)
    }

    /// The name with this text, if there is one. No variable can have it otherwise.
    pub fn known(s: &str) -> Option<Sym> {
        table().read().unwrap_or_else(std::sync::PoisonError::into_inner).get(s).map(|n| Sym(n))
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }

    /// One bit of 64 for this name, for a scope to say it may hold it.
    #[inline(always)]
    pub fn bit(self) -> u64 {
        1 << ((self.0.as_ptr() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58)
    }

    /// `f`, with the names it makes a script's own (see the top of this file), and what each
    /// of those stands for.
    pub fn in_script<R>(f: impl FnOnce() -> R) -> (R, HashMap<String, Sym>) {
        let outer = SCRIPT.with(|sc| sc.replace(Some(HashMap::new())));
        let r = f();
        let names = SCRIPT.with(|sc| std::mem::replace(&mut *sc.borrow_mut(), outer)).unwrap_or_default();
        (r, names)
    }
}

impl PartialEq for Sym {
    #[inline(always)]
    fn eq(&self, other: &Sym) -> bool {
        std::ptr::eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl Eq for Sym {}

impl std::hash::Hash for Sym {
    #[inline(always)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.0.as_ptr() as usize);
    }
}

impl PartialOrd for Sym {
    fn partial_cmp(&self, other: &Sym) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Sym {
    fn cmp(&self, other: &Sym) -> std::cmp::Ordering {
        self.0.cmp(other.0)
    }
}

impl std::ops::Deref for Sym {
    type Target = str;
    #[inline(always)]
    fn deref(&self) -> &str {
        self.0
    }
}

impl std::fmt::Debug for Sym {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.0, f)
    }
}

impl std::fmt::Display for Sym {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl PartialEq<str> for Sym {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Sym {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for Sym {
    fn eq(&self, other: &String) -> bool {
        self.0 == other
    }
}

impl PartialEq<Sym> for String {
    fn eq(&self, other: &Sym) -> bool {
        self == other.0
    }
}

impl PartialEq<Sym> for &str {
    fn eq(&self, other: &Sym) -> bool {
        *self == other.0
    }
}

impl PartialEq<Sym> for str {
    fn eq(&self, other: &Sym) -> bool {
        self == other.0
    }
}

impl From<&str> for Sym {
    fn from(s: &str) -> Sym {
        Sym::new(s)
    }
}

impl From<&String> for Sym {
    fn from(s: &String) -> Sym {
        Sym::new(s)
    }
}

impl From<String> for Sym {
    fn from(s: String) -> Sym {
        Sym::new(&s)
    }
}

impl From<Sym> for String {
    fn from(s: Sym) -> String {
        s.0.to_string()
    }
}

impl AsRef<str> for Sym {
    fn as_ref(&self) -> &str {
        self.0
    }
}

/// What a scope can be asked for a variable by: a name, or text that may be one.
pub trait Name {
    /// The name, if there is one with this text.
    fn known(self) -> Option<Sym>;
    /// The name, made if needed: for a variable being defined.
    fn make(self) -> Sym;
}

impl Name for Sym {
    #[inline(always)]
    fn known(self) -> Option<Sym> {
        Some(self)
    }
    #[inline(always)]
    fn make(self) -> Sym {
        self
    }
}

impl Name for &Sym {
    #[inline(always)]
    fn known(self) -> Option<Sym> {
        Some(*self)
    }
    #[inline(always)]
    fn make(self) -> Sym {
        *self
    }
}

impl Name for &str {
    fn known(self) -> Option<Sym> {
        Sym::known(self)
    }
    fn make(self) -> Sym {
        Sym::new(self)
    }
}

impl Name for &&str {
    fn known(self) -> Option<Sym> {
        Sym::known(self)
    }
    fn make(self) -> Sym {
        Sym::new(self)
    }
}

impl Name for &&String {
    fn known(self) -> Option<Sym> {
        Sym::known(self)
    }
    fn make(self) -> Sym {
        Sym::new(self)
    }
}

impl Name for &String {
    fn known(self) -> Option<Sym> {
        Sym::known(self)
    }
    fn make(self) -> Sym {
        Sym::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_text_is_one_name() {
        assert!(Sym::new("sym_test_a") == Sym::new(&String::from("sym_test_a")));
        assert!(Sym::new("sym_test_a") != Sym::new("sym_test_b"));
        assert!(Sym::new("") == Sym::new(""));
        assert!(Sym::new("") != Sym::new("sym_test_a"));
        assert!(Sym::new("名前") == Sym::new("名前"));
        assert_eq!(Sym::new("名前").as_str(), "名前");
        assert_eq!(Sym::new("sym_test_a").len(), 10);
    }

    #[test]
    fn asking_for_text_adds_nothing() {
        assert!(Sym::known("sym_test_never_defined").is_none());
        assert!(Sym::known("sym_test_never_defined").is_none());
        let n = Sym::new("sym_test_defined");
        assert!(Sym::known("sym_test_defined") == Some(n));
    }

    #[test]
    fn a_scripts_names_are_its_own() {
        let known = Sym::new("sym_test_known");
        let (made, names) = Sym::in_script(|| {
            let a = Sym::new("sym_test_script_a");
            (a, Sym::new("sym_test_script_b"), Sym::new("sym_test_script_a"), Sym::new("sym_test_known"))
        });
        // The same text is the same name, another text another, and what the program has is
        // the program's.
        assert!(made.0 == made.2 && made.0 != made.1 && made.3 == known);
        assert!(made.0.starts_with('$') && made.1.starts_with('$'));
        assert_eq!(names.len(), 2);
        assert!(names["sym_test_script_a"] == made.0);
        // Nothing of the script's became a name for the rest.
        assert!(Sym::known("sym_test_script_a").is_none());
        // The next script uses the same ones again.
        let (again, _) = Sym::in_script(|| Sym::new("sym_test_script_c"));
        assert!(again == made.0);
    }

    #[test]
    fn threads_agree_on_a_name() {
        let names: Vec<usize> = std::thread::scope(|s| {
            let ts: Vec<_> = (0..8).map(|_| s.spawn(|| Sym::new("sym_test_threads").as_ptr() as usize)).collect();
            ts.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert!(names.iter().all(|p| *p == names[0]));
    }
}
