//! The CRM in apps/crm, run as a program: signing a workspace up, rules
//! refusing on the server what the browser hinted at, a purchase running
//! its flow, and waiting flows carried on by the job.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;
use std::path::{Path, PathBuf};

fn copy_app(to: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("apps/crm");
    for dir in ["", "lib", "plugins", "ui", "migrations", "public"] {
        std::fs::create_dir_all(to.join(dir)).unwrap();
        for e in std::fs::read_dir(from.join(dir)).unwrap().flatten() {
            if e.path().is_file() {
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

    /// A server fn's answer: `{"ok": ...}` or `{"err": ...}`.
    fn rpc(&mut self, host: &str, name: &str, args: serde_json::Value, cookie: Option<&str>) -> serde_json::Value {
        let r = self.send(host, "POST", &format!("/_quillo/rpc/{}", name), &args.to_string(), cookie);
        serde_json::from_slice(&r.body).unwrap_or_else(|e| panic!("{} {}: {}", name, e, r.text()))
    }

    fn call(&mut self, name: &str) {
        let f = quillo::value::lookup(&self.it.globals, name).unwrap();
        self.it.call(&f, vec![], 0).unwrap_or_else(|_| panic!("{} failed", name));
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
    let order = |card: &str| serde_json::json!([{ "sku": "tee", "name": "Grace Hopper", "email": "grace@navy.test", "qty": 5, "card": card, "expiry": "12/30", "cvc": "123", "country": "US" }]);
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
    let buy = serde_json::json!([{ "sku": "tee", "name": "Lois Lane", "email": "lois@planet.test", "qty": 1, "card": "4242424242424242", "expiry": "12/30", "cvc": "123", "country": "US" }]);
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
    assert!(themed.contains("--brand: #b91c1c") && themed.contains("class=\"site dark\""), "{}", themed);
    // Switching the Consent plugin off takes its block with it.
    crm.rpc(host, "set_plugins", serde_json::json!([["loyalty"]]), Some(&cookie));
    assert!(!crm.send(host, "GET", "/p/new-arrivals", "", None).text().contains("privacy@acme.test"));
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
