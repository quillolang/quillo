//! `mobile::sync`, copying a phone app's program into its container, when the
//! container gets in the way: the launcher shows what it returns.

mod support;

use quillo::mobile::sync;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn root(name: &str) -> (PathBuf, PathBuf) {
    let r = support::temp_dir(&format!("mobile-failures-{}", name));
    let bundle = r.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("main.qwe"), "route GET / = \"one\"\n").unwrap();
    (bundle, r.join("app"))
}

#[test]
fn a_file_where_the_apps_folder_goes_names_it() {
    let (bundle, app) = root("file-for-folder");
    std::fs::write(&app, "in the way").unwrap();
    let e = sync(&bundle, &app).unwrap_err();
    // macOS says "Not a directory", Linux "File exists".
    assert!(e.starts_with(&app.display().to_string()) && e.contains("os error"), "{}", e);
}

#[test]
fn a_file_where_a_folder_inside_goes_names_that_path() {
    let (bundle, app) = root("inner-file");
    std::fs::create_dir(bundle.join("views")).unwrap();
    std::fs::write(bundle.join("views/page.qwe"), "x").unwrap();
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(app.join("views"), "in the way").unwrap();
    let e = sync(&bundle, &app).unwrap_err();
    assert!(e.starts_with(&app.join("views").display().to_string()), "{}", e);
}

#[test]
fn a_file_of_the_app_that_can_not_be_replaced_is_named() {
    let (bundle, app) = root("read-only");
    assert_eq!(sync(&bundle, &app), Ok(1));
    let kept = app.join("main.qwe");
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::write(bundle.join("main.qwe"), "route GET / = \"two\"\n").unwrap();
    let e = sync(&bundle, &app).unwrap_err();
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(e, format!("{}: Permission denied (os error 13)", kept.display()));
    assert_eq!(std::fs::read_to_string(&kept).unwrap(), "route GET / = \"one\"\n", "the old file is left whole");
    assert_eq!(sync(&bundle, &app), Ok(1), "and it goes through once it can");
}

#[test]
fn a_file_of_the_bundle_it_may_not_read_is_named() {
    let (bundle, app) = root("unreadable");
    let shut = bundle.join("shut.qwe");
    std::fs::write(&shut, "x").unwrap();
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).unwrap();
    let got = sync(&bundle, &app);
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o644)).unwrap();
    let e = got.unwrap_err();
    assert_eq!(e, format!("{}: Permission denied (os error 13)", app.join("shut.qwe").display()));
}
