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
    // Each file is under a hash of its own (FEEDBACK54.md, Q-688; branch hmr: the test changed
    // for it), `<hash>/<path>`.
    let list = loader.split("for (const f of [").nth(1).unwrap().split(']').next().unwrap();
    let named: Vec<String> = serde_json::from_str(&format!("[{}]", list)).unwrap();
    let files: Vec<(&str, &str)> = named.iter().map(|f| f.split_once('/').unwrap()).collect();
    assert_eq!(files.iter().map(|f| f.1).collect::<Vec<_>>(), ["quillo.js", "program.js", "boot.js"]);
    let (hash, _) = files[1];
    assert!(client::dev_file(hash, "program.js").is_some());
}
