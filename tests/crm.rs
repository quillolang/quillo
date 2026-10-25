//! The CRM in apps/crm, run as a program: signing a workspace up, rules
//! refusing on the server what the browser hinted at, a purchase running
//! its flow, and waiting flows carried on by the job.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::{Path, PathBuf};

fn copy_app(to: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("apps/crm");
    copy_dirs(&from, to);
    // As shipped: no plugins from the studio yet.
    std::fs::write(to.join("plugins/studio/index.qwe"), "let studio_plugins = []\nlet studio_renderers = {}\nlet studio_runners = {}\nlet studio_built = 0\n").unwrap();
    std::fs::write(to.join("plugins/studio/registry.json"), "{\"plugins\": []}").unwrap();
}

fn copy_dirs(from: &Path, to: &Path) {
    for dir in ["", "lib", "plugins", "plugins/studio", "ui", "migrations", "public"] {
        std::fs::create_dir_all(to.join(dir)).unwrap();
        for e in std::fs::read_dir(from.join(dir)).unwrap().flatten() {
            // The studio's own plugins are the installation's, not the app's.
            let name = e.file_name().to_string_lossy().into_owned();
            let studio_made = dir == "plugins/studio" && !["index.qwe", "registry.json", "template.txt"].contains(&name.as_str());
            if e.path().is_file() && !studio_made {
                std::fs::copy(e.path(), to.join(dir).join(e.file_name())).unwrap();
            }
        }
    }
}

struct Crm {
    it: Interp,
    dir: PathBuf,
}

