//! Translations: what `translations` refuses, plural forms language by
//! language, and the arguments `t`, `locale_from` and `with_locale` take.

use indexmap::IndexMap;
use quillo::i18n::{plural, I18n};
use quillo::server::{HttpRequest, HttpResponse};
use quillo::value::Value;
use quillo::Interp;
use std::collections::HashMap;
use std::path::PathBuf;

fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("quillo-plurals-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app(src: &str) -> Interp {
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}\n{}", e, src));
    it
}

fn get(it: &mut Interp, url: &str, headers: &[(&str, &str)]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "x".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "127.0.0.1".into() })
}

#[track_caller]
fn refused(src: &str, wanted: &str) {
    refused_in(&std::env::temp_dir(), src, wanted);
}

#[track_caller]
fn refused_in(dir: &std::path::Path, src: &str, wanted: &str) {
    let mut it = Interp::new(dir);
    match it.load_source(src, "app.qwe") {
        Ok(_) => panic!("expected an error\n{}", src),
        Err(e) => assert!(e.to_string().contains(wanted), "{}\n  wanted: {}\n  got: {}", src, wanted, e),
    }
}

#[test]
fn translations_refuses_what_is_not_text() {
    refused("translations({ en: { a: { one: 1, other: \"x\" } } })", "`translations` (en): the plural forms of `a` must be text");
    refused("translations({ en: { a: 5 } })", "`translations` (en): `a` must be text or a map, not int");
    refused("translations({ en: { a: { b: [1] } } })", "`translations` (en): `a.b` must be text or a map, not list");
    refused("translations({ en: 5 })", "`translations`: `en` must be a map, not int");
    refused("translations(5)", "`translations` expects a folder or a map of locales, got int");
    refused("translations()", "`translations` expects a folder or a map of locales, got nothing");
    refused("translations({ \"not a locale\": {} })", "`translations`: \"not a locale\" is not a locale name such as \"en\" or \"pt-BR\"");
    refused("translations({})", "`translations` found no locales");
    refused("translations(\"../up\")", "`translations` needs a folder inside the program's, got \"../up\"");
}

#[test]
fn translations_says_what_is_wrong_with_its_folder() {
    let d = folder("dir");
    refused_in(&d, "translations(\"missing\")", "`translations`: cannot read");
    std::fs::create_dir_all(d.join("empty")).unwrap();
    std::fs::write(d.join("empty/notes.txt"), "not a locale").unwrap();
    refused_in(&d, "translations(\"empty\")", "`translations` found no locales");
    std::fs::create_dir_all(d.join("list")).unwrap();
    std::fs::write(d.join("list/en.json"), "[1, 2]").unwrap();
    refused_in(&d, "translations(\"list\")", "en.json must hold a JSON object");
    std::fs::create_dir_all(d.join("broken")).unwrap();
    std::fs::write(d.join("broken/en.json"), "{").unwrap();
    refused_in(&d, "translations(\"broken\")", "invalid JSON");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_default_locale_must_be_one_of_them() {
    refused("translations({ en: {}, fr: {} }, { default: \"de\" })", "`translations`: the default \"de\" is not among the locales");
    let mut it = app("translations({ de: { a: \"Hallo\" }, en: { a: \"Hello\" } }, {})\nroute GET / = locale()\n");
    assert_eq!(get(&mut it, "/", &[]).text(), "en", "`en` is the default when there is one");
    let mut it = app("translations({ de: { a: \"Hallo\" }, fr: { a: \"Salut\" } }, { locale: \"fr\" })\nroute GET / = locale()\n");
    assert_eq!(get(&mut it, "/", &[]).text(), "de", "else the first");
    let mut it = app("translations({ de: { a: \"Hallo\" }, fr: { a: \"Salut\" } }, { default: \"FR\" })\nroute GET / = locale()\n");
    assert_eq!(get(&mut it, "/", &[]).text(), "fr", "named in any case");
}

#[test]
fn locale_from_needs_a_function_that_names_a_locale() {
    refused("locale_from(5)", "`locale_from` expects a function of the request, got int");
    refused("locale_from()", "`locale_from` expects a function of the request, got nothing");
    let words = "translations({ en: { a: \"Hello\" }, fr: { a: \"Salut\" } })\n";
    // Nothing named, and a name with nothing in it, leave the request's own.
    let mut it = app(&format!("{}locale_from((r) => r.query.lang)\nroute GET / = locale()\n", words));
    assert_eq!(get(&mut it, "/", &[("Accept-Language", "fr")]).text(), "fr");
    assert_eq!(get(&mut it, "/?lang=", &[("Accept-Language", "fr")]).text(), "fr");
    assert_eq!(get(&mut it, "/?lang=en", &[("Accept-Language", "fr")]).text(), "en");
    let mut it = app(&format!("{}locale_from((r) => 5)\nroute GET / = locale()\n", words));
    assert_eq!(get(&mut it, "/", &[("Accept-Language", "fr")]).text(), "fr");
}

#[test]
fn with_locale_and_t_say_what_they_take() {
    let words = "translations({ en: { a: \"Hello\" }, fr: { a: \"Salut\" } })\n";
    refused(&format!("{}with_locale(5, () => 1)", words), "`with_locale` expects a locale name, got int");
    refused(&format!("{}with_locale()", words), "`with_locale` expects a locale name, got nothing");
    refused(&format!("{}t(5)", words), "`t` expects a key, got int");
    refused(&format!("{}t()", words), "`t` expects a key, got nothing");
    refused(&format!("{}t(\"a\", 5)", words), "`t` expects its values as a map, got int");
    let mut it = app(&format!("{}route GET / = with_locale(\"\", () => t(\"a\"))\n", words));
    assert_eq!(get(&mut it, "/", &[]).text(), "Hello", "no name is the default");
}

#[test]
fn the_browsers_copy_of_a_locale_not_there_is_the_default() {
    let mut en = HashMap::new();
    en.insert("hi".to_string(), Value::str("Hello </script>"));
    let mut fr = HashMap::new();
    fr.insert("hi".to_string(), Value::str("Salut"));
    let mut locales = IndexMap::new();
    locales.insert("en".into(), en);
    locales.insert("fr".into(), fr);
    let i18n = I18n { default: "en".into(), locales };
    assert_eq!(i18n.browser_json("xx"), "{\"locale\":\"xx\",\"t\":{\"hi\":\"Hello \\u003c/script>\"}}");
    assert_eq!(i18n.browser_json("fr"), "{\"locale\":\"fr\",\"t\":{\"hi\":\"Salut\"}}");
    assert_eq!(i18n.best(""), None);
    assert_eq!(i18n.best("  ").as_deref(), None);
    assert_eq!(i18n.best("FR_ca").as_deref(), Some("fr"));
}

fn forms(locale: &str, counts: &[f64]) -> Vec<&'static str> {
    counts.iter().map(|n| plural(locale, *n)).collect()
}

