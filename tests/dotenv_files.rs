//! `.env` files: read into the process before anything reads it, a real
//! variable winning over a file's. One test, since it changes the process's
//! environment.

use quillo::dotenv::{load_into_process, parse, read, var};
use std::collections::HashMap;

fn pairs(text: &str) -> Vec<(String, String)> {
    parse(text)
}

fn one(k: &str, v: &str) -> Vec<(String, String)> {
    vec![(k.to_string(), v.to_string())]
}

#[test]
fn dotenv_files_fill_in_what_the_environment_does_not_set() {
    // What the lines mean.
    assert_eq!(pairs("\n# a comment\n   \n"), vec![]);
    assert_eq!(pairs("no equals sign"), vec![]);
    assert_eq!(pairs("=value"), vec![]);
    assert_eq!(pairs("BAD-KEY=value"), vec![]);
    assert_eq!(pairs("export A=1"), one("A", "1"));
    assert_eq!(pairs("  A = 1  "), one("A", "1"));
    assert_eq!(pairs("A=bare # a note"), one("A", "bare"));
    assert_eq!(pairs("A=bare#not a note"), one("A", "bare#not a note"));
    assert_eq!(pairs(r#"A="two\nlines\tand \"quotes\" \\ \q" ignored"#), one("A", "two\nlines\tand \"quotes\" \\ q"));
    assert_eq!(pairs(r#"A="ends in a slash\"#), one("A", "ends in a slash"));
    assert_eq!(pairs("A='as $written\\n' ignored"), one("A", "as $written\\n"));
    assert_eq!(pairs("A='no end"), one("A", "no end"));

    let d = std::env::temp_dir().join(format!("quillo-dotenv-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join(".env");
    std::fs::write(&f, "QUILLO_T_REAL=from file\nQUILLO_T_FILE=from file\n").unwrap();
    std::env::set_var("QUILLO_T_REAL", "real");
    load_into_process(&f, true).unwrap();
    assert_eq!(std::env::var("QUILLO_T_REAL").unwrap(), "real");
    assert_eq!(std::env::var("QUILLO_T_FILE").unwrap(), "from file");
    let keys = std::env::var("QUILLO_ENV_FROM_FILE").unwrap();
    assert!(keys.split(',').any(|k| k == "QUILLO_T_FILE") && !keys.split(',').any(|k| k == "QUILLO_T_REAL"), "{}", keys);

    // A second file may replace what a file set, not what the environment did.
    let second = d.join("other.env");
    std::fs::write(&second, "QUILLO_T_REAL=second\nQUILLO_T_FILE=second\n").unwrap();
    load_into_process(&second, false).unwrap();
    assert_eq!(std::env::var("QUILLO_T_REAL").unwrap(), "real");
    assert_eq!(std::env::var("QUILLO_T_FILE").unwrap(), "second");

    // A file that is not there is nothing, unless it was named.
    let missing = d.join("missing.env");
    assert!(load_into_process(&missing, false).is_ok());
    let e = load_into_process(&missing, true).unwrap_err();
    assert!(e.starts_with("cannot read ") && e.contains("missing.env"), "{}", e);

    // A service's own file: the environment first, unless a file set it.
    let own: HashMap<String, String> = read(&f);
    assert_eq!(own.len(), 2);
    assert!(read(&missing).is_empty());
    assert_eq!(var(&own, "QUILLO_T_REAL").as_deref(), Some("real"));
    assert_eq!(var(&own, "QUILLO_T_FILE").as_deref(), Some("from file"));
    assert_eq!(var(&HashMap::new(), "QUILLO_T_FILE").as_deref(), Some("second"));
    let only: HashMap<String, String> = [("QUILLO_T_ONLY_OWN".to_string(), "mine".to_string())].into();
    assert_eq!(var(&only, "QUILLO_T_ONLY_OWN").as_deref(), Some("mine"));
    assert_eq!(var(&only, "QUILLO_T_NOWHERE"), None);
    // The list of keys is no variable of the program's.
    assert_eq!(var(&HashMap::new(), "QUILLO_ENV_FROM_FILE"), None);
    let _ = std::fs::remove_dir_all(&d);
}