impl Crm {
    fn new(name: &str) -> Crm {
        let dir = std::env::temp_dir().join(format!("quillo-crm-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        copy_app(&dir);
        let mut it = Interp::new(&dir);
        it.load_file(&dir.join("main.qwe")).unwrap_or_else(|e| panic!("{}", e));
        Crm { it, dir }
    }

    fn send(&mut self, host: &str, method: &str, url: &str, body: &str, cookie: Option<&str>) -> HttpResponse {
        let mut headers = vec![("Host".to_string(), host.to_string()), ("Content-Type".to_string(), "application/json".to_string())];
        if let Some(c) = cookie {
            headers.push(("Cookie".to_string(), c.to_string()));
        }
        self.it.handle(&HttpRequest { method: method.into(), url: url.into(), headers, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
    }

    /// A form posted as a browser posts it (no JavaScript).
    fn form(&mut self, host: &str, url: &str, body: &str, cookie: Option<&str>) -> HttpResponse {
        let mut headers = vec![("Host".to_string(), host.to_string()), ("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string())];
        if let Some(c) = cookie {
            headers.push(("Cookie".to_string(), c.to_string()));
        }
        self.it.handle(&HttpRequest { method: "POST".into(), url: url.into(), headers, body: body.as_bytes().to_vec(), remote: "127.0.0.1".into() })
    }

    /// A server fn's answer: `{"ok": ...}` or `{"err": ...}`.
    fn rpc(&mut self, host: &str, name: &str, args: serde_json::Value, cookie: Option<&str>) -> serde_json::Value {
        let r = self.send(host, "POST", &format!("/_quillo/rpc/{}", name), &args.to_string(), cookie);
        serde_json::from_slice(&r.body).unwrap_or_else(|e| panic!("{} {}: {}", name, e, r.text()))
    }

    fn call(&mut self, name: &str) {
        let f = quillo::value::lookup(&self.it.globals, name).unwrap();
        self.it.call(&f, vec![], 0).unwrap_or_else(|_| panic!("{} failed", name));
    }

    /// Loads the program again from its files (what --watch would do).
    fn reload(&mut self) {
        let mut it = Interp::new(&self.dir);
        it.load_file(&self.dir.join("main.qwe")).unwrap_or_else(|e| panic!("{}", e));
        self.it = it;
    }

    fn sql(&self, tenant: &str, sql: &str) {
        let c = rusqlite::Connection::open(self.dir.join(format!("data/{}.db", tenant))).unwrap();
        c.execute_batch(sql).unwrap();
    }
}

impl Drop for Crm {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Signs a merchant workspace up; the owner's session cookie.
fn signup(crm: &mut Crm) -> String {
    let w = serde_json::json!([{ "name": "Acme", "slug": "acme", "industry": "merchant", "owner_name": "Ada", "email": "ada@acme.test", "password": "a-long-password" }]);
    let r = crm.rpc("localhost:9000", "create_workspace", w, None);
    let url = r["ok"]["url"].as_str().unwrap_or_else(|| panic!("{}", r)).to_string();
    assert!(url.starts_with("http://acme.localhost:9000/welcome/"), "{}", url);
    let path = &url["http://acme.localhost:9000".len()..];
    let r = crm.send("acme.localhost:9000", "GET", path, "", None);
    assert_eq!((r.status, r.header("location")), (303, Some("/app")));
    let cookie = r.header("set-cookie").unwrap().split(';').next().unwrap().to_string();
    // The link works once.
    assert_eq!(crm.send("acme.localhost:9000", "GET", path, "", None).header("location"), Some("/login?problem=expired"));
    cookie
}

#[test]
fn a_workspace_signs_up_and_its_rules_hold_on_the_server() {
    let mut crm = Crm::new("rules");
    // The signup's own rules, on the server.
    let bad = crm.rpc("localhost:9000", "create_workspace", serde_json::json!([{ "slug": "x", "industry": "cars", "email": "no", "password": "1" }]), None);
    let fields: Vec<&str> = bad["err"]["problems"].as_array().unwrap().iter().map(|p| p["field"].as_str().unwrap()).collect();
    assert_eq!(fields, ["name", "slug", "industry", "owner_name", "email", "password"]);
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    // A workspace that does not exist is a 404, and makes nothing.
    assert_eq!(crm.send("nobody.localhost:9000", "GET", "/login", "", None).status, 404);
    assert!(!crm.dir.join("data/nobody.db").exists());
    // Not signed in: no data.
    assert!(crm.rpc(host, "list_contacts", serde_json::json!([{}]), None)["err"].is_string());

    // The loyalty plugin (on for merchants) refuses negative points; core
    // refuses a bad email. The browser hinted the same; here they refuse.
    let c = serde_json::json!([{ "first_name": "Linus", "email": "linus@", "fields": { "points": "-5" }, "stage": "lead", "tags": [] }]);
    let r = crm.rpc(host, "save_contact", c, Some(&cookie));
    let msgs: Vec<&str> = r["err"]["problems"].as_array().unwrap_or_else(|| panic!("{}", r)).iter().map(|p| p["message"].as_str().unwrap()).collect();
    assert_eq!(msgs, ["Not an email address", "Points are a whole number, zero or more"]);
    let ok = crm.rpc(host, "save_contact", serde_json::json!([{ "first_name": "Linus", "email": "linus@kernel.test", "fields": { "points": "40" }, "stage": "lead", "tags": ["vip"] }]), Some(&cookie));
    assert_eq!(ok["ok"]["tags"], serde_json::json!(["vip"]));
    // The same email twice: a check only the server can make.
    let dup = crm.rpc(host, "save_contact", serde_json::json!([{ "first_name": "Other", "email": "LINUS@kernel.test", "stage": "lead", "tags": [] }]), Some(&cookie));
    assert_eq!(dup["err"]["problems"][0]["message"], "Another contact has this email");
    // Switching the plugin off lifts its rule.
    crm.rpc(host, "set_plugins", serde_json::json!([["vat", "gdpr"]]), Some(&cookie));
    let r = crm.rpc(host, "save_contact", serde_json::json!([{ "first_name": "Neg", "email": "neg@x.test", "fields": { "points": "-5" }, "stage": "lead", "tags": [] }]), Some(&cookie));
    assert!(r["ok"].is_object(), "{}", r);
}

#[test]
fn a_purchase_runs_its_flow_and_waiting_flows_carry_on() {
    let mut crm = Crm::new("flows");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    // The checkout's rules on the server: a card that fails Luhn's check.
    let order = |card: &str| serde_json::json!([{ "sku": "tee", "name": "Grace Hopper", "email": "grace@navy.test", "qty": 5, "card": card, "expiry": "12/30", "cvc": "123", "country": "US", "address": "1 Navy Yard, Arlington" }]);
    let bad = crm.rpc(host, "checkout", order("4242 4242 4242 4241"), None);
    assert_eq!(bad["err"]["problems"][0]["field"], "card");
    assert_eq!(crm.rpc(host, "checkout", order("4000 0000 0000 0002"), None)["err"]["problems"][0]["message"], "The card was declined");
    let paid = crm.rpc(host, "checkout", order("4242 4242 4242 4242"), None);
    assert_eq!(paid["ok"]["total"], "$125.00", "{}", paid);
    // The "After a purchase" flow ran: an email, a tag, and (over 100) a task.
    let contacts = crm.rpc(host, "list_contacts", serde_json::json!([{}]), Some(&cookie));
    let grace = &contacts["ok"][0];
    assert_eq!((grace["stage"].as_str(), grace["tags"].clone()), (Some("customer"), serde_json::json!(["customer"])));
    let mail = crm.rpc(host, "list_outbox", serde_json::json!([]), Some(&cookie));
    let subjects: Vec<&str> = mail["ok"].as_array().unwrap().iter().map(|m| m["subject"].as_str().unwrap()).collect();
    assert!(subjects.contains(&"Thank you, Grace!") && subjects.iter().any(|s| s.starts_with("Your order")), "{:?}", subjects);
    let tasks = crm.rpc(host, "list_tasks", serde_json::json!([false]), Some(&cookie));
    assert_eq!(tasks["ok"][0]["title"], "Send Grace a thank-you gift");

    // The welcome flow: a sign-up, then it waits two days.
    let signed = crm.rpc(host, "submit_form", serde_json::json!(["newsletter", { "email": "lois@planet.test", "first_name": "Lois", "consent": true }]), None);
    assert!(signed["ok"].is_object(), "{}", signed);
    let id = grace["id"].as_i64().unwrap();
    let lois = crm.rpc(host, "list_contacts", serde_json::json!([{ "q": "lois" }]), Some(&cookie))["ok"][0]["id"].as_i64().unwrap();
    let runs = |crm: &mut Crm| crm.rpc(host, "get_contact", serde_json::json!([lois]), Some(&cookie))["ok"]["runs"].clone();
    assert_eq!(runs(&mut crm)[0]["status"], "waiting");
    // Two days on: the job offers the product, then waits for the purchase.
    crm.sql("acme", "update flow_runs set next_at = 0 where status = 'waiting'");
    crm.call("tick");
    assert_eq!(runs(&mut crm)[0]["status"], "waiting");
    let offers = crm.rpc(host, "list_outbox", serde_json::json!([]), Some(&cookie));
    assert!(offers["ok"].as_array().unwrap().iter().any(|m| m["subject"] == "Something for you"), "{}", offers);
    // Lois buys: the flow leaves by "yes" and tags her a buyer.
    let buy = serde_json::json!([{ "sku": "tee", "name": "Lois Lane", "email": "lois@planet.test", "qty": 1, "card": "4242424242424242", "expiry": "12/30", "cvc": "123", "country": "US", "address": "1 Planet Plaza, Metropolis" }]);
    assert!(crm.rpc(host, "checkout", buy, None)["ok"].is_object());
    let c = crm.rpc(host, "get_contact", serde_json::json!([lois]), Some(&cookie))["ok"].clone();
    assert!(c["tags"].as_array().unwrap().contains(&serde_json::json!("buyer")), "{}", c["tags"]);
    assert_eq!(c["first_name"], "Lois");
    let _ = id;
}

#[test]
fn pages_are_made_of_plugin_blocks_and_publish_only_when_the_rules_pass() {
    let mut crm = Crm::new("pages");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    let id = crm.rpc(host, "new_page", serde_json::json!(["launch"]), Some(&cookie))["ok"].as_i64().unwrap();
    let page = crm.rpc(host, "get_page", serde_json::json!([id]), Some(&cookie))["ok"].clone();
    assert_eq!(page["slug"], "new-arrivals");
    // A draft with a bad link and a privacy note (the Consent plugin's block) with no
    // address saves, but does not publish: the same rules the designer hints with.
    let blocks = serde_json::json!([
        { "id": "b1", "type": "hero", "config": { "headline": "Hi", "button": "Go", "link": "javascript:alert(1)" } },
        { "id": "b2", "type": "gdpr.privacy", "config": {} },
    ]);
    let draft = serde_json::json!([{ "id": id, "title": "New arrivals", "slug": "new-arrivals", "description": "", "blocks": blocks }]);
    assert!(crm.rpc(host, "save_page", draft, Some(&cookie))["ok"].is_object());
    let refused = crm.rpc(host, "publish_page", serde_json::json!([id]), Some(&cookie));
    let msgs: Vec<&str> = refused["err"]["problems"].as_array().unwrap_or_else(|| panic!("{}", refused)).iter().map(|p| p["message"].as_str().unwrap()).collect();
    assert!(msgs.contains(&"Hero: Button link must start with /, https:// or mailto:"), "{:?}", msgs);
    assert!(msgs.iter().any(|m| m.starts_with("Privacy note:")), "{:?}", msgs);
    assert_eq!(crm.send(host, "GET", "/p/new-arrivals", "", None).status, 404);
    // Fixed, it publishes; visitors see it, and the home page is it.
    let fixed = serde_json::json!([
        { "id": "b1", "type": "hero", "config": { "headline": "Organic tees", "button": "Shop", "link": "/shop" } },
        { "id": "b2", "type": "gdpr.privacy", "config": { "contact": "privacy@acme.test" } },
        { "id": "b3", "type": "product", "config": { "product": "tee" } },
    ]);
    crm.rpc(host, "save_page", serde_json::json!([{ "id": id, "title": "New arrivals", "slug": "new-arrivals", "description": "Tees", "blocks": fixed }]), Some(&cookie));
    assert!(crm.rpc(host, "publish_page", serde_json::json!([id]), Some(&cookie))["ok"].is_object());
    let html = crm.send(host, "GET", "/p/new-arrivals", "", None).text();
    assert!(html.contains("<h1>Organic tees</h1>") && html.contains("privacy@acme.test") && html.contains("href=\"/shop/tee\""), "{}", html);
    assert!(html.contains("<title>New arrivals · Acme</title>"));
    assert_eq!(crm.send(host, "GET", "/", "", None).status, 303);
    crm.rpc(host, "set_home", serde_json::json!([id]), Some(&cookie));
    assert!(crm.send(host, "GET", "/", "", None).text().contains("<h1>Organic tees</h1>"));
    // A theme's values go into a style sheet: only a colour is a colour.
    let bad = crm.rpc(host, "save_theme", serde_json::json!([{ "color": "red;}body{display:none", "font": "serif" }]), Some(&cookie));
    assert_eq!(bad["err"]["problems"][0]["field"], "color");
    assert!(crm.rpc(host, "save_theme", serde_json::json!([{ "color": "#B91C1C", "font": "serif", "dark": true }]), Some(&cookie))["ok"].is_object());
    let themed = crm.send(host, "GET", "/p/new-arrivals", "", None).text();
    assert!(themed.contains("--brand: #b91c1c") && themed.contains("class=\"site theme-clean head-bar dark\""), "{}", themed);
    // Switching the Consent plugin off takes its block with it.
    crm.rpc(host, "set_plugins", serde_json::json!([["loyalty"]]), Some(&cookie));
    assert!(!crm.send(host, "GET", "/p/new-arrivals", "", None).text().contains("privacy@acme.test"));
}

#[test]
fn sections_are_made_once_and_shown_on_every_page_that_puts_them_in() {
    let mut crm = Crm::new("sections");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    let rpc = |crm: &mut Crm, name: &str, args: serde_json::Value| crm.rpc(host, name, args, Some(&cookie));
    let sec = rpc(&mut crm, "new_page", serde_json::json!(["section"]))["ok"].as_i64().unwrap();
    let page = rpc(&mut crm, "new_page", serde_json::json!(["blank"]))["ok"].as_i64().unwrap();
    let s = rpc(&mut crm, "get_page", serde_json::json!([sec]))["ok"].clone();
    assert_eq!(s["kind"], "section");
    let text = |body: &str| serde_json::json!([{ "id": "b1", "type": "text", "config": { "body": body } }]);
    let put_in = |id: i64| serde_json::json!([
        { "id": "b1", "type": "hero", "config": { "headline": "Hello" } },
        { "id": "b2", "type": "section", "config": { "section": id.to_string() } },
    ]);
    rpc(&mut crm, "save_page", serde_json::json!([{ "id": sec, "title": "Pitch", "slug": s["slug"], "blocks": text("Join the **club**") }]));
    rpc(&mut crm, "save_page", serde_json::json!([{ "id": page, "title": "Home", "slug": "home", "blocks": put_in(sec) }]));
    // Not published yet: the page cannot show it.
    let refused = rpc(&mut crm, "publish_page", serde_json::json!([page]));
    assert!(refused["err"]["problems"].to_string().contains("publish “Pitch” first"), "{}", refused);
    assert!(rpc(&mut crm, "publish_page", serde_json::json!([sec]))["ok"].is_object());
    assert!(rpc(&mut crm, "publish_page", serde_json::json!([page]))["ok"].is_object());
    assert!(crm.send(host, "GET", "/p/home", "", None).text().contains("Join the <strong>club</strong>"));
    // A section has no address of its own, and cannot be the home page.
    assert_eq!(crm.send(host, "GET", &format!("/p/{}", s["slug"].as_str().unwrap()), "", None).status, 404);
    rpc(&mut crm, "set_home", serde_json::json!([sec]));
    assert_eq!(crm.send(host, "GET", "/", "", None).status, 303);
    // Changing the section changes the page.
    rpc(&mut crm, "save_page", serde_json::json!([{ "id": sec, "title": "Pitch", "slug": s["slug"], "blocks": text("Now **better**") }]));
    rpc(&mut crm, "publish_page", serde_json::json!([sec]));
    assert!(crm.send(host, "GET", "/p/home", "", None).text().contains("Now <strong>better</strong>"));
    let s = rpc(&mut crm, "get_page", serde_json::json!([sec]))["ok"].clone();
    assert_eq!(s["used_on"][0]["title"], "Home");
    // Loops are refused: a section in itself, or in a section it is in.
    let inner = rpc(&mut crm, "new_page", serde_json::json!(["section"]))["ok"].as_i64().unwrap();
    let inner_slug = rpc(&mut crm, "get_page", serde_json::json!([inner]))["ok"]["slug"].clone();
    rpc(&mut crm, "save_page", serde_json::json!([{ "id": inner, "title": "Inner", "slug": inner_slug, "blocks": put_in(sec) }]));
    assert!(rpc(&mut crm, "publish_page", serde_json::json!([inner]))["ok"].is_object());
    rpc(&mut crm, "save_page", serde_json::json!([{ "id": sec, "title": "Pitch", "slug": s["slug"], "blocks": put_in(inner) }]));
    let looped = rpc(&mut crm, "publish_page", serde_json::json!([sec]));
    assert!(looped["err"]["problems"].to_string().contains("already has this section in it"), "{}", looped);
    rpc(&mut crm, "save_page", serde_json::json!([{ "id": sec, "title": "Pitch", "slug": s["slug"], "blocks": put_in(sec) }]));
    assert!(rpc(&mut crm, "publish_page", serde_json::json!([sec]))["err"]["problems"].to_string().contains("cannot be put in itself"));
    // A section in use is neither unpublished nor deleted.
    assert!(rpc(&mut crm, "delete_page", serde_json::json!([sec]))["err"].to_string().contains("Used on"));
    assert!(rpc(&mut crm, "unpublish_page", serde_json::json!([sec]))["err"].to_string().contains("“Home”"));
    assert!(rpc(&mut crm, "delete_page", serde_json::json!([page]))["ok"].as_bool().unwrap());
    assert!(rpc(&mut crm, "delete_page", serde_json::json!([inner]))["ok"].as_bool().unwrap());
    assert!(rpc(&mut crm, "delete_page", serde_json::json!([sec]))["ok"].as_bool().unwrap());
}

#[test]
fn images_upload_crop_and_serve_at_their_sizes() {
    let mut crm = Crm::new("media");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    let png = {
        let img = image::RgbImage::from_fn(800, 600, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    let upload = |crm: &mut Crm, name: &str, bytes: &[u8]| {
        let mut body = b"--B\r\nContent-Disposition: form-data; name=\"args\"\r\n\r\n[{\"$quillo_file\": 0}]\r\n--B\r\n".to_vec();
        body.extend_from_slice(format!("Content-Disposition: form-data; name=\"file0\"; filename=\"{}\"\r\nContent-Type: image/png\r\n\r\n", name).as_bytes());
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n--B--\r\n");
        let headers = vec![
            ("Host".to_string(), host.to_string()),
            ("Content-Type".to_string(), "multipart/form-data; boundary=B".to_string()),
            ("X-Quillo-Rpc".to_string(), "1".to_string()),
            ("Cookie".to_string(), cookie.clone()),
        ];
        let r = crm.it.handle(&HttpRequest { method: "POST".into(), url: "/_quillo/rpc/upload_image".into(), headers, body, remote: "127.0.0.1".into() });
        serde_json::from_slice::<serde_json::Value>(&r.body).unwrap()
    };
    let m = upload(&mut crm, "pattern.png", &png)["ok"].clone();
    assert_eq!((m["width"].as_i64(), m["height"].as_i64(), m["format"].as_str()), (Some(800), Some(600), Some("png")), "{}", m);
    // Not an image: refused, and nothing kept.
    assert!(upload(&mut crm, "fake.png", b"hello")["err"].as_str().unwrap().contains("not a JPEG"));
    assert_eq!(std::fs::read_dir(crm.dir.join("data/media/acme/originals")).unwrap().count(), 1);
    // A crop, at a size; sizes and crops that are not allowed are not made.
    let key = m["key"].as_str().unwrap().to_string();
    let r = crm.send(host, "GET", &format!("/media/{}/100,50,400,400/320", key), "", None);
    assert_eq!((r.status, r.header("content-type"), r.header("cache-control")), (200, Some("image/jpeg"), Some("public, max-age=31536000, immutable")));
    let got = image::load_from_memory(&r.body).unwrap();
    assert_eq!((got.width(), got.height()), (320, 320));
    assert_eq!(crm.send(host, "GET", &format!("/media/{}/full/333", key), "", None).status, 404);
    assert_eq!(crm.send(host, "GET", &format!("/media/{}/600,0,400,400/320", key), "", None).status, 404);
    assert_eq!(crm.send(host, "GET", "/media/nosuchkey/full/320", "", None).status, 404);
    // Deleting takes the file and its sizes.
    crm.rpc(host, "delete_media", serde_json::json!([m["id"]]), Some(&cookie));
    assert_eq!(crm.send(host, "GET", &format!("/media/{}/full/320", key), "", None).status, 404);
    assert!(!crm.dir.join(format!("data/media/acme/cache/{}", key)).exists());
}

#[test]
fn developers_write_plugins_try_them_and_release_them() {
    std::env::set_var("CRM_DEVELOPERS", "ada@acme.test");
    let mut crm = Crm::new("studio");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    let id = crm.rpc(host, "studio_create", serde_json::json!(["events", "Events"]), Some(&cookie));
    assert_eq!(id["ok"], "events", "{}", id);
    let src = crm.rpc(host, "studio_get", serde_json::json!(["events"]), Some(&cookie))["ok"]["source"].as_str().unwrap().to_string();
    assert!(src.contains("let events_plugin = {") && src.contains("\"events.callout\""));
    // Names that could clash with other plugins, and code that does not check, are refused.
    let r = crm.rpc(host, "studio_save", serde_json::json!(["events", format!("{}\nlet total = 1\n", src)]), Some(&cookie));
    assert!(r["err"]["problems"][0]["message"].as_str().unwrap().contains("\"total\" must start with events_"), "{}", r);
    let broken = src.replace("label: \"Callout\"", "label: nope_undefined");
    let r = crm.rpc(host, "studio_save", serde_json::json!(["events", broken]), Some(&cookie));
    assert_eq!(r["err"]["problems"][0]["message"], "unknown name `nope_undefined`", "{}", r);
    assert!(r["err"]["problems"][0]["line"].as_i64().unwrap() > 0);
    assert!(!crm.dir.join("plugins/studio/events.draft.qwe").exists(), "nothing is written until it checks");
    // While typing: the same checks, nothing written.
    let r = crm.rpc(host, "studio_check", serde_json::json!(["events", broken]), Some(&cookie));
    assert_eq!(r["ok"][0]["message"], "unknown name `nope_undefined`", "{}", r);
    assert_eq!(crm.rpc(host, "studio_check", serde_json::json!(["events", src]), Some(&cookie))["ok"], serde_json::json!([]));
    assert!(!crm.dir.join("plugins/studio/events.draft.qwe").exists());
    // A good draft: tried in this workspace, its block renders; the live
    // preview shows it once the program has loaded it.
    let saved = crm.rpc(host, "studio_save", serde_json::json!(["events", src]), Some(&cookie));
    assert_eq!(saved["ok"]["saved"], true);
    crm.reload();
    assert!(crm.rpc(host, "studio_built_at", serde_json::json!([]), Some(&cookie))["ok"].as_i64().unwrap() >= saved["ok"]["written"].as_i64().unwrap());
    let preview = crm.send(host, "GET", "/app/studio/events/preview", "", Some(&cookie)).text();
    assert!(preview.contains("events.callout") && preview.contains("<b>Title</b>"), "{}", preview);
    assert_eq!(crm.send(host, "GET", "/app/studio/events/preview", "", None).status, 403);
    crm.rpc(host, "set_try_drafts", serde_json::json!([true]), Some(&cookie));
    let block = serde_json::json!([[{ "id": "b1", "type": "events.callout", "config": { "title": "Open day", "tone": "success" } }]]);
    let html = crm.rpc(host, "preview_page", block.clone(), Some(&cookie))["ok"][0]["html"].as_str().unwrap().to_string();
    assert!(html.contains("<b>Open day</b>") && html.contains("#ecfdf3"), "{}", html);
    // Released: workspaces switch it on as any plugin.
    assert_eq!(crm.rpc(host, "studio_release", serde_json::json!(["events"]), Some(&cookie))["ok"]["released"], 1);
    crm.reload();
    crm.rpc(host, "set_try_drafts", serde_json::json!([false]), Some(&cookie));
    let off = crm.rpc(host, "preview_page", block.clone(), Some(&cookie))["ok"][0]["html"].as_str().unwrap().to_string();
    assert!(off.contains("switched off"), "{}", off);
    crm.rpc(host, "set_plugins", serde_json::json!([["events"]]), Some(&cookie));
    let on = crm.rpc(host, "preview_page", block, Some(&cookie))["ok"][0]["html"].as_str().unwrap().to_string();
    assert!(on.contains("<b>Open day</b>"), "{}", on);
    // Not a developer: no studio.
    std::env::remove_var("CRM_DEVELOPERS");
    let mut other = Crm::new("studio-closed");
    let c2 = signup(&mut other);
    assert!(other.rpc(host, "studio_list", serde_json::json!([]), Some(&c2))["err"].as_str().unwrap().contains("developers"));
}

#[test]
fn a_blog_with_members_only_posts_a_feed_and_latest_posts_on_pages() {
    let mut crm = Crm::new("blog");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    // Off by default for a merchant; on, /blog answers.
    assert_eq!(crm.send(host, "GET", "/blog", "", None).status, 404);
    crm.rpc(host, "set_plugins", serde_json::json!([["blog", "paywall"]]), Some(&cookie));
    let id = crm.rpc(host, "new_post", serde_json::json!([]), Some(&cookie))["ok"].as_i64().unwrap();
    // The post rules refuse to publish an empty post.
    let r = crm.rpc(host, "publish_post", serde_json::json!([id]), Some(&cookie));
    let fields: Vec<&str> = r["err"]["problems"].as_array().unwrap().iter().map(|p| p["field"].as_str().unwrap()).collect();
    assert_eq!(fields, ["title", "body"]);
    let post = serde_json::json!([{ "id": id, "slug": "hello", "title": "Hello <world>", "excerpt": "First words.", "body": "## Inside\n\nFor members.",
        "tags": ["News", "first post"], "members_only": true }]);
    assert!(crm.rpc(host, "save_post", post, Some(&cookie))["ok"].is_object());
    assert_eq!(crm.rpc(host, "publish_post", serde_json::json!([id]), Some(&cookie))["ok"]["status"], "published");
    // Listed, tagged, and locked for a visitor.
    let index = crm.send(host, "GET", "/blog", "", None).text();
    assert!(index.contains("Hello &lt;world&gt;") && index.contains("href=\"/blog/hello\""), "{}", index);
    assert!(crm.send(host, "GET", "/blog?tag=first-post", "", None).text().contains("Hello &lt;world&gt;"));
    assert!(!crm.send(host, "GET", "/blog?tag=other", "", None).text().contains("Hello &lt;world&gt;"));
    let locked = crm.send(host, "GET", "/blog/hello", "", None).text();
    assert!(locked.contains("class=\"paywall\"") && !locked.contains("<h2>Inside</h2>"), "{}", locked);
    // A member signs in with the link sent to them, and reads it.
    crm.rpc(host, "save_contact", serde_json::json!([{ "first_name": "Lois", "email": "lois@planet.test", "stage": "customer", "tags": ["member"] }]), Some(&cookie));
    let r = crm.send(host, "POST", "/blog/signin", "", None);
    assert_eq!(r.status, 400, "the form's fields are needed");
    let form = |email: &str| format!("email={}&back=%2Fblog%2Fhello", email);
    let mut req = |crm: &mut Crm, body: String| {
        let headers = vec![("Host".to_string(), host.to_string()), ("Content-Type".to_string(), "application/x-www-form-urlencoded".to_string())];
        crm.it.handle(&HttpRequest { method: "POST".into(), url: "/blog/signin".into(), headers, body: body.into_bytes(), remote: "127.0.0.1".into() })
    };
    assert!(req(&mut crm, form("lois@planet.test")).text().contains("Check your email"));
    assert!(req(&mut crm, form("stranger@x.test")).text().contains("Check your email"), "the same answer for anyone");
    let c = rusqlite::Connection::open(crm.dir.join("data/acme.db")).unwrap();
    let token: String = c.query_row("select token from reader_links", [], |r| r.get(0)).unwrap();
    let r = crm.send(host, "GET", &format!("/blog/signin/{}?back=/blog/hello", token), "", None);
    assert_eq!(r.header("location"), Some("/blog/hello"));
    let reader = r.header("set-cookie").unwrap().split(';').next().unwrap().to_string();
    let open = crm.send(host, "GET", "/blog/hello", "", Some(&reader)).text();
    assert!(open.contains("<h2>Inside</h2>") && !open.contains("class=\"paywall\""), "{}", open);
    // Links back only to the blog.
    let r = crm.send(host, "GET", &format!("/blog/signin/{}?back=https://evil.test", token), "", None);
    assert_eq!(r.header("location"), Some("/blog"));
    // The feed, and the latest posts on a page.
    let feed = crm.send(host, "GET", "/blog/feed.xml", "", None);
    assert_eq!(feed.header("content-type"), Some("application/rss+xml; charset=utf-8"));
    assert!(feed.text().contains("<title>Hello &lt;world&gt;</title>"), "{}", feed.text());
    let block = serde_json::json!([[{ "id": "b1", "type": "blog.latest", "config": { "heading": "Latest", "count": "3" } }]]);
    let html = crm.rpc(host, "preview_page", block, Some(&cookie))["ok"][0]["html"].as_str().unwrap().to_string();
    assert!(html.contains("Hello &lt;world&gt;") && html.contains("<h2>Latest</h2>"), "{}", html);
}

#[test]
fn a_shop_with_categories_a_cart_discount_codes_and_shipping() {
    let mut crm = Crm::new("shop");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    let mug = serde_json::json!([{ "name": "Clay mug", "sku": "mug", "price_cents": 1800, "kind": "one_time", "interval": "", "description": "Hand **thrown**.",
        "stock": 12, "active": true, "images": [], "category": "Home Goods", "compare_at_cents": 2400, "featured": true, "digital": false }]);
    let saved = crm.rpc(host, "save_product", mug, Some(&cookie));
    assert_eq!(saved["ok"]["category"], "home-goods", "{}", saved);
    // A price before that is not more than the price is refused.
    let bad = serde_json::json!([{ "name": "X", "sku": "x", "price_cents": 1000, "compare_at_cents": 900, "kind": "one_time", "active": true, "images": [] }]);
    let refused = crm.rpc(host, "save_product", bad, Some(&cookie));
    assert_eq!(refused["err"]["problems"][0]["field"], "compare_at", "{}", refused);
    let code = |c: &str, kind: &str, value: i64| serde_json::json!([{ "code": c, "kind": kind, "value": value, "min_cents": 2000, "max_uses": 1, "active": true }]);
    assert_eq!(crm.rpc(host, "save_discount", code("10", "percent", 10), Some(&cookie))["err"]["problems"][0]["field"], "code");
    assert_eq!(crm.rpc(host, "save_discount", code("WELCOME10", "percent", 10), Some(&cookie))["ok"], true);
    assert_eq!(crm.rpc(host, "save_shipping", serde_json::json!([500, 6000]), Some(&cookie))["ok"], true);

    // The catalogue: by category, searched; a product's own page.
    let shop = crm.send(host, "GET", "/shop?category=home-goods", "", None).text();
    assert!(shop.contains("Clay mug") && !shop.contains("Organic cotton tee") && shop.contains("<s>$24.00</s>"), "{}", shop);
    assert!(crm.send(host, "GET", "/shop?q=organic", "", None).text().contains("Organic cotton tee"));
    let page = crm.send(host, "GET", "/shop/mug", "", None).text();
    assert!(page.contains("Hand <strong>thrown</strong>.") && page.contains("action=\"/cart/add\""), "{}", page);
    assert!(crm.send(host, "GET", "/sitemap.xml", "", None).text().contains("/shop/mug</loc>"));

    // A visitor's cart, in their session (a cookie, which each change sends anew).
    let r = crm.form(host, "/cart/add", "sku=mug&qty=1&then=cart", None);
    assert_eq!(r.header("location"), Some("/cart"));
    let mut visitor = r.header("set-cookie").unwrap().split(';').next().unwrap().to_string();
    let mut post = |crm: &mut Crm, url: &str, body: &str| {
        let r = crm.form(host, url, body, Some(&visitor));
        if let Some(c) = r.header("set-cookie") {
            visitor = c.split(';').next().unwrap().to_string();
        }
        visitor.clone()
    };
    post(&mut crm, "/cart/discount", "code=welcome10");
    // Under the code's minimum: no discount, and shipping.
    let v = post(&mut crm, "/cart/update", "sku=mug&qty=1");
    let cart = crm.send(host, "GET", "/cart", "", Some(&v)).text();
    assert!(cart.contains("WELCOME10 needs $20.00 in the cart") && cart.contains("$23.00"), "{}", cart);
    let visitor = post(&mut crm, "/cart/add", "sku=tee&qty=2");
    let ctx = crm.rpc(host, "checkout_context", serde_json::json!([]), Some(&visitor));
    assert_eq!(ctx["ok"]["lines"].as_array().unwrap().len(), 2, "{}", ctx);
    assert_eq!(ctx["ok"]["discount"]["code"], "WELCOME10");
    // Something to ship needs an address; then it pays: 2 × $25 + $18, less 10%, shipped free over $60.
    let order = |addr: &str| serde_json::json!([{ "name": "Grace Hopper", "email": "grace@navy.test", "card": "4242424242424242", "expiry": "12/30", "cvc": "123", "country": "US", "address": addr }]);
    assert_eq!(crm.rpc(host, "checkout", order(""), Some(&visitor))["err"]["problems"][0]["field"], "address");
    let r = crm.send(host, "POST", "/_quillo/rpc/checkout", &order("1 Navy Yard, Arlington").to_string(), Some(&visitor));
    let paid: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(paid["ok"]["total"], "$61.20", "{}", paid);
    let visitor = r.header("set-cookie").map(|c| c.split(';').next().unwrap().to_string()).unwrap_or(visitor);
    assert!(crm.send(host, "GET", "/cart", "", Some(&visitor)).text().contains("Your cart is empty"));
    let products = crm.rpc(host, "list_products", serde_json::json!([]), Some(&cookie))["ok"].clone();
    let stock = |sku: &str| products.as_array().unwrap().iter().find(|p| p["sku"] == sku).unwrap()["stock"].clone();
    assert_eq!(stock("mug"), 11);
    let codes = crm.rpc(host, "list_discounts", serde_json::json!([]), Some(&cookie));
    assert_eq!(codes["ok"][0]["uses"], 1);
    // Used up: the next cart pays in full.
    let r = crm.form(host, "/cart/discount", "code=WELCOME10", Some(&visitor));
    let visitor = r.header("set-cookie").map(|c| c.split(';').next().unwrap().to_string()).unwrap_or(visitor);
    let r = crm.form(host, "/cart/add", "sku=tee&qty=3", Some(&visitor));
    let visitor = r.header("set-cookie").map(|c| c.split(';').next().unwrap().to_string()).unwrap_or(visitor);
    assert!(crm.send(host, "GET", "/cart", "", Some(&visitor)).text().contains("That code has been used up"));
    // A refund puts what was sold back on the shelf.
    let orders = crm.rpc(host, "list_orders", serde_json::json!([""]), Some(&cookie));
    let id = orders["ok"][0]["id"].as_i64().unwrap();
    assert_eq!(orders["ok"][0]["discount_cents"], 680);
    crm.rpc(host, "refund_order", serde_json::json!([id]), Some(&cookie));
    let products = crm.rpc(host, "list_products", serde_json::json!([]), Some(&cookie))["ok"].clone();
    assert_eq!(products.as_array().unwrap().iter().find(|p| p["sku"] == "mug").unwrap()["stock"], 12);

    // Variants: each its own stock and price; the product page asks which.
    let cap = serde_json::json!([{ "name": "Cap", "sku": "cap", "price_cents": 2000, "kind": "one_time", "active": true, "images": [], "category": "",
        "variant_label": "Size", "variants": [{ "name": "S", "stock": 1, "price_cents": null }, { "name": "XL", "stock": 5, "price_cents": 2400 }, { "name": "M", "stock": 0, "price_cents": null }] }]);
    assert!(crm.rpc(host, "save_product", cap, Some(&cookie))["ok"].is_object());
    let dup = serde_json::json!([{ "name": "Cap", "sku": "cap2", "price_cents": 2000, "kind": "one_time", "active": true, "images": [], "variants": [{ "name": "S" }, { "name": "s" }] }]);
    assert_eq!(crm.rpc(host, "save_product", dup, Some(&cookie))["err"]["problems"][0]["message"], "Two variants have the same name");
    let page = crm.send(host, "GET", "/shop/cap", "", None).text();
    assert!(page.contains("name=\"variant\" value=\"M\" disabled") && page.contains("From $20.00"), "{}", page);
    let r = crm.form(host, "/cart/add", "sku=cap&qty=1", None);
    assert_eq!(r.header("location"), Some("/shop/cap?choose=1"));
    let r = crm.form(host, "/cart/add", "sku=cap&qty=2&variant=S", None);
    let shopper = r.header("set-cookie").unwrap().split(';').next().unwrap().to_string();
    let r = crm.form(host, "/cart/add", "sku=cap&qty=1&variant=XL", Some(&shopper));
    let shopper = r.header("set-cookie").map(|c| c.split(';').next().unwrap().to_string()).unwrap_or(shopper);
    let buy = order("2 Market St, Metropolis");
    let too_many = crm.rpc(host, "checkout", buy.clone(), Some(&shopper));
    assert_eq!(too_many["err"]["problems"][0]["message"], "Cap (S): only 1 left", "{}", too_many);
    let r = crm.form(host, "/cart/update", "sku=cap~S&qty=1", Some(&shopper));
    let shopper = r.header("set-cookie").map(|c| c.split(';').next().unwrap().to_string()).unwrap_or(shopper);
    let paid = crm.rpc(host, "checkout", buy, Some(&shopper));
    assert_eq!(paid["ok"]["total"], "$49.00", "S at $20, XL at $24, and $5 shipping: {}", paid);
    let cap = crm.rpc(host, "list_products", serde_json::json!([]), Some(&cookie))["ok"].as_array().unwrap().iter().find(|p| p["sku"] == "cap").unwrap().clone();
    assert_eq!((cap["variants"][0]["stock"].clone(), cap["variants"][1]["stock"].clone()), (serde_json::json!(0), serde_json::json!(4)));
    assert!(crm.send(host, "GET", "/shop/cap", "", None).text().contains("name=\"variant\" value=\"S\" disabled"));
}

fn signup_as(crm: &mut Crm, slug: &str, industry: &str) -> String {
    let w = serde_json::json!([{ "name": "The Planet", "slug": slug, "industry": industry, "owner_name": "Perry White", "email": "perry@planet.test", "password": "a-long-password" }]);
    let r = crm.rpc("localhost:9000", "create_workspace", w, None);
    let url = r["ok"]["url"].as_str().unwrap_or_else(|| panic!("{}", r)).to_string();
    let host = format!("{}.localhost:9000", slug);
    let r = crm.send(&host, "GET", &url[format!("http://{}", host).len()..], "", None);
    r.header("set-cookie").unwrap().split(';').next().unwrap().to_string()
}

#[test]
fn a_newsroom_with_themes_menus_categories_and_news_blocks() {
    let mut crm = Crm::new("newsroom");
    let cookie = signup_as(&mut crm, "planet", "news");
    let host = "planet.localhost:9000";
    let rpc = |crm: &mut Crm, name: &str, args: serde_json::Value| crm.rpc(host, name, args, Some(&cookie));
    // A news outlet starts in the Editorial theme: a masthead with the date.
    let blog = crm.send(host, "GET", "/blog", "", None).text();
    assert!(blog.contains("class=\"site theme-editorial head-masthead\"") && blog.contains("site-dateline"), "{}", blog);
    // The theme's rules: links a page may have, CSS that stays CSS.
    let theme = |menu: serde_json::Value, css: &str| serde_json::json!([{ "id": "editorial", "color": "#b91c1c", "font": "serif", "menu": menu, "css": css,
        "footer": "355 Plaza Avenue", "footer_links": [{ "label": "About", "link": "/p/about" }] }]);
    let bad = rpc(&mut crm, "save_theme", theme(serde_json::json!([{ "label": "X", "link": "javascript:alert(1)" }]), ""));
    assert_eq!(bad["err"]["problems"][0]["field"], "menu", "{}", bad);
    let bad = rpc(&mut crm, "save_theme", theme(serde_json::json!([]), "h1{}</style><script>"));
    assert_eq!(bad["err"]["problems"][0]["field"], "css");
    let ok = rpc(&mut crm, "save_theme", theme(serde_json::json!([{ "label": "Politics", "link": "/blog/category/politics" }]), "h1 { letter-spacing: -1px }"));
    assert!(ok["ok"].is_object(), "{}", ok);
    // Posts in categories, by an author with a line about them.
    rpc(&mut crm, "save_profile", serde_json::json!(["Perry White", "Editor in chief since 1938."]));
    let post = |crm: &mut Crm, title: &str, category: &str, featured: bool, body: &str| {
        let id = crm.rpc(host, "new_post", serde_json::json!([]), Some(&cookie))["ok"].as_i64().unwrap();
        let slug = title.to_lowercase().replace(' ', "-");
        crm.rpc(host, "save_post", serde_json::json!([{ "id": id, "slug": slug, "title": title, "excerpt": "", "body": body, "cover": null, "tags": [],
            "description": "", "members_only": false, "category": category, "featured": featured }]), Some(&cookie));
        let r = crm.rpc(host, "publish_post", serde_json::json!([id]), Some(&cookie));
        assert!(r["ok"].is_object(), "{}", r);
    };
    post(&mut crm, "Council votes", "Politics", true, "The council voted.");
    post(&mut crm, "Derby won", "sport", false, "In extra time.");
    post(&mut crm, "How we count votes", "tech", false, "```sql\nselect count(*) from votes;\n```");
    let cat = crm.send(host, "GET", "/blog/category/politics", "", None).text();
    assert!(cat.contains("Council votes") && !cat.contains("Derby won") && cat.contains("href=\"/blog/category/politics\""), "{}", cat);
    assert!(cat.contains("--site-width: 1180px") && cat.contains(".site { h1 { letter-spacing: -1px } }") && cat.contains("355 Plaza Avenue"), "{}", cat);
    let found = crm.send(host, "GET", "/blog?q=extra", "", None).text();
    assert!(found.contains("Derby won") && !found.contains("Council votes"), "{}", found);
    let author = crm.send(host, "GET", "/blog/author/1", "", None).text();
    assert!(author.contains("Editor in chief since 1938.") && author.contains("How we count votes"), "{}", author);
    let code = crm.send(host, "GET", "/blog/how-we-count-votes", "", None).text();
    assert!(code.contains("<span class=\"hl-k\">select</span>") && code.contains("og:type\" content=\"article"), "{}", code);
    // A front page of news blocks: the featured story leads.
    let id = rpc(&mut crm, "new_page", serde_json::json!(["blank"]))["ok"].as_i64().unwrap();
    let blocks = serde_json::json!([
        { "id": "b1", "type": "blog.breaking", "config": { "text": "Polls close at 8", "label": "Live" } },
        { "id": "b2", "type": "blog.top", "config": {} },
        { "id": "b3", "type": "blog.category", "config": { "category": "sport" } },
        { "id": "b4", "type": "blog.category", "config": { "category": "weather" } },
    ]);
    rpc(&mut crm, "save_page", serde_json::json!([{ "id": id, "title": "Front page", "slug": "front", "blocks": blocks }]));
    let page = rpc(&mut crm, "get_page", serde_json::json!([id]))["ok"].clone();
    assert_eq!(page["slug"], "front");
    let warned = rpc(&mut crm, "publish_page", serde_json::json!([id]));
    assert!(warned["ok"].is_object(), "a category with no posts warns, it does not refuse: {}", warned);
    let front = crm.send(host, "GET", "/p/front", "", None).text();
    let lead = front.find("blog-card is-lead").unwrap();
    assert!(front[lead..].find("Council votes").unwrap() < 400, "{}", &front[lead..lead + 600]);
    assert!(front.contains("<b>Live</b>") && front.contains("Derby won"), "{}", front);
    // For search engines; and the theme picker's preview of another theme.
    let map = crm.send(host, "GET", "/sitemap.xml", "", None).text();
    assert!(map.contains("/blog/council-votes</loc>") && map.contains("/p/front</loc>"), "{}", map);
    assert!(crm.send(host, "GET", "/robots.txt", "", None).text().contains("Sitemap: "));
    let shown = rpc(&mut crm, "preview_site", serde_json::json!([{ "id": "journal" }, "blog"]));
    assert!(shown["ok"].as_str().unwrap().contains("theme-journal head-center"), "{}", shown);
    // Comments: checked by the comment rules, kept until a moderator approves.
    let say = |name: &str, email: &str, body: &str, website: &str| serde_json::json!(["derby-won", { "name": name, "email": email, "body": body, "website": website }]);
    let bad = crm.rpc(host, "post_comment", say("Jimmy", "not-an-email", "Great match", ""), None);
    assert_eq!(bad["err"]["problems"][0]["field"], "email", "{}", bad);
    assert_eq!(crm.rpc(host, "post_comment", say("Bot", "b@spam.test", "buy http://a http://b http://c", "http://spam"), None)["ok"]["waiting"], true);
    assert_eq!(crm.rpc(host, "post_comment", say("Jimmy Olsen", "jimmy@planet.test", "What a header!", ""), None)["ok"]["waiting"], true);
    assert!(!crm.send(host, "GET", "/blog/derby-won", "", None).text().contains("What a header!"));
    let waiting = rpc(&mut crm, "list_comments", serde_json::json!(["pending"]))["ok"].clone();
    assert_eq!(waiting.as_array().unwrap().len(), 1, "the bot's comment was never kept: {}", waiting);
    rpc(&mut crm, "moderate_comment", serde_json::json!([waiting[0]["id"], "approved"]));
    let shown = crm.send(host, "GET", "/blog/derby-won", "", None).text();
    assert!(shown.contains("What a header!") && shown.contains("1 comment"), "{}", shown);
    // Switching the Blog plugin off: the app says what goes with it.
    let uses = rpc(&mut crm, "plugin_uses", serde_json::json!(["blog"]));
    assert_eq!(uses["ok"]["pages"], serde_json::json!(["Front page"]));
}

#[test]
fn custom_objects_with_designer_rules_field_scripts_and_roles() {
    let mut crm = Crm::new("objects");
    let cookie = signup(&mut crm);
    let host = "acme.localhost:9000";
    let rpc = |crm: &mut Crm, c: &str, name: &str, args: serde_json::Value| crm.rpc(host, name, args, Some(c));
    let key = rpc(&mut crm, &cookie, "create_from_template", serde_json::json!(["Ticket"]))["ok"].as_str().unwrap().to_string();
    assert_eq!(key, "ticket");
    crm.rpc(host, "submit_form", serde_json::json!(["newsletter", { "email": "grace@navy.test", "first_name": "Grace", "consent": true }]), None);
    let grace = rpc(&mut crm, &cookie, "list_contacts", serde_json::json!([{}]))["ok"][0]["id"].as_i64().unwrap();
    let ticket = |subject: &str, priority: &str, due: &str, customer: i64| serde_json::json!(["ticket", { "fields": {
        "subject": subject, "customer": customer.to_string(), "priority": priority, "status": "new", "due": due } }]);
    // The designer's rules hold on the server, and links must lead somewhere.
    let r = rpc(&mut crm, &cookie, "save_record", ticket("Broken zip", "urgent", "", grace));
    assert_eq!(r["err"]["problems"][0]["message"], "An urgent ticket needs a due date", "{}", r);
    let r = rpc(&mut crm, &cookie, "save_record", ticket("Broken zip", "normal", "", 999));
    assert_eq!(r["err"]["problems"][0]["message"], "Customer: that one no longer exists", "{}", r);
    let one = rpc(&mut crm, &cookie, "save_record", ticket("Broken zip", "normal", "", grace));
    assert_eq!(one["ok"]["title"], "Broken zip", "{}", one);
    assert_eq!(rpc(&mut crm, &cookie, "records_of_contact", serde_json::json!([grace]))["ok"][0]["title"], "Broken zip");

    // A rule script on a field, run on the server: subjects are not repeated.
    let obj = rpc(&mut crm, &cookie, "get_object", serde_json::json!(["ticket"]))["ok"].clone();
    let subject = obj["fields"].as_array().unwrap().iter().find(|f| f["key"] == "subject").unwrap()["id"].as_i64().unwrap();
    let bad = rpc(&mut crm, &cookie, "save_field_script", serde_json::json!([subject, "others_with(\"subject\", value) == 0 ||"]));
    assert_eq!(bad["err"]["problems"][0]["field"], "script", "{}", bad);
    let script = "others_with(\"subject\", value) == 0 ? true : `“${value}” is already a ticket`";
    assert_eq!(rpc(&mut crm, &cookie, "save_field_script", serde_json::json!([subject, script]))["ok"], true);
    let again = rpc(&mut crm, &cookie, "save_record", ticket("Broken zip", "low", "", grace));
    assert_eq!(again["err"]["problems"][0]["message"], "“Broken zip” is already a ticket", "{}", again);
    let hints = rpc(&mut crm, &cookie, "check_record_scripts", serde_json::json!(["ticket", { "fields": { "subject": "Broken zip" } }]));
    assert_eq!(hints["ok"][0]["field"], "fields.subject");
    // Saving the same record again is not a repeat of itself.
    let id = one["ok"]["id"].as_i64().unwrap();
    let mut same = ticket("Broken zip", "high", "", grace);
    same[1]["id"] = serde_json::json!(id);
    assert!(rpc(&mut crm, &cookie, "save_record", same)["ok"].is_object());
    // A script that runs away is stopped, and says so.
    rpc(&mut crm, &cookie, "save_field_script", serde_json::json!([subject, "let f = (g) => g(g)\nf(f)"]));
    let stuck = rpc(&mut crm, &cookie, "save_record", ticket("Another", "low", "", grace));
    let msg = stuck["err"]["problems"][0]["message"].as_str().unwrap_or("").to_string();
    assert!(msg.starts_with("Subject: its rule script failed") && (msg.contains("too many steps") || msg.contains("recursion")), "{}", stuck);
    // Scripts reach nothing but what they are given.
    let tried = rpc(&mut crm, &cookie, "try_field_script", serde_json::json!(["ticket", "subject", "db.query(\"select * from users\")", {}]));
    assert!(tried["ok"]["error"].as_str().unwrap().contains("unknown name `db`"), "{}", tried);
    rpc(&mut crm, &cookie, "save_field_script", serde_json::json!([subject, ""]));
    let urgent = rpc(&mut crm, &cookie, "save_record", ticket("Site down", "urgent", "2026-11-01", grace))["ok"]["id"].as_i64().unwrap();

    // Roles: Support sees tickets that are not urgent, and may not change them.
    let bad = rpc(&mut crm, &cookie, "save_role", serde_json::json!([{ "name": "Support", "access": { "ticket": { "level": "read", "script": "record.priority !=" } } }]));
    assert_eq!(bad["err"]["problems"][0]["field"], "access", "{}", bad);
    let role = rpc(&mut crm, &cookie, "save_role", serde_json::json!([{ "name": "Support", "access": { "ticket": { "level": "read", "script": "record.priority != \"urgent\"" } } }]));
    let role = role["ok"].as_i64().unwrap_or_else(|| panic!("{}", role));
    rpc(&mut crm, &cookie, "add_user", serde_json::json!([{ "name": "Sam", "email": "sam@acme.test", "password": "a-long-password", "role": "member" }]));
    let ws = rpc(&mut crm, &cookie, "load_workspace", serde_json::json!([]))["ok"].clone();
    let sam = ws["users"].as_array().unwrap().iter().find(|u| u["email"] == "sam@acme.test").unwrap()["id"].as_i64().unwrap();
    rpc(&mut crm, &cookie, "set_user_role", serde_json::json!([sam, role]));
    let r = crm.form(host, "/login", "email=sam%40acme.test&password=a-long-password", None);
    let sam_cookie = r.header("set-cookie").unwrap().split(';').next().unwrap().to_string();
    let seen = rpc(&mut crm, &sam_cookie, "list_records", serde_json::json!(["ticket", ""]))["ok"].clone();
    let titles: Vec<&str> = seen.as_array().unwrap().iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert_eq!(titles, vec!["Broken zip"]);
    assert!(rpc(&mut crm, &sam_cookie, "get_record", serde_json::json!(["ticket", urgent]))["err"].as_str().unwrap().contains("may not see this record"));
    assert!(rpc(&mut crm, &sam_cookie, "save_record", ticket("Mine", "low", "", grace))["err"].as_str().unwrap().contains("may not edit"));
    assert!(rpc(&mut crm, &sam_cookie, "delete_record", serde_json::json!(["ticket", id]))["err"].as_str().unwrap().contains("may not delete"));
    assert_eq!(rpc(&mut crm, &sam_cookie, "records_of_contact", serde_json::json!([grace]))["ok"].as_array().unwrap().len(), 1);
    let csv = crm.send(host, "GET", "/app/export/objects/ticket", "", Some(&sam_cookie)).text();
    assert!(csv.starts_with("id,title,subject") && csv.contains("Broken zip") && !csv.contains("Site down"), "{}", csv);
    // A workspace's owner may do anything.
    assert_eq!(rpc(&mut crm, &cookie, "list_records", serde_json::json!(["ticket", ""]))["ok"].as_array().unwrap().len(), 2);

    // Lookups: a field that shows a field of what a link leads to.
    let lookup = |label: &str, entity: &str, via: &str, field: &str| serde_json::json!([{ "entity": entity, "label": label, "type": "lookup", "options": [via, field] }]);
    let bad = rpc(&mut crm, &cookie, "save_field", lookup("Customer email", "obj.ticket", "subject", "email"));
    assert_eq!(bad["err"]["problems"][0]["message"], "Choose the link it shows a field of", "{}", bad);
    let bad = rpc(&mut crm, &cookie, "save_field", lookup("Customer email", "obj.ticket", "customer", "password_hash"));
    assert_eq!(bad["err"]["problems"][0]["message"], "Choose which of its fields", "{}", bad);
    assert_eq!(rpc(&mut crm, &cookie, "save_field", lookup("Customer email", "obj.ticket", "customer", "email"))["ok"], true);
    let shown = rpc(&mut crm, &cookie, "get_record", serde_json::json!(["ticket", id]))["ok"].clone();
    assert_eq!(shown["fields"]["customer_email"], "grace@navy.test", "{}", shown);
    // Through another object's record, and used by a rule: no more seats than the venue has.
    let venue = rpc(&mut crm, &cookie, "save_object", serde_json::json!([{ "name": "Venue", "plural": "Venues", "title_field": "name" }]))["ok"].as_str().unwrap().to_string();
    for (label, ty) in [("Name", "text"), ("Seats", "number")] {
        rpc(&mut crm, &cookie, "save_field", serde_json::json!([{ "entity": format!("obj.{}", venue), "label": label, "type": ty }]));
    }
    let hall = rpc(&mut crm, &cookie, "save_record", serde_json::json!([venue, { "fields": { "name": "Town hall", "seats": 80 } }]))["ok"]["id"].as_i64().unwrap();
    let event = rpc(&mut crm, &cookie, "save_object", serde_json::json!([{ "name": "Talk", "plural": "Talks" }]))["ok"].as_str().unwrap().to_string();
    let ent = format!("obj.{}", event);
    rpc(&mut crm, &cookie, "save_field", serde_json::json!([{ "entity": ent, "label": "Title", "type": "text" }]));
    rpc(&mut crm, &cookie, "save_field", serde_json::json!([{ "entity": ent, "label": "Where", "type": "record", "options": [venue] }]));
    rpc(&mut crm, &cookie, "save_field", serde_json::json!([{ "entity": ent, "label": "Tickets", "type": "number" }]));
    assert_eq!(rpc(&mut crm, &cookie, "save_field", lookup("Room for", &ent, "where", "seats"))["ok"], true);
    let talk = rpc(&mut crm, &cookie, "get_object", serde_json::json!([event]))["ok"].clone();
    let rule = serde_json::json!({ "if": null, "then": { "field": "tickets", "op": "at most", "value": "@room_for" }, "message": "More tickets than seats", "warning": false });
    rpc(&mut crm, &cookie, "save_object", serde_json::json!([{ "id": talk["id"], "key": event, "name": "Talk", "plural": "Talks", "rules": [rule] }]));
    let talk_of = |n: i64| serde_json::json!([event, { "fields": { "title": "On type", "where": hall.to_string(), "tickets": n, "room_for": 999 } }]);
    let over = rpc(&mut crm, &cookie, "save_record", talk_of(120));
    assert_eq!(over["err"]["problems"][0]["message"], "More tickets than seats", "the lookup is the venue's, not what the form sent: {}", over);
    let ok = rpc(&mut crm, &cookie, "save_record", talk_of(60));
    assert_eq!(ok["ok"]["fields"]["room_for"], 80, "{}", ok);
    // The venue grows: the talk's lookup follows.
    rpc(&mut crm, &cookie, "save_record", serde_json::json!([venue, { "id": hall, "fields": { "name": "Town hall", "seats": 150 } }]));
    let t = ok["ok"]["id"].as_i64().unwrap();
    assert_eq!(rpc(&mut crm, &cookie, "get_record", serde_json::json!([event, t]))["ok"]["fields"]["room_for"], 150);
}

#[test]
fn each_category_has_its_own_editors_reviewers_and_publishers() {
    let mut crm = Crm::new("desks");
    let owner = signup_as(&mut crm, "planet", "news");
    let host = "planet.localhost:9000";
    let rpc = |crm: &mut Crm, c: &str, name: &str, args: serde_json::Value| crm.rpc(host, name, args, Some(c));
    // The team: a writer, two reviewers, a publisher.
    let mut ids = std::collections::HashMap::new();
    let mut cookies = std::collections::HashMap::new();
    for (name, email) in [("Lois", "lois@planet.test"), ("Clark", "clark@planet.test"), ("Jimmy", "jimmy@planet.test"), ("Cat", "cat@planet.test")] {
        rpc(&mut crm, &owner, "add_user", serde_json::json!([{ "name": name, "email": email, "password": "a-long-password", "role": "member" }]));
        let r = crm.form(host, "/login", &format!("email={}&password=a-long-password", email.replace('@', "%40")), None);
        cookies.insert(name, r.header("set-cookie").unwrap().split(';').next().unwrap().to_string());
    }
    let ws = rpc(&mut crm, &owner, "load_workspace", serde_json::json!([]))["ok"].clone();
    for u in ws["users"].as_array().unwrap() {
        ids.insert(u["name"].as_str().unwrap().to_string(), u["id"].as_i64().unwrap());
    }
    let id = |n: &str| ids[n];
    // Politics: Lois writes, Clark and Jimmy review (two approvals), Cat publishes.
    let too_few = rpc(&mut crm, &owner, "save_desk", serde_json::json!([{ "category": "politics", "editors": [id("Lois")], "reviewers": [id("Clark")], "publishers": [id("Cat")], "approvals": 2 }]));
    assert_eq!(too_few["err"]["problems"][0]["field"], "reviewers", "{}", too_few);
    let desk = rpc(&mut crm, &owner, "save_desk", serde_json::json!([{ "category": "Politics", "editors": [id("Lois")], "reviewers": [id("Clark"), id("Jimmy")], "publishers": [id("Cat")], "approvals": 2 }]));
    assert!(desk["ok"].is_i64(), "{}", desk);
    // Sport has no desk: whoever writes publishes.
    let post = |crm: &mut Crm, who: &str, title: &str, category: &str| {
        let c = cookies[who].clone();
        let pid = crm.rpc(host, "new_post", serde_json::json!([]), Some(&c))["ok"].as_i64().unwrap();
        let r = crm.rpc(host, "save_post", serde_json::json!([{ "id": pid, "slug": title.to_lowercase().replace(' ', "-"), "title": title, "excerpt": "", "body": "Words.",
            "cover": null, "tags": [], "description": "", "members_only": false, "category": category, "featured": false }]), Some(&c));
        (pid, r)
    };
    let (sport, _) = post(&mut crm, "Jimmy", "Derby", "sport");
    assert!(rpc(&mut crm, &cookies["Jimmy"], "publish_post", serde_json::json!([sport]))["ok"].is_object());
    // Only the desk's editors write in politics.
    let (_, r) = post(&mut crm, "Jimmy", "Council", "politics");
    assert!(r["err"]["problems"][0]["message"].as_str().unwrap().contains("Politics desk's editors"), "{}", r);
    let (p, r) = post(&mut crm, "Lois", "Council votes", "politics");
    assert!(r["ok"].is_object(), "{}", r);
    // Not published before review, by anyone.
    let no = rpc(&mut crm, &cookies["Lois"], "publish_post", serde_json::json!([p]));
    assert!(no["err"]["problems"][0]["message"].as_str().unwrap().contains("publishers"), "{}", no);
    assert_eq!(rpc(&mut crm, &cookies["Lois"], "submit_post", serde_json::json!([p]))["ok"], true);
    let queue = rpc(&mut crm, &cookies["Clark"], "my_queue", serde_json::json!([]));
    assert_eq!(queue["ok"][0]["waiting"], "review", "{}", queue);
    let mail = rpc(&mut crm, &owner, "list_outbox", serde_json::json!([]))["ok"].clone();
    assert!(mail.as_array().unwrap().iter().any(|m| m["subject"] == "For review: Council votes"), "{}", mail);
    // Lois cannot review her own; Clark asks for changes, with a note.
    assert!(rpc(&mut crm, &cookies["Cat"], "review_post", serde_json::json!([p, "approved", ""]))["err"].as_str().unwrap().contains("reviewers"));
    assert_eq!(rpc(&mut crm, &cookies["Clark"], "review_post", serde_json::json!([p, "changes", ""]))["err"]["problems"][0]["field"], "note");
    rpc(&mut crm, &cookies["Clark"], "review_post", serde_json::json!([p, "changes", "Name the councillors"]));
    assert_eq!(rpc(&mut crm, &cookies["Lois"], "get_post", serde_json::json!([p]))["ok"]["stage"], "changes");
    // Resubmitted; one approval is not enough for Cat.
    rpc(&mut crm, &cookies["Lois"], "submit_post", serde_json::json!([p]));
    rpc(&mut crm, &cookies["Clark"], "review_post", serde_json::json!([p, "approved", ""]));
    let one = rpc(&mut crm, &cookies["Cat"], "publish_post", serde_json::json!([p]));
    assert!(one["err"]["problems"][0]["message"].as_str().unwrap().contains("needs 2 approvals, and has 1"), "{}", one);
    // An edit after review clears the approvals.
    let edit = serde_json::json!([{ "id": p, "slug": "council-votes", "title": "Council votes 7-2", "excerpt": "", "body": "Words.", "cover": null, "tags": [],
        "description": "", "members_only": false, "category": "politics", "featured": false }]);
    rpc(&mut crm, &cookies["Lois"], "save_post", edit);
    assert_eq!(rpc(&mut crm, &cookies["Cat"], "post_workflow", serde_json::json!([p]))["ok"]["approved"], 0);
    rpc(&mut crm, &cookies["Clark"], "review_post", serde_json::json!([p, "approved", ""]));
    rpc(&mut crm, &cookies["Jimmy"], "review_post", serde_json::json!([p, "approved", "Good"]));
    assert_eq!(rpc(&mut crm, &cookies["Lois"], "get_post", serde_json::json!([p]))["ok"]["stage"], "approved");
    assert!(rpc(&mut crm, &cookies["Lois"], "publish_post", serde_json::json!([p]))["err"].is_object(), "the writer does not publish");
    assert!(rpc(&mut crm, &cookies["Cat"], "publish_post", serde_json::json!([p]))["ok"].is_object());
    assert!(crm.send(host, "GET", "/blog/council-votes", "", None).text().contains("Council votes 7-2"));
    let log = rpc(&mut crm, &owner, "post_workflow", serde_json::json!([p]))["ok"]["reviews"].clone();
    assert_eq!(log[0]["decision"], "published");
    assert!(rpc(&mut crm, &cookies["Lois"], "unpublish_post", serde_json::json!([p]))["err"].is_string());
}

#[test]
fn groups_roles_on_fields_state_machines_and_decisions_in_flows() {
    let mut crm = Crm::new("states");
    let owner = signup_as(&mut crm, "planet", "news");
    let host = "planet.localhost:9000";
    let rpc = |crm: &mut Crm, c: &str, name: &str, args: serde_json::Value| crm.rpc(host, name, args, Some(c));
    let mut cookies = std::collections::HashMap::new();
    for (name, email) in [("Wendy", "wendy@planet.test"), ("Ed", "ed@planet.test")] {
        rpc(&mut crm, &owner, "add_user", serde_json::json!([{ "name": name, "email": email, "password": "a-long-password", "role": "member" }]));
        let r = crm.form(host, "/login", &format!("email={}&password=a-long-password", email.replace('@', "%40")), None);
        cookies.insert(name, r.header("set-cookie").unwrap().split(';').next().unwrap().to_string());
    }
    let ws = rpc(&mut crm, &owner, "load_workspace", serde_json::json!([]))["ok"].clone();
    let uid = |n: &str| ws["users"].as_array().unwrap().iter().find(|u| u["name"] == n).unwrap()["id"].as_i64().unwrap();
    // A pitch: pitched → assigned (the editors' move, which asks for a deadline) → filed.
    let key = rpc(&mut crm, &owner, "save_object", serde_json::json!([{ "name": "Pitch", "plural": "Pitches", "title_field": "headline" }]))["ok"].as_str().unwrap().to_string();
    for (label, ty) in [("Headline", "text"), ("Deadline", "date"), ("Budget", "number"), ("Source", "text")] {
        rpc(&mut crm, &owner, "save_field", serde_json::json!([{ "entity": format!("obj.{}", key), "label": label, "type": ty }]));
    }
    let role = rpc(&mut crm, &owner, "save_role", serde_json::json!([{ "name": "Newsroom", "access": { "pitch": { "level": "edit", "script": "", "fields": { "budget": "read", "source": "hidden" } } } }]))["ok"].as_i64().unwrap();
    let editors = rpc(&mut crm, &owner, "save_group", serde_json::json!([{ "name": "Editors", "members": [uid("Ed")], "roles": [role], "capabilities": ["designer", "reviewer"] }]))["ok"].as_i64().unwrap();
    rpc(&mut crm, &owner, "save_group", serde_json::json!([{ "name": "Stringers", "members": [uid("Wendy")], "roles": [role], "capabilities": ["writer"] }]));
    let obj = rpc(&mut crm, &owner, "get_object", serde_json::json!([key]))["ok"].clone();
    let states = serde_json::json!([{ "label": "Pitched" }, { "label": "Assigned", "color": "amber" }, { "label": "Filed", "color": "green" }]);
    let moves = serde_json::json!([
        { "label": "Assign", "from": ["pitched"], "to": "assigned", "groups": [editors], "roles": [], "fields": ["deadline"] },
        { "label": "File", "from": ["assigned"], "to": "filed", "groups": [], "roles": [], "fields": [] },
    ]);
    // Ed designs (a designer, by his group); Wendy does not.
    let as_designer = |c: &str, crm: &mut Crm| crm.rpc(host, "save_object", serde_json::json!([{ "id": obj["id"], "key": key, "name": "Pitch", "plural": "Pitches", "title_field": "headline", "states": states, "transitions": moves }]), Some(c));
    assert!(as_designer(&cookies["Wendy"].clone(), &mut crm)["err"].as_str().unwrap().contains("designers"));
    assert_eq!(as_designer(&cookies["Ed"].clone(), &mut crm)["ok"], "pitch");

    // Wendy pitches: it starts pitched; the budget is not hers to set, the source not hers to see.
    let made = rpc(&mut crm, &cookies["Wendy"], "save_record", serde_json::json!(["pitch", { "fields": { "headline": "Tram strike", "budget": 900, "source": "x" } }]));
    let id = made["ok"]["id"].as_i64().unwrap_or_else(|| panic!("{}", made));
    assert_eq!(made["ok"]["state"], "pitched");
    assert!(made["ok"]["fields"]["budget"].is_null() && made["ok"]["fields"].get("source").is_none(), "{}", made);
    rpc(&mut crm, &owner, "save_record", serde_json::json!(["pitch", { "id": id, "fields": { "headline": "Tram strike", "budget": 500, "source": "a driver" } }]));
    let seen = rpc(&mut crm, &cookies["Wendy"], "get_record", serde_json::json!(["pitch", id]))["ok"].clone();
    assert_eq!(seen["fields"]["budget"], 500);
    assert!(seen["fields"].get("source").is_none() && seen["moves"].as_array().unwrap().is_empty(), "{}", seen);
    // Saving keeps what she may not change, and never the state.
    rpc(&mut crm, &cookies["Wendy"], "save_record", serde_json::json!(["pitch", { "id": id, "state": "filed", "fields": { "headline": "Tram strike: day two", "budget": 1 } }]));
    let after = rpc(&mut crm, &owner, "get_record", serde_json::json!(["pitch", id]))["ok"].clone();
    assert_eq!((after["state"].as_str(), after["fields"]["budget"].as_i64(), after["fields"]["source"].as_str()), (Some("pitched"), Some(500), Some("a driver")));
    assert!(rpc(&mut crm, &cookies["Wendy"], "move_record", serde_json::json!(["pitch", id, "assign", { "deadline": "2026-11-01" }]))["err"].as_str().unwrap().contains("may not make this move"));

    // A flow: when a pitch is assigned, the editors decide; approved, it is filed.
    let graph = serde_json::json!({
        "nodes": [
            { "id": "t", "type": "trigger", "x": 0, "y": 0, "config": { "event": "record_moved", "object": "pitch", "to": "assigned" } },
            { "id": "a", "type": "ask", "x": 200, "y": 0, "config": { "title": "Run it on the front page?", "group": editors.to_string() } },
            { "id": "m", "type": "move_record", "x": 400, "y": 0, "config": { "to": "filed" } },
        ],
        "edges": [{ "from": "t", "port": "out", "to": "a" }, { "from": "a", "port": "approved", "to": "m" }],
    });
    let flow = rpc(&mut crm, &owner, "save_flow", serde_json::json!([{ "name": "Front page", "graph": graph, "active": true }]));
    assert!(flow["ok"].is_object(), "{}", flow);
    // Ed's move: its form needs the deadline.
    let r = rpc(&mut crm, &cookies["Ed"], "get_record", serde_json::json!(["pitch", id]))["ok"].clone();
    assert_eq!(r["moves"][0]["key"], "assign");
    let r = rpc(&mut crm, &cookies["Ed"], "move_record", serde_json::json!(["pitch", id, "assign", {}]));
    assert_eq!(r["err"]["problems"][0]["message"], "Deadline: needed to assign", "{}", r);
    let r = rpc(&mut crm, &cookies["Ed"], "move_record", serde_json::json!(["pitch", id, "assign", { "deadline": "2026-11-01" }]));
    assert_eq!(r["ok"]["state"], "assigned", "{}", r);
    // The flow waits for the editors: not Wendy's to decide.
    assert!(rpc(&mut crm, &cookies["Wendy"], "my_flow_tasks", serde_json::json!([]))["ok"].as_array().unwrap().is_empty());
    let tasks = rpc(&mut crm, &cookies["Ed"], "my_flow_tasks", serde_json::json!([]))["ok"].clone();
    assert_eq!((tasks[0]["title"].as_str(), tasks[0]["record"]["title"].as_str()), (Some("Run it on the front page?"), Some("Tram strike: day two")), "{}", tasks);
    let task = tasks[0]["id"].as_i64().unwrap();
    assert!(rpc(&mut crm, &cookies["Wendy"], "answer_flow_task", serde_json::json!([task, "approved", ""]))["err"].is_string());
    let mail = rpc(&mut crm, &owner, "list_outbox", serde_json::json!([]))["ok"].clone();
    assert!(mail.as_array().unwrap().iter().any(|m| m["subject"] == "To decide: Run it on the front page?" && m["to_email"] == "ed@planet.test"), "{}", mail);
    assert_eq!(rpc(&mut crm, &cookies["Ed"], "answer_flow_task", serde_json::json!([task, "approved", "Lead story"]))["ok"], true);
    assert_eq!(rpc(&mut crm, &owner, "get_record", serde_json::json!(["pitch", id]))["ok"]["state"], "filed");

    // Desks take groups: the Editors (reviewers) review politics.
    rpc(&mut crm, &owner, "save_desk", serde_json::json!([{ "category": "politics", "editors": [], "reviewers": [], "publishers": [], "reviewer_groups": [editors], "approvals": 1 }]));
    let pid = rpc(&mut crm, &cookies["Wendy"], "new_post", serde_json::json!([]))["ok"].as_i64().unwrap();
    let saved = rpc(&mut crm, &cookies["Wendy"], "save_post", serde_json::json!([{ "id": pid, "slug": "strike", "title": "Strike", "excerpt": "", "body": "Words.", "cover": null, "tags": [], "description": "", "members_only": false, "category": "politics", "featured": false }]));
    assert!(saved["ok"].is_object(), "a writer writes on a desk that names no editors: {}", saved);
    rpc(&mut crm, &cookies["Wendy"], "submit_post", serde_json::json!([pid]));
    assert_eq!(rpc(&mut crm, &cookies["Ed"], "my_queue", serde_json::json!([]))["ok"][0]["waiting"], "review");
}

#[test]
fn each_object_is_a_table_of_its_own() {
    let mut crm = Crm::new("tables");
    let owner = signup(&mut crm);
    let host = "acme.localhost:9000";
    let rpc = |crm: &mut Crm, name: &str, args: serde_json::Value| crm.rpc(host, name, args, Some(&owner));
    let columns = |crm: &Crm, table: &str| -> Vec<(String, String)> {
        let c = rusqlite::Connection::open(crm.dir.join("data/acme.db")).unwrap();
        let mut st = c.prepare(&format!("select name, type from pragma_table_info('{}')", table)).unwrap();
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap().map(|x| x.unwrap()).collect()
    };
    assert_eq!(rpc(&mut crm, "create_from_template", serde_json::json!(["Ticket"]))["ok"], "ticket");
    let cols = columns(&crm, "o_ticket");
    assert!(cols.contains(&("f_subject".into(), "TEXT".into())) && cols.contains(&("state".into(), "TEXT".into())), "{:?}", cols);
    assert!(cols.iter().any(|(n, t)| n == "f_customer" && t == "INTEGER"), "a contact is a reference: {:?}", cols);
    // A unique field: the table's index keeps it so.
    assert_eq!(rpc(&mut crm, "save_field", serde_json::json!([{ "entity": "obj.ticket", "label": "Code", "type": "text", "is_unique": true }]))["ok"], true);
    crm.rpc(host, "submit_form", serde_json::json!(["newsletter", { "email": "grace@navy.test", "first_name": "Grace", "consent": true }]), None);
    let grace = rpc(&mut crm, "list_contacts", serde_json::json!([{}]))["ok"][0]["id"].as_i64().unwrap();
    let ticket = |subject: &str, code: &str| serde_json::json!(["ticket", { "fields": { "subject": subject, "customer": grace.to_string(), "priority": "low", "status": "new", "code": code } }]);
    let one = rpc(&mut crm, "save_record", ticket("Zip broke", "A1"));
    assert_eq!(one["ok"]["fields"]["code"], "A1", "{}", one);
    let two = rpc(&mut crm, "save_record", ticket("Zip broke again", "A1"));
    assert_eq!(two["err"]["problems"][0]["message"], "Code: another record has this value", "{}", two);
    // Unique only when no two records share a value already.
    rpc(&mut crm, "save_record", ticket("Zip broke again", "A2"));
    let fields = rpc(&mut crm, "get_object", serde_json::json!(["ticket"]))["ok"]["fields"].clone();
    let field = |k: &str| fields.as_array().unwrap().iter().find(|f| f["key"] == k).unwrap().clone();
    let mut priority = field("priority");
    priority["is_unique"] = serde_json::json!(true);
    let refused = rpc(&mut crm, "save_field", serde_json::json!([priority]));
    assert!(refused["err"]["problems"][0]["message"].as_str().unwrap().contains("share a value of Priority"), "{}", refused);
    // A field's type stays; a field removed is a column dropped.
    let mut code = field("code");
    code["type"] = serde_json::json!("number");
    assert_eq!(rpc(&mut crm, "save_field", serde_json::json!([code]))["err"]["problems"][0]["field"], "type");
    rpc(&mut crm, "delete_field", serde_json::json!([field("due")["id"]]));
    assert!(!columns(&crm, "o_ticket").iter().any(|(n, _)| n == "f_due"));
    // Records from before (one shared table) move in, once.
    crm.sql("acme", "insert into object_records (object, title, fields, state, created_at, updated_at) values ('ticket', 'Old one', '{\"subject\": \"Old one\", \"priority\": \"high\", \"status\": \"open\"}', 'new', 1, 1); delete from settings where key in ('objects_in_tables', 'object_tables')");
    let all = rpc(&mut crm, "list_records", serde_json::json!(["ticket", ""]))["ok"].clone();
    assert!(all.as_array().unwrap().iter().any(|r| r["title"] == "Old one" && r["fields"]["priority"] == "high"), "{}", all);
    // Once: the old rows stay as a backup, and are not copied again.
    let again = rpc(&mut crm, "list_records", serde_json::json!(["ticket", ""]))["ok"].as_array().unwrap().iter().filter(|r| r["title"] == "Old one").count();
    assert_eq!(again, 1);
    let c = rusqlite::Connection::open(crm.dir.join("data/acme.db")).unwrap();
    assert_eq!(c.query_row("select count(*) from object_records", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    // An object another links to stays until that link goes.
    let venue = rpc(&mut crm, "save_object", serde_json::json!([{ "name": "Venue" }]))["ok"].as_str().unwrap().to_string();
    rpc(&mut crm, "save_field", serde_json::json!([{ "entity": "obj.ticket", "label": "Where", "type": "record", "options": [venue] }]));
    assert!(rpc(&mut crm, "delete_object", serde_json::json!([venue]))["err"].as_str().unwrap().contains("Ticket's field Where links to it"));
    assert!(columns(&crm, "o_ticket").iter().any(|(n, t)| n == "f_where" && t == "INTEGER"));
}

#[test]
fn the_newsroom_suite_writes_reviews_and_publishes_with_its_groups() {
    let mut crm = Crm::new("newsroom-suite");
    let owner = signup_as(&mut crm, "metro", "news");
    let host = "metro.localhost:9000";
    let rpc = |crm: &mut Crm, c: &str, name: &str, args: serde_json::Value| crm.rpc(host, name, args, Some(c));
    // The suite's model is the workspace's: two objects, three groups.
    let objects = rpc(&mut crm, &owner, "list_objects", serde_json::json!([]))["ok"].clone();
    let keys: Vec<&str> = objects.as_array().unwrap().iter().map(|o| o["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"article") && keys.contains(&"category"), "{:?}", keys);
    let groups = rpc(&mut crm, &owner, "list_groups", serde_json::json!([]))["ok"].clone();
    let gid = |n: &str| groups.as_array().unwrap().iter().find(|g| g["name"] == n).unwrap()["id"].as_i64().unwrap();
    // Sections, one depending on another.
    let cat = |crm: &mut Crm, name: &str, parent: Option<i64>| crm.rpc(host, "save_record", serde_json::json!(["category", { "fields": { "name": name, "color": "#b42318",
        "parent": parent.map(|p| p.to_string()) } }]), Some(&owner))["ok"]["id"].as_i64().unwrap();
    let politics = cat(&mut crm, "Politics", None);
    let elections = cat(&mut crm, "Elections", Some(politics));
    // Alone (no one in the groups yet), a member does it all: a blogger.
    let mut cookies = std::collections::HashMap::new();
    for (name, email) in [("Ana", "ana@metro.test"), ("Mei", "mei@metro.test"), ("Sam", "sam@metro.test")] {
        rpc(&mut crm, &owner, "add_user", serde_json::json!([{ "name": name, "email": email, "password": "a-long-password", "role": "member" }]));
        let r = crm.form(host, "/login", &format!("email={}&password=a-long-password", email.replace('@', "%40")), None);
        cookies.insert(name, r.header("set-cookie").unwrap().split(';').next().unwrap().to_string());
    }
    let story = |headline: &str, cat: i64| serde_json::json!(["article", { "fields": { "headline": headline, "standfirst": "What it means.", "body": "The council voted **seven to two**.\n\nIt starts in May.", "category": cat.to_string() } }]);
    let solo = rpc(&mut crm, &cookies["Ana"], "save_record", story("A blog post", politics))["ok"]["id"].as_i64().unwrap();
    for m in ["submit", "approve"] {
        let r = rpc(&mut crm, &cookies["Ana"], "move_record", serde_json::json!(["article", solo, m, {}]));
        assert!(r["ok"].is_object() || m == "approve", "{}: {}", m, r);
    }
    // ...the cover rule holds there too: an approved story needs its picture.
    assert!(rpc(&mut crm, &cookies["Ana"], "move_record", serde_json::json!(["article", solo, "approve", {}]))["err"]["problems"][0]["message"].as_str().unwrap().contains("cover"));

    // With a newsroom: Ana writes, Mei reviews, Sam publishes.
    let ws = rpc(&mut crm, &owner, "load_workspace", serde_json::json!([]))["ok"].clone();
    let uid = |n: &str| ws["users"].as_array().unwrap().iter().find(|u| u["name"] == n).unwrap()["id"].as_i64().unwrap();
    for (g, who, caps) in [("Writers", "Ana", vec!["writer"]), ("Reviewers", "Mei", vec!["reviewer"]), ("Publishers", "Sam", vec!["publisher", "reviewer"])] {
        rpc(&mut crm, &owner, "save_group", serde_json::json!([{ "id": gid(g), "name": g, "members": [uid(who)], "roles": [], "capabilities": caps }]));
    }
    let cover = serde_json::json!({ "key": "nope", "w": 960, "h": 600, "alt": "The council" });
    let mut s = story("Council votes 7-2 for the tram", elections);
    s[1]["fields"]["cover"] = cover;
    let id = rpc(&mut crm, &cookies["Ana"], "save_record", s)["ok"]["id"].as_i64().unwrap();
    assert!(rpc(&mut crm, &cookies["Mei"], "move_record", serde_json::json!(["article", id, "submit", {}]))["err"].is_string(), "only writers submit");
    assert_eq!(rpc(&mut crm, &cookies["Ana"], "move_record", serde_json::json!(["article", id, "submit", {}]))["ok"]["state"], "review");
    let mail = rpc(&mut crm, &owner, "list_outbox", serde_json::json!([]))["ok"].clone();
    assert!(mail.as_array().unwrap().iter().any(|m| m["to_email"] == "mei@metro.test" && m["subject"].as_str().unwrap().contains("In review")), "{}", mail);
    assert_eq!(rpc(&mut crm, &cookies["Mei"], "my_records_queue", serde_json::json!([]))["ok"][0]["id"], id);
    // Mei marks a few words of the headline, and the picture; sends it back.
    let note = rpc(&mut crm, &cookies["Mei"], "add_annotation", serde_json::json!(["article", id, { "field": "headline", "quote": "7-2", "prefix": "Council votes ", "image": "", "body": "Say seven to two" }]));
    let note = note["ok"].as_i64().unwrap_or_else(|| panic!("{}", note));
    rpc(&mut crm, &cookies["Mei"], "add_annotation", serde_json::json!(["article", id, { "field": "cover", "quote": "", "prefix": "", "image": "/media/x.jpg", "body": "Crop it tighter" }]));
    assert_eq!(rpc(&mut crm, &cookies["Mei"], "add_annotation", serde_json::json!(["article", id, { "field": "nope", "quote": "x", "body": "?" }]))["err"]["problems"][0]["field"], "field");
    assert_eq!(rpc(&mut crm, &cookies["Mei"], "move_record", serde_json::json!(["article", id, "send_back", {}]))["ok"]["state"], "changes");
    // Ana sees them, answers, resolves, sends it again.
    let notes = rpc(&mut crm, &cookies["Ana"], "list_annotations", serde_json::json!(["article", id]))["ok"].clone();
    assert_eq!((notes[0]["quote"].as_str(), notes[1]["image"].as_str()), (Some("7-2"), Some("/media/x.jpg")));
    rpc(&mut crm, &cookies["Ana"], "reply_annotation", serde_json::json!([note, "Done"]));
    rpc(&mut crm, &cookies["Ana"], "resolve_annotation", serde_json::json!([note, true]));
    let notes = rpc(&mut crm, &cookies["Ana"], "list_annotations", serde_json::json!(["article", id]))["ok"].clone();
    assert_eq!((notes[0]["resolved"].as_bool(), notes[0]["replies"][0]["body"].as_str()), (Some(true), Some("Done")));
    let view = rpc(&mut crm, &cookies["Mei"], "review_view", serde_json::json!(["article", id]))["ok"].clone();
    assert_eq!(view[0]["role"], "headline");
    assert!(view.as_array().unwrap().iter().any(|f| f["key"] == "body" && f["html"].as_str().unwrap().contains("<strong>seven to two</strong>")), "{}", view);
    rpc(&mut crm, &cookies["Ana"], "move_record", serde_json::json!(["article", id, "submit", {}]));
    rpc(&mut crm, &cookies["Mei"], "move_record", serde_json::json!(["article", id, "approve", {}]));
    let slug = format!("/news/{}-council-votes-7-2-for-the-tram", id);
    assert_eq!(crm.send(host, "GET", &slug, "", None).status, 404, "not published yet");
    let preview = crm.send(host, "GET", &format!("{}?preview=1", slug), "", Some(&cookies["Mei"])).text();
    assert!(preview.contains("Preview · Approved"), "{}", &preview[..preview.len().min(400)]);
    assert!(rpc(&mut crm, &cookies["Mei"], "move_record", serde_json::json!(["article", id, "publish", {}]))["err"].is_string());
    assert_eq!(rpc(&mut crm, &cookies["Sam"], "move_record", serde_json::json!(["article", id, "publish", {}]))["ok"]["state"], "published");
    // On the site: the front page (the home), the section (with the one that depends on it), the story.
    let front = crm.send(host, "GET", "/", "", None).text();
    assert!(front.contains("Council votes 7-2 for the tram") && front.contains("np-top"), "{}", front);
    let section = crm.send(host, "GET", "/news/c/politics", "", None).text();
    assert!(section.contains("Council votes 7-2") && section.contains("href=\"/news/c/elections\""), "{}", section);
    let page = crm.send(host, "GET", &slug, "", None).text();
    assert!(page.contains("<strong>seven to two</strong>") && page.contains("By <b>Ana</b>") && page.contains("np-label"), "{}", page);
}
