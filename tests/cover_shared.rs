//! What live pages wait on (src/shared.rs), at its edges: a sequence number from another
//! server, more topics than are remembered, and a page following one topic twice.

use quillo::shared::{Bell, Shared};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn topics(names: &[&str]) -> Vec<String> {
    names.iter().map(|t| t.to_string()).collect()
}

/// A bell for this thread, the one that waits: `follow` takes one since FEEDBACK61.md's Q-900, each
/// page's thread woken by its own.
fn bell() -> Arc<Bell> {
    let b = Bell::new();
    b.bind();
    b
}

/// A number from another server says nothing about this one: every topic asked of counts as
/// changed, without waiting.
#[test]
fn a_sequence_number_from_another_server_counts_every_topic_as_changed() {
    let s = Arc::new(Shared::new());
    s.publish(&topics(&["db:notes"]));
    let seq = s.live_seq();
    let asked = topics(&["db:notes", "db:other"]);
    assert_eq!(s.live_now(seq, Some("not this one"), &asked), (seq, asked.clone()));
    assert_eq!(s.live_now(seq, Some(s.id.as_str()), &asked), (seq, Vec::new()), "its own number: nothing since");
    assert_eq!(s.live_now(seq, None, &asked), (seq, Vec::new()));
    let f = s.follow(asked.clone(), &bell());
    assert_eq!(f.wait(seq, Some("not this one"), Duration::from_secs(5)), (seq, asked));
}

/// Past 65536 topics, those no page follows are forgotten: they count as changed at the last
/// change from then on (a page asking gets one needless refresh), while one a page follows
/// keeps its own version.
#[test]
fn past_the_most_topics_remembered_those_no_page_follows_are_forgotten() {
    let s = Arc::new(Shared::new());
    let followed = topics(&["db:kept"]);
    let page = s.follow(followed.clone(), &bell());
    s.publish(&followed);
    let kept_at = s.live_seq();
    let many: Vec<String> = (0..70_000).map(|i| format!("db:t{}", i)).collect();
    s.publish(&many);
    let after = s.live_seq();
    // The followed topic did not change in the big publish.
    assert!(s.unchanged_since(&followed, kept_at));
    // A topic never heard of counts as changed at the floor, which is now the last change.
    assert!(!s.unchanged_since(&topics(&["db:never"]), kept_at));
    assert!(s.unchanged_since(&topics(&["db:never"]), after));
    // One of the many, forgotten, is the same.
    assert!(!s.unchanged_since(&topics(&["db:t5"]), kept_at));
    assert!(s.unchanged_since(&topics(&["db:t5"]), after));
    drop(page);
}

/// A page that names a topic twice follows it once; when it leaves, the topic is let go
/// once, and a page following it later is woken by its next change.
#[test]
fn a_page_following_one_topic_twice_lets_it_go_once() {
    let s = Arc::new(Shared::new());
    s.publish(&topics(&["db:x"]));
    let since = s.live_seq();
    let twice = s.follow(topics(&["db:x", "db:x"]), &bell());
    drop(twice);
    let page = s.follow(topics(&["db:x"]), &bell());
    let s2 = s.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        s2.publish(&topics(&["db:x"]))
    });
    let began = Instant::now();
    let (seq, changed) = page.wait(since, None, Duration::from_secs(10));
    let made = writer.join().unwrap();
    assert_eq!((seq, changed), (made, topics(&["db:x"])));
    // Woken by the change, not at the wait's end.
    assert!(began.elapsed() < Duration::from_secs(5), "{:?}", began.elapsed());
}
