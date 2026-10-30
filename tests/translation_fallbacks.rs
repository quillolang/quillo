//! Which words `t` falls back on: the first locale when none is `en`, and the
//! `other` form when the count's own form was not written.

use quillo::server::HttpRequest;
use quillo::Interp;

fn page(src: &str, url: &str) -> String {
    let mut it = Interp::new(&std::env::temp_dir());
    it.load_source(src, "app.qwe").unwrap_or_else(|e| panic!("{}", e));
    let h = vec![("Host".to_string(), "x".to_string())];
    let r = it.handle(&HttpRequest { method: "GET".into(), url: url.into(), headers: h, body: vec![], remote: "127.0.0.1".into() });
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    String::from_utf8_lossy(&r.body).into_owned()
}

#[test]
fn with_no_english_the_first_locale_is_the_default() {
    let src = "translations({ fr: { hi: \"Salut\" }, de: { hi: \"Hallo\" } })\nroute GET / = t(\"hi\")\n";
    assert_eq!(page(src, "/"), "Salut");
}

#[test]
fn with_english_among_them_it_is_the_default_wherever_it_stands() {
    let src = "translations({ fr: { hi: \"Salut\" }, en: { hi: \"Hello\" } })\nroute GET / = t(\"hi\")\n";
    assert_eq!(page(src, "/"), "Hello");
}

#[test]
fn a_count_with_no_form_of_its_own_takes_the_other_form() {
    let src = "translations({ en: { apples: { other: \"{count} apples\" }, pears: { one: \"a pear\", other: \"{count} pears\" } } })\n\
               route GET /:n(int) = t(\"apples\", { count: n }) + \", \" + t(\"pears\", { count: n })\n";
    assert_eq!(page(src, "/1"), "1 apples, a pear");
    assert_eq!(page(src, "/2"), "2 apples, 2 pears");
}
