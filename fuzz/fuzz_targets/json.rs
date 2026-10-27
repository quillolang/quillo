//! JSON to values and back (src/value.rs, src/interp.rs): what `json.parse`
//! reads, what responses write, and what server fns send to the browser.
#![no_main]

use libfuzzer_sys::fuzz_target;
use quillo::Value;

fuzz_target!(|data: &[u8]| {
    let Ok(j) = serde_json::from_slice::<serde_json::Value>(data) else { return };
    let v = Value::from_json(&j);
    // The text written straight is the JSON `to_json` makes.
    let text = v.json_text();
    let back: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {}", e, text));
    assert_eq!(back, v.to_json(), "{}", text);
    // And it reads as the same value again.
    assert_eq!(Value::from_json(&back).repr(), v.repr());
    // To the browser and back: the same value (but for a map whose one key
    // is `$dec`, `$int` or `$quillo_err`, which is read as what it tags).
    if !text.contains("\"$") {
        let island = quillo::interp::island_json(&v);
        assert_eq!(quillo::interp::from_island_json(&island).repr(), v.repr());
    }
    // The browser's own JSON, read as a server fn reads it.
    let _ = quillo::interp::from_island_json(&j).repr();
});
