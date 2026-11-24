//! `page_head`'s reading of a head's metas and links as the browser reads them, found by the review
//! of 1c77039 (FEEDBACK84.md, Q-1320): an attribute's value with character references in it
//! (`name="descr&#105;ption"`), which the browser decodes, and a meta whose `name` is not one of
//! `page_head`'s but whose `property` is (`name="twitter:description" property="og:description"`).
//! Each was left beside `page_head`'s own, so a crawler read two. Each page's head as parse5 7.3.0
//! reads it (scripting on and off) was checked to hold the meta, link or title the test says
//! (src/head.rs; branch feedback82-85).

use quillo::head::{apply, Head, KEYS};

/// A head that says `pairs`, each at depth 0 in no component, in their order.
fn said_head(pairs: &[(&str, &str)]) -> Head {
    let mut h = Head::default();
    for (k, v) in pairs {
        h.say(KEYS.iter().position(|x| x == k).unwrap(), 0, 0, v.to_string());
        h.called();
    }
    h
}

/// `page` with `h`'s tags in its head.
fn with_head(page: &str, h: &Head) -> String {
    let mut b = page.as_bytes().to_vec();
    apply(&mut b, h);
    String::from_utf8(b).unwrap()
}

/// A document whose head is `head`.
fn doc(head: &str) -> String {
    format!("<!doctype html><html><head>{head}</head><body>b</body></html>")
}

/// What `page_head({ title: "T" })` writes.
const TT: &str = "<title>T</title><meta property=\"og:title\" content=\"T\">";

/// What `page_head({ description: "D" })` writes.
const DD: &str = "<meta name=\"description\" content=\"D\"><meta property=\"og:description\" content=\"D\">";

/// What `page_head({ canonical: "https://x.example/a" })` writes.
const CC: &str = "<link rel=\"canonical\" href=\"https://x.example/a\"><meta property=\"og:url\" content=\"https://x.example/a\">";

/// A meta whose name or property is a description once its character references are decoded, as
/// the browser decodes them in an attribute's value (a number, decimal or hex, `x` or `X`, the `;`
/// left out or not, leading zeros; a named one; a capital letter, folded after it is decoded;
/// unquoted; every letter of it), is cut, and `page_head`'s description is the only one. One that
/// is not a description decoded is left: a character past ASCII (`&#305;`, dotless i; `&#x169;`,
/// whose low byte is an `i`), a `&#` with no digits, 0 and a number past U+10FFFF (U+FFFD), one
/// letter short or one more, a named reference not one of the standard's (`&colon` without its
/// `;`), and the empty name (src/head.rs value_is, FEEDBACK84.md Q-1320, found by the review of
/// 1c77039; branch feedback82-85).
#[test]
fn a_metas_name_is_read_with_its_character_references_decoded() {
    let h = said_head(&[("description", "D")]);
    for head in [
        "<meta name=\"descr&#105;ption\" content=\"OLD\">",
        "<meta name=\"descr&#x69;ption\" content=\"OLD\">",
        "<meta name=\"descr&#X49;ption\" content=\"OLD\">",
        "<meta name=\"descr&#105ption\" content=\"OLD\">",
        "<meta name=\"descr&#0000105;ption\" content=\"OLD\">",
        "<meta name=descr&#105;ption content=OLD>",
        "<meta name='&#68;&#69;&#83;&#67;&#82;&#73;&#80;&#84;&#73;&#79;&#78;' content='OLD'>",
        "<meta property=\"og&colon;description\" content=\"OLD\">",
        "<meta property=\"og&#58;descr&#x0069;ption\" content=\"OLD\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(DD), "{head}");
    }
    for head in [
        "<meta name=\"descr&#305;ption\" content=\"K\">",
        "<meta name=\"descr&#x169;ption\" content=\"K\">",
        "<meta name=\"&#;description\" content=\"K\">",
        "<meta name=\"description&#0;\" content=\"K\">",
        "<meta name=\"descr&#x110069;ption\" content=\"K\">",
        "<meta name=\"descr&#105;ptio\" content=\"K\">",
        "<meta name=\"descr&#105;ptions\" content=\"K\">",
        "<meta property=\"og&colondescription\" content=\"K\">",
        "<meta name=\"\" content=\"K\">",
        "<meta name=\"déscription\" content=\"K\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{DD}")), "{head}");
    }
}

