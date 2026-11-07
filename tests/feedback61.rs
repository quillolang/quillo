//! FEEDBACK61.md: what complex layout and re-rendering found.

mod support;

use quillo::check::{check_file_opts, Options};
use std::path::PathBuf;
use std::process::Command;
use support::temp_dir;

/// A program of one file, removed when the test ends.
struct App(PathBuf);

impl App {
    fn new(name: &str, src: &str) -> App {
        let d = temp_dir(&format!("feedback61-{}", name));
        std::fs::write(d.join("main.qwe"), src).unwrap();
        App(d)
    }

    /// The classes `quillo check` calls unused, sorted.
    fn unused(&self) -> Vec<String> {
        let opts = Options { css: true, ..Default::default() };
        let mut out: Vec<String> = check_file_opts(&self.0.join("main.qwe"), Default::default(), opts)
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("nothing in the program writes it"))
            .map(|d| d.message.split('`').nth(1).unwrap().trim_start_matches('.').to_string())
            .collect();
        out.sort();
        out
    }

    /// What `quillo classes` prints, one class a line.
    fn listed(&self) -> Vec<String> {
        let o = Command::new(env!("CARGO_BIN_EXE_quillo")).arg("classes").arg(self.0.join("main.qwe")).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A component with these rules and this markup.
fn page(rules: &[&str], markup: &str) -> String {
    let sheet: Vec<String> = rules.iter().map(|r| format!(".{} {{ color: red }}", r)).collect();
    format!(
        "component Part(on: bool, n: list) {{\n    style css`\n{}\n    `\n    <div>{}</div>\n}}\nroute GET / = <html><body><Part on={{true}} n={{[1]}}/></body></html>\n",
        sheet.join("\n"),
        markup
    )
}

#[test]
fn a_class_map_writes_its_keys() {
    let app = App::new("map", &page(&["tab", "active"], "<p class={{ tab: true, active: on }}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
    assert_eq!(app.listed(), ["active", "tab"]);
}

#[test]
fn a_key_counts_whatever_its_value_says() {
    // `false` today may be a variable tomorrow: the rule is not dead because of what it holds now.
    let app = App::new("false", &page(&["off"], "<p class={{ off: false }}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
}

#[test]
fn a_quoted_key_with_a_dash_a_space_or_an_accent_is_written() {
    let app = App::new(
        "quoted",
        &page(&["is-on", "two", "words", "café"], "<p class={{ \"is-on\": on, \"two words\": on, \"café\": on }}>x</p>"),
    );
    assert_eq!(app.unused(), Vec::<String>::new());
    assert_eq!(app.listed(), ["café", "is-on", "two", "words"]);
}

#[test]
fn a_map_in_a_list_or_a_choice_is_read() {
    let app = App::new(
        "nested",
        &page(
            &["inlist", "deep", "yes", "no"],
            "<p class={[\"a\", { inlist: on }, [{ deep: on }]]}>x</p><p class={on ? { yes: true } : { no: true }}>y</p>",
        ),
    );
    assert_eq!(app.unused(), ["a"; 0]);
    assert_eq!(app.listed(), ["a", "deep", "inlist", "no", "yes"]);
}

#[test]
fn a_toggle_writes_its_class() {
    let app = App::new("toggle", &page(&["toggled"], "<p class:toggled={on}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
}

#[test]
fn a_map_is_read_on_a_child_and_in_a_loop() {
    let app = App::new(
        "where",
        &page(&["child", "row"], "<section><p class={{ child: on }}>x</p></section>{for i in n => <p class={{ row: on }}>{i}</p>}"),
    );
    assert_eq!(app.unused(), Vec::<String>::new());
    assert_eq!(app.listed(), ["child", "row"]);
}

#[test]
fn class_name_takes_a_map_as_class_does() {
    let app = App::new("classname", &page(&["named"], "<p className={{ named: on }}>x</p>"));
    assert_eq!(app.unused(), Vec::<String>::new());
}

#[test]
fn a_dead_rule_is_still_told() {
    let app = App::new("dead", &page(&["live", "gone"], "<p class={{ live: on }}>x</p>"));
    assert_eq!(app.unused(), ["gone"]);
}

#[test]
fn an_empty_map_writes_nothing() {
    let app = App::new("empty", &page(&["gone"], "<p class={{}}>x</p>"));
    assert_eq!(app.unused(), ["gone"]);
    assert_eq!(app.listed(), Vec::<String>::new());
}

#[test]
fn a_map_that_is_no_class_writes_nothing() {
    // Its keys are fields or properties: `.title` here is dead however many records have a title.
    let src = page(&["title", "color"], "<p style={{ color: \"red\" }}>x</p>");
    let app = App::new("record", &format!("let post = {{ title: 1 }}\nprint(post)\n{}", src));
    assert_eq!(app.unused(), ["color", "title"]);
}

#[test]
fn the_forms_that_worked_still_work() {
    let app = App::new(
        "plain",
        &page(&["short", "said", "built-big"], "<p.short class=\"said\">x</p><p class={`built-${on ? \"big\" : \"small\"}`}>y</p>"),
    );
    assert_eq!(app.unused(), Vec::<String>::new());
}
