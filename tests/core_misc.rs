//! The small parts under the language: errors as they are shown, the lock
//! threads hand each other, the pool for large lists, stacks grown on
//! demand, and what is read from a program without running it.

use quillo::shared::{Baton, Shared};
use quillo::{error, island, par, stack, QuilloError};
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;

fn message(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default()
}

#[test]
fn an_error_shows_what_it_knows_of_where_it_was() {
    assert_eq!(QuilloError::new("no", 3).in_file("a.qwe").to_string(), "a.qwe:3: no");
    assert_eq!(QuilloError::new("no", 0).in_file("a.qwe").to_string(), "a.qwe: no");
    assert_eq!(QuilloError::new("no", 3).to_string(), "line 3: no");
    assert_eq!(QuilloError::new("no", 0).to_string(), "no");
    assert_eq!(QuilloError::new("no", 1).in_file("a.qwe").in_file("b.qwe").to_string(), "a.qwe:1: no");
}

/// Run from a copy of this test program, whose file can be changed: a
/// process rebuilt under it says so.
#[test]
fn a_process_built_again_since_it_started_says_to_restart() {
    const COPY: &str = "QUILLO_TEST_RUNNING_FROM_A_COPY";
    let exe = std::env::current_exe().unwrap();
    if std::env::var_os(COPY).is_some() {
        error::note_start();
        let notes = error::which_quillo();
        assert!(notes.contains("if the code was written for a newer Quillo"), "{}", notes);
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(86_400);
        std::fs::File::open(&exe).unwrap().set_modified(later).unwrap();
        let notes = error::which_quillo();
        assert!(notes.contains("which has been built again since this process started; restart it to use the new build"), "{}", notes);
        return;
    }
    let dir = std::env::temp_dir().join(format!("quillo-core-misc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join("copy");
    std::fs::copy(&exe, &copy).unwrap();
    let out = std::process::Command::new(&copy)
        .args(["--exact", "a_process_built_again_since_it_started_says_to_restart", "--nocapture"])
        .env(COPY, "1")
        .output()
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"), "{}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn a_baton_goes_to_one_thread_at_a_time() {
    let baton = Arc::new(Baton::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..6)
        .map(|_| {
            let (baton, count) = (baton.clone(), count.clone());
            std::thread::spawn(move || {
                for _ in 0..50_000 {
                    baton.take();
                    // Not an atomic add: only the baton keeps these apart.
                    let n = count.load(SeqCst);
                    std::hint::spin_loop();
                    count.store(n + 1, SeqCst);
                    baton.give();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(count.load(SeqCst), 300_000);
}

#[test]
fn a_new_shared_state_starts_with_nothing_changed() {
    let s = Shared::default();
    assert_eq!(s.live_seq(), 1);
    assert_eq!(s.id.len(), 8);
    assert_ne!(s.id, Shared::default().id);
}

#[test]
fn each_task_runs_once() {
    let done: Vec<AtomicUsize> = (0..100).map(|_| AtomicUsize::new(0)).collect();
    par::each(100, 1 << 20, |i| {
        done[i].fetch_add(1, SeqCst);
    });
    assert!(done.iter().all(|d| d.load(SeqCst) == 1));
}

#[test]
fn while_the_pool_is_busy_another_caller_works_alone() {
    let (started, finished) = (AtomicBool::new(false), AtomicBool::new(false));
    std::thread::scope(|s| {
        s.spawn(|| {
            par::each(4, 1 << 20, |_| {
                started.store(true, SeqCst);
                while !finished.load(SeqCst) {
                    std::thread::yield_now();
                }
            })
        });
        while !started.load(SeqCst) {
            std::thread::yield_now();
        }
        let n = AtomicUsize::new(0);
        par::each(10, 1 << 20, |_| {
            n.fetch_add(1, SeqCst);
        });
        assert_eq!(n.load(SeqCst), 10);
        finished.store(true, SeqCst);
    });
}

#[test]
fn a_panic_in_a_task_reaches_the_caller() {
    // More tasks than threads: this thread's share panics too.
    let r = catch_unwind(|| par::each(1000, 1 << 20, |_| panic!("a task failed")));
    assert_eq!(message(r.unwrap_err()), "a task failed");
    // And the pool still works.
    let n = AtomicUsize::new(0);
    par::each(1000, 1 << 20, |_| {
        n.fetch_add(1, SeqCst);
    });
    assert_eq!(n.load(SeqCst), 1000);
}

#[test]
fn a_panic_on_a_grown_stack_reaches_the_caller() {
    assert_eq!(stack::maybe_grow(1 << 40, 1 << 20, || 5), 5);
    let r = catch_unwind(|| stack::maybe_grow(1 << 40, 1 << 20, || panic!("deep")));
    assert_eq!(message(r.unwrap_err()), "deep");
    // The stack's end is the outer one's again.
    assert!(stack::remaining().is_some_and(|left| left > 1 << 12));
}

#[test]
fn an_element_of_a_table_is_its_own_island_when_it_is_one_element() {
    assert_eq!(island::own_root("<tr><td><table><tr><td>in</td></tr></table></td></tr>"), Some(3));
    assert_eq!(island::own_root("<tr><td>not closed</td>"), None);
    assert_eq!(island::own_root("<tr><td>a</td></tr>x"), None);
}

/// Whether the component `body` starts with the page (`data-e`).
fn runs_by_itself(body: &str) -> bool {
    let src = format!("server fn f() = 1\ncomponent A() {{\n{}\n}}\n", body);
    let stmts = quillo::parser::Parser::new(&src).parse_program().unwrap_or_else(|e| panic!("{}", e));
    let quillo::ast::Stmt::Fn(def) = &stmts[1] else { panic!("no component") };
    island::runs_by_itself(&def.body)
}

#[test]
fn only_what_runs_while_the_component_renders_starts_it() {
    assert!(runs_by_itself("let x = live(f)\n<i/>"));
    assert!(runs_by_itself("state x = live(f)\n<i/>"));
    assert!(runs_by_itself("<i @mount={() => 1}/>"));
    assert!(!runs_by_itself("<i/>"));
    // A handler runs when someone acts; a `reply` here does not start anything.
    assert!(!runs_by_itself("let g = () => every(10, f)\nif 1 > 2 {\n  reply 404\n}\n<i/>"));
    assert!(!runs_by_itself("fn g() = 1\nuse x.qwe\n<i/>"));
    assert!(!runs_by_itself("return\n<i/>"));
}

#[test]
fn a_parsed_program_can_be_printed() {
    let stmts = quillo::parser::Parser::new("let a = <b/>").parse_program().unwrap_or_else(|e| panic!("{}", e));
    assert!(format!("{:?}", stmts).contains("ElFacts"));
}

#[test]
fn an_iframe_s_written_sandbox_lets_it_show_data() {
    let fails = |src: &str| match quillo::eval_str(src) {
        Ok(v) => panic!("{} gave {}", src, v.repr()),
        Err(e) => e.msg,
    };
    let refused = "`srcdoc` in <iframe> takes only text written here, unless the element also has a `sandbox` attribute without allow-same-origin";
    let shown = |src: &str| quillo::eval_str(src).map(|v| v.repr()).unwrap_or_else(|e| panic!("{}", e));
    assert_eq!(shown("let x = \"<b>\"\n<iframe srcdoc={x} sandbox/>"), "<iframe srcdoc=\"&lt;b&gt;\" sandbox></iframe>");
    assert_eq!(shown("let x = \"<b>\"\n<iframe srcdoc={x} sandbox=\"allow-scripts\"/>"), "<iframe srcdoc=\"&lt;b&gt;\" sandbox=\"allow-scripts\"></iframe>");
    assert_eq!(fails("let x = \"<b>\"\nlet y = \"a\"\n<iframe srcdoc={x} sandbox={y}/>"), refused);
    assert_eq!(fails("let x = \"<b>\"\n<iframe srcdoc={x} sandbox=\"allow-same-origin\"/>"), refused);
}
