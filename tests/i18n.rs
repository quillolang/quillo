//! Translations: choosing the locale, `t`, plurals, cached views, and the
//! same text in components in the browser.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::process::Command;

const WORDS: &str = r#"
translations({
    en: {
        hi: "Hello, {name}!",
        nav: { home: "Home", about: "About" },
        items: { one: "{count} item", other: "{count} items" },
        only_en: "English only",
    },
    fr: {
        hi: "Bonjour, {name} !",
        nav: { home: "Accueil" },
        items: { one: "{count} article", other: "{count} articles" },
    },
    "pt-BR": { hi: "Olá, {name}!" },
})
"#;

fn app(extra: &str) -> Interp {
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source(&format!("{}\n{}", WORDS, extra), "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    it
}

fn get(it: &mut Interp, url: &str, headers: &[(&str, &str)]) -> HttpResponse {
    let mut h = vec![("Host".to_string(), "x".to_string())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "127.0.0.1".into() })
}

#[test]
fn the_locale_comes_from_the_request() {
    let mut it = app(r#"route GET / = `${locale()}: ${t("hi", { name: "Ada" })}`"#);
    let lang = |it: &mut Interp, h: &[(&str, &str)]| get(it, "/", h).text();
    assert_eq!(lang(&mut it, &[]), "en: Hello, Ada!");
    assert_eq!(lang(&mut it, &[("Accept-Language", "fr-CA,fr;q=0.9,en;q=0.8")]), "fr: Bonjour, Ada !");
    assert_eq!(lang(&mut it, &[("Accept-Language", "de, en;q=0.5, fr;q=0.7")]), "fr: Bonjour, Ada !");
    assert_eq!(lang(&mut it, &[("Accept-Language", "pt")]), "pt-BR: Olá, Ada!");
    assert_eq!(lang(&mut it, &[("Accept-Language", "ja, fr;q=0")]), "en: Hello, Ada!");
    // A chosen language (a cookie) wins over the browser's.
    assert_eq!(lang(&mut it, &[("Accept-Language", "fr"), ("Cookie", "locale=en")]), "en: Hello, Ada!");

    let mut it = app(
        r#"
locale_from((r) => r.query.lang)
route GET / = t("nav.home")
"#,
    );
    assert_eq!(get(&mut it, "/?lang=fr", &[("Accept-Language", "en")]).text(), "Accueil");
    assert_eq!(get(&mut it, "/?lang=xx", &[("Accept-Language", "fr")]).text(), "Accueil");
    assert_eq!(get(&mut it, "/", &[]).text(), "Home");
}

#[test]
fn missing_text_falls_back_and_plurals_follow_the_count() {
    let mut it = app(
        r#"
route GET / = [t("only_en"), t("nav.about"), t("no.such.key"), t("items", { count: 1 }), t("items", { count: 3 }), t("items"),
               with_locale("en", () => t("nav.home")), locale()]
"#,
    );
    let r = get(&mut it, "/", &[("Accept-Language", "fr")]);
    assert_eq!(
        r.text(),
        r#"["English only","About","no.such.key","1 article","3 articles","{count} articles","Home","fr"]"#
    );
    // Text from translations is text in pages: escaped like any other.
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source(r#"translations({ en: { x: "<b>{v}</b>" } })
route GET / = <p>{t("x", { v: "<i>" })}</p>"#, "app.qwe").unwrap();
    assert_eq!(get(&mut it, "/", &[]).text(), "<p>&lt;b&gt;&lt;i&gt;&lt;/b&gt;</p>");
}

#[test]
fn translations_load_from_a_folder() {
    let d = std::env::temp_dir().join(format!("quillo-i18n-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("locales")).unwrap();
    std::fs::write(d.join("locales/en.json"), r#"{ "menu": { "open": "Open" } }"#).unwrap();
    std::fs::write(d.join("locales/de.json"), r#"{ "menu": { "open": "Öffnen" } }"#).unwrap();
    std::fs::write(d.join("locales/notes.txt"), "not a locale").unwrap();
    let mut it = Interp::new(&d);
    it.load_source("translations(\"locales\", { default: \"de\" })\nroute GET / = t(\"menu.open\")", "app.qwe").unwrap();
    assert_eq!(get(&mut it, "/", &[]).text(), "Öffnen");
    assert_eq!(get(&mut it, "/", &[("Accept-Language", "en-GB")]).text(), "Open");
    let mut it = Interp::new(&d);
    let Err(e) = it.load_source("translations(\"../x\")", "app.qwe") else { panic!("a folder outside was read") };
    assert!(e.to_string().contains("inside the program"), "{}", e);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn views_are_cached_per_locale() {
    let mut it = app(
        r#"
view Menu() = <nav>{t("nav.home")}</nav>
view Page() = <main><Menu/></main>
route GET / = <Page/>
"#,
    );
    for _ in 0..3 {
        assert_eq!(get(&mut it, "/", &[("Accept-Language", "fr")]).text(), "<main><nav>Accueil</nav></main>");
        assert_eq!(get(&mut it, "/", &[]).text(), "<main><nav>Home</nav></main>");
    }
    let (hits, _) = it.view_cache_stats();
    assert!(hits >= 4, "{} hits", hits);
}

#[test]
fn pages_with_components_carry_their_locales_text() {
    let mut it = app(
        r#"
component Cart() {
    ~n = 1
    <button @click={() => { n += 1 }}>{t("items", { count: n })}</button>
}
route GET / = <html><body><Cart/></body></html>
route GET /plain = <html><body>{t("hi", { name: "</script>" })}</body></html>
"#,
    );
    let page = get(&mut it, "/", &[("Accept-Language", "fr")]).text();
    assert!(page.contains("1 article</button>"), "{}", page);
    let at = page.find("window.__quillo_i18n=").expect("translations for the browser");
    assert!(page[at..].contains(r#""locale":"fr""#) && page[at..].contains(r#""nav.about":"About""#), "{}", &page[at..]);
    assert!(page.find("/_quillo/b/").unwrap() > at);
    // Only pages with components, and never a way out of the script.
    assert!(!get(&mut it, "/plain", &[]).text().contains("__quillo_i18n"));
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source(r#"translations({ en: { x: "</script><script>alert(1)</script>" } })
component C() { <p>{t("x")}</p> }
route GET / = <html><body><C/></body></html>"#, "app.qwe").unwrap();
    let page = get(&mut it, "/", &[]).text();
    assert_eq!(page.matches("</script>").count(), 2, "{}", page);
}

#[test]
fn the_browser_translates_as_the_server_does() {
    if !Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
        return;
    }
    let src = r#"
translations({ ru: { files: { one: "{count} файл", few: "{count} файла", many: "{count} файлов", other: "{count} файла" }, hi: "Привет, {name}" } })
fn __t() = [0, 1, 2, 5, 11, 21, 22, 1.5] | map((n) => t("files", { count: n })) | push(t("hi", { name: "Ада" })) | push(t("nope")) | push(locale())
"#;
    let mut it = Interp::new(".");
    it.load_source(src, "case.qwe").unwrap();
    let server = it.call(&quillo::value::lookup(&it.globals, "__t").unwrap(), vec![], 0).ok().unwrap().repr();
    let words = it.site.i18n.as_ref().unwrap().browser_json("ru");
    let js = format!("globalThis.window = {{ __quillo_i18n: {} }};\n{}", words, quillo::client::test_script(&it, "__t").unwrap());
    let out = Command::new("node").arg("-e").arg(&js).output().expect("node");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(server, String::from_utf8_lossy(&out.stdout).trim());
    assert!(server.contains("\"21 файл\"") && server.contains("\"22 файла\"") && server.contains("\"5 файлов\""), "{}", server);
}