/// A canonical link whose `rel` holds `canonical` once its references are decoded (a space or a tab
/// by reference splits it, as one written does; a letter by reference) is cut; one whose `rel`
/// does not (a no-break space, `&nbsp;`, splits nothing; a comma by reference is no space) is left
/// (src/head.rs has_token, FEEDBACK84.md Q-1320, found by the review of 1c77039; branch
/// feedback82-85).
#[test]
fn a_links_rel_is_read_with_its_character_references_decoded() {
    let h = said_head(&[("canonical", "https://x.example/a")]);
    for head in [
        "<link rel=\"icon&#32;canonical\" href=\"/old\">",
        "<link rel=\"canon&#105;cal&Tab;x\" href=\"/old\">",
        "<link rel=\"x&NewLine;&#x43;ANONICAL\" href=\"/old\">",
        "<link rel=\"&#99;anonical\" href=\"/old\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(CC), "{head}");
    }
    for head in ["<link rel=\"icon&nbsp;canonical\" href=\"/k\">", "<link rel=\"canonical&#44;x\" href=\"/k\">", "<link rel=\"canonica&#108;l\" href=\"/k\">"] {
        assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{CC}")), "{head}");
    }
}

/// A meta is cut where its `name` or its `property` is of a key `page_head` says, in either order:
/// `name="twitter:description" property="og:description"` is a second og:description to a crawler,
/// and `name="x" property="og:title"` a second og:title. Left: one where neither is of a key said,
/// and one where the second of two `property` (or `name`) attributes would be, as the browser keeps
/// the first (src/head.rs replaces, FEEDBACK84.md Q-1320, found by the review of 1c77039; branch
/// feedback82-85).
#[test]
fn a_meta_is_cut_where_its_name_or_its_property_is_of_a_key_said() {
    let h = said_head(&[("description", "D")]);
    for head in [
        "<meta name=\"twitter:description\" property=\"og:description\" content=\"OLD\">",
        "<meta property=\"og:description\" name=\"twitter:description\" content=\"OLD\">",
        "<meta name=\"description\" property=\"og:image\" content=\"OLD\">",
        "<meta name=\"x\" property=\"og&#58;description\" content=\"OLD\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(DD), "{head}");
    }
    for head in [
        "<meta name=\"twitter:description\" property=\"og:title\" content=\"K\">",
        "<meta property=\"x\" property=\"og:description\" content=\"K\">",
        "<meta name=\"x\" name=\"description\" content=\"K\">",
    ] {
        assert_eq!(with_head(&doc(head), &h), doc(&format!("{head}{DD}")), "{head}");
    }
    let t = said_head(&[("title", "T")]);
    let head = "<meta name=\"twitter:title\" property=\"og:title\" content=\"OLD\"><title>Old</title>";
    assert_eq!(with_head(&doc(head), &t), doc(TT));
}

/// A MathML `annotation-xml` in a head's template whose `encoding` is `text/html` or
/// `application/xhtml+xml` once its references are decoded is where the page's own elements are
/// read, so a `<style>` in it holds `</template><title>in</title>` as its text, and only the head's
/// own title is cut; with an encoding that is not (`text/xml`) the style is MathML's, `</template>`
/// closes the template, and the title after it is the head's too, cut (src/head.rs foreign,
/// FEEDBACK84.md Q-1320, found by the review of 1c77039; branch feedback82-85).
#[test]
fn an_annotation_xmls_encoding_is_read_with_its_character_references_decoded() {
    let h = said_head(&[("title", "T")]);
    let template = |enc: &str| format!("<template><math><annotation-xml encoding=\"{enc}\"><style></template><title>in</title></style></annotation-xml></math></template>");
    for enc in ["text&sol;html", "TEXT&#47;HTML", "application&#x2F;xhtml&plus;xml"] {
        let t = template(enc);
        assert_eq!(with_head(&doc(&format!("{t}<title>Old</title>")), &h), doc(&format!("{t}{TT}")), "{enc}");
    }
    let t = template("text&sol;xml");
    assert_eq!(
        with_head(&doc(&format!("{t}<title>Old</title>")), &h),
        doc(&format!("<template><math><annotation-xml encoding=\"text&sol;xml\"><style></template></style></annotation-xml></math></template>{TT}"))
    );
}