#[test]
fn slavic_languages_have_their_forms() {
    let n = [1.0, 2.0, 4.0, 5.0, 11.0, 12.0, 21.0, 22.0, 25.0, 101.0, 112.0];
    for l in ["sr", "hr", "bs-Latn"] {
        assert_eq!(forms(l, &n), ["one", "few", "few", "other", "other", "other", "one", "few", "other", "one", "other"], "{}", l);
    }
    assert_eq!(forms("pl", &n), ["one", "few", "few", "many", "many", "many", "many", "few", "many", "many", "many"]);
    for l in ["cs", "sk"] {
        assert_eq!(forms(l, &n), ["one", "few", "few", "other", "other", "other", "other", "other", "other", "other", "other"], "{}", l);
    }
    assert_eq!(forms("uk", &[1.0, 3.0, 5.0]), ["one", "few", "many"]);
}

#[test]
fn arabic_has_six_forms_and_hebrew_three() {
    let n = [0.0, 1.0, 2.0, 3.0, 10.0, 11.0, 99.0, 100.0, 101.0, 103.0, 111.0];
    assert_eq!(forms("ar", &n), ["zero", "one", "two", "few", "few", "many", "many", "other", "other", "few", "many"]);
    assert_eq!(forms("he", &[0.0, 1.0, 2.0, 3.0, 20.0]), ["other", "one", "two", "other", "other"]);
    assert_eq!(forms("ar", &[1.5, f64::INFINITY]), ["other", "other"]);
}

#[test]
fn t_picks_the_form_for_the_visitors_language() {
    let mut it = app(
        r#"
translations({
    en: { files: { one: "{count} file", other: "{count} files" } },
    pl: { files: { one: "{count} plik", few: "{count} pliki", many: "{count} plików", other: "{count} pliku" } },
    ar: { files: { zero: "لا ملفات", one: "ملف", two: "ملفان", few: "{count} ملفات", many: "{count} ملفًا", other: "{count} ملف" } },
})
route GET /:n = t("files", { count: int(n) })
"#,
    );
    let got = |it: &mut Interp, url: &str, lang: &str| get(it, url, &[("Accept-Language", lang)]).text();
    assert_eq!(got(&mut it, "/1", "pl"), "1 plik");
    assert_eq!(got(&mut it, "/3", "pl"), "3 pliki");
    assert_eq!(got(&mut it, "/5", "pl"), "5 plików");
    assert_eq!(got(&mut it, "/22", "pl"), "22 pliki");
    assert_eq!(got(&mut it, "/0", "ar"), "لا ملفات");
    assert_eq!(got(&mut it, "/2", "ar"), "ملفان");
    assert_eq!(got(&mut it, "/100", "ar"), "100 ملف");
}
