//! A file that isn't there, or can't be read: what `use`, `mount`, `service`
//! and `translations` say about it when a program runs. The checker usually
//! says it first; these are programs run without it, as `--no-check` runs them.

mod support;

use quillo::Interp;
use support::temp_dir;

fn run(dir: &std::path::Path, src: &str) -> Result<Interp, String> {
    let mut it = Interp::new(dir);
    it.load_source(src, "main.qwe").map_err(|e| e.to_string())?;
    Ok(it)
}

#[test]
fn use_says_which_file_it_cannot_read() {
    let d = temp_dir("missing-use");
    let e = run(&d, "use \"lib/gone.qwe\"\n").err().expect("refused");
    assert!(e.contains(&format!("cannot read {}", d.join("lib/gone.qwe").display())), "{}", e);
}

#[test]
fn mount_says_which_file_it_cannot_read_and_stops() {
    let d = temp_dir("missing-mount");
    let e = run(&d, "route GET / = \"ok\"\nmount /shop = \"shop.qwe\"\n").err().expect("refused");
    let full = d.join("shop.qwe").display().to_string();
    assert!(e.contains(&format!("cannot read {}", full)), "{}", e);
    assert!(e.starts_with(&full), "the error is the mounted file's: {}", e);
}

#[test]
fn mount_refuses_the_root_of_the_disk() {
    let d = temp_dir("missing-mount-root");
    let e = run(&d, "mount /shop = \"/\"\n").err().expect("refused");
    assert!(e.contains("cannot read /"), "{}", e);
}

#[test]
fn a_mount_that_fails_as_it_loads_names_its_file() {
    let d = temp_dir("mount-fails");
    std::fs::write(d.join("shop.qwe"), "let a = 1\nlet b = a / 0\n").unwrap();
    let e = run(&d, "mount /shop = \"shop.qwe\"\n").err().expect("refused");
    assert!(e.starts_with(&d.join("shop.qwe").display().to_string()), "{}", e);
    assert!(e.contains("division by zero"), "{}", e);
}

#[test]
fn a_service_with_no_file_cannot_be_declared() {
    let d = temp_dir("missing-service");
    let e = run(&d, "service billing = \"billing.qwe\"\n").err().expect("refused");
    assert!(e.contains("billing.qwe"), "{}", e);
    assert!(e.contains("cannot read"), "{}", e);
}

#[test]
fn a_service_that_fails_as_it_loads_names_its_file_and_can_be_declared_again() {
    let d = temp_dir("service-fails");
    std::fs::write(d.join("billing.qwe"), "server fn total() = 1\nlet b = 1 / 0\n").unwrap();
    let mut it = Interp::new(&d);
    let e = it.load_source("service billing = \"billing.qwe\"\n", "main.qwe").err().expect("refused").to_string();
    assert!(e.starts_with(&d.join("billing.qwe").display().to_string()), "{}", e);
    assert!(e.contains("division by zero"), "{}", e);
    std::fs::write(d.join("billing.qwe"), "server fn total() = 1\n").unwrap();
    assert_eq!(it.load_source("service billing = \"billing.qwe\"\nbilling.total()", "main.qwe").unwrap().repr(), "1");
}

#[test]
fn translations_in_a_folder_say_which_file_is_unreadable() {
    let d = temp_dir("missing-locales");
    std::fs::create_dir_all(d.join("locales")).unwrap();
    std::fs::write(d.join("locales/en.json"), [0xff, 0xfe, 0x00]).unwrap();
    let e = run(&d, "translations(\"locales\")\n").err().expect("refused");
    let full = d.join("locales/en.json").display().to_string();
    assert!(e.contains(&format!("`translations`: cannot read {}:", full)), "{}", e);
    assert!(e.contains("UTF-8"), "{}", e);
}

#[test]
fn a_folder_named_like_a_locale_file_cannot_be_read_either() {
    let d = temp_dir("locale-folder");
    std::fs::create_dir_all(d.join("locales/fr.json")).unwrap();
    let e = run(&d, "translations(\"locales\")\n").err().expect("refused");
    assert!(e.contains(&format!("`translations`: cannot read {}:", d.join("locales/fr.json").display())), "{}", e);
}
