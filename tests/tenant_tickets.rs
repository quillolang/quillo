//! The ticket a page is given to connect to channels: it carries the tenant
//! the page is in, so that its connection is in that tenant too.

use quillo::server::{HttpRequest, HttpResponse};
use quillo::Interp;

fn ticket(it: &mut Interp, host: &str) -> HttpResponse {
    let headers = vec![("Host".to_string(), host.to_string())];
    it.handle(&HttpRequest { method: "POST".into(), url: "/_quillo/wt-ticket".into(), headers, body: vec![], remote: "127.0.0.1".into() })
}

/// The tenant a ticket names, once it is seen to be good.
fn tenant_in(r: &HttpResponse) -> serde_json::Value {
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let j: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    let t = j["ticket"].as_str().unwrap();
    assert!(quillo::wt::ticket_of(&format!("t={}", t)).is_some());
    let sealed: serde_json::Value = serde_json::from_str(&quillo::session::unseal("quillo channel ticket", t).unwrap()).unwrap();
    sealed["t"].clone()
}

#[test]
fn a_ticket_names_the_tenant_of_the_page_that_asked() {
    let mut it = Interp::new(&std::env::temp_dir());
    // One channel use (changed with FEEDBACK75.md, Q-1052: tickets only for programs with channels or
    // media; branch feedback75).
    it.load_source("tenant_from(\"example.com\")\nchannel.on(\"c\", (m, p) => null)\nroute GET / = \"ok\"\n", "app.qwe").unwrap();
    assert_eq!(tenant_in(&ticket(&mut it, "acme.example.com")), "acme");
    assert_eq!(tenant_in(&ticket(&mut it, "example.com")), serde_json::Value::Null);
}
