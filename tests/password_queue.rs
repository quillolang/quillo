//! The queue of passwords to hash: a burst past its limit is told so at once, where it would
//! otherwise pile up behind the cores and hold a login page's memory and threads.

use std::sync::{Arc, Barrier};

const BURST: usize = 1500;

#[test]
fn a_burst_of_passwords_past_the_limit_is_told_to_try_again() {
    let start = Arc::new(Barrier::new(BURST));
    let calls: Vec<_> = (0..BURST)
        .map(|i| {
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                let src = if i % 2 == 0 { "crypto.hash_password(\"pw\")" } else { "crypto.verify_password(\"pw\", \"not a hash\")" };
                (i % 2 == 0, quillo::eval_str(src))
            })
        })
        .collect();
    let (mut hashed, mut checked, mut refused_hash, mut refused_check) = (0, 0, 0, 0);
    for call in calls {
        match call.join().unwrap() {
            (true, Ok(v)) => {
                assert!(v.repr().contains("$argon2id$"), "{}", v.repr());
                hashed += 1;
            }
            (false, Ok(v)) => {
                assert_eq!(v.repr(), "false");
                checked += 1;
            }
            (hash, Err(e)) => {
                let name = if hash { "`crypto.hash_password`" } else { "`crypto.verify_password`" };
                assert_eq!(e.msg, format!("{}: too many passwords are being checked at once; try again shortly", name));
                if hash {
                    refused_hash += 1;
                } else {
                    refused_check += 1;
                }
            }
        }
    }
    assert!(refused_hash > 0 && refused_check > 0, "none was refused: {} hashed, {} checked", hashed, checked);
    assert!(hashed > 0 && checked > 0, "all were refused: {} and {}", refused_hash, refused_check);
    // With the burst gone, the limit is gone too.
    assert_eq!(quillo::eval_str("crypto.verify_password(\"pw\", \"not a hash\")").unwrap().repr(), "false");
}
