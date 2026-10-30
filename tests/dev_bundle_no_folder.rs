//! A development bundle for a program whose folder is gone: the bundle is
//! still made, with paths as the program named them. In a test program of its
//! own, as it turns `client::DEV` on.

use quillo::client;
use quillo::Interp;
use std::sync::atomic::Ordering;

#[test]
fn a_program_whose_folder_is_gone_still_gets_its_bundle() {
    client::DEV.store(true, Ordering::Relaxed);
    let gone = std::env::temp_dir().join(format!("quillo-no-such-folder-{}", std::process::id()));
    assert!(!gone.exists());
    let main = gone.join("main.qwe");
    let src = "component Hello() {\n  <p>hello</p>\n}\nroute GET / = <html><body><Hello/></body></html>\n";
    let mut it = Interp::new(&gone);
    it.load_source(src, main.to_str().unwrap()).unwrap_or_else(|e| panic!("{}", e));
    let loader = client::bundle(&it).unwrap();
    let at = loader.split("const at = \"").nth(1).unwrap().split('"').next().unwrap();
    let hash = at.trim_end_matches('/').rsplit('/').next().unwrap();
    let list = loader.split("for (const f of [").nth(1).unwrap().split(']').next().unwrap();
    let files: Vec<String> = serde_json::from_str(&format!("[{}]", list)).unwrap();
    assert_eq!(files, ["quillo.js", "program.js", "boot.js"]);
    assert!(client::dev_file(hash, "program.js").is_some());
}
