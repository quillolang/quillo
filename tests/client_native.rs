//! A plugin's function in browser code, refused by the compiler itself. One
//! test: it sets the plugin cache in this process's environment.

mod support;

use quillo::Interp;

#[test]
fn the_compiler_keeps_a_plugins_function_on_the_server() {
    std::env::set_var("QUILLO_NATIVE_CACHE", std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("native-cache"));
    let d = support::temp_dir("client-native");
    std::fs::write(d.join("kv.qwer"), "pub fn fetch_one(key: &str) -> String { key.to_string() }\n").unwrap();
    let src = "use \"kv.qwer\"\ncomponent C() = <p>{fetch_one(\"a\")}</p>\n";
    let e = Interp::new(d.to_str().unwrap()).load_source(src, "main.qwe").err().expect("it loaded").to_string();
    assert_eq!(e, "main.qwe:2: <C> (compiled for the browser): `fetch_one` is Rust, which runs on the server, not in the browser; call it from a `server fn`");
    let _ = std::fs::remove_dir_all(&d);
}
