//! Thin cargo-fuzz entry point; the harness itself lives in `src/lib.rs` so it
//! can also be smoke-tested with `cargo test --lib` on stable (no nightly,
//! sanitizer, or libFuzzer runtime needed).

#![no_main]

use harvestlock_escrow_fuzz::run;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if !data.is_empty() {
        run(data);
    }
});
