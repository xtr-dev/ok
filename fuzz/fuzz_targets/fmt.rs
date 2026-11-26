//! The formatter must accept every program that parses, keep its meaning
//! (it checks the syntax tree and comments of its own output and refuses on
//! a difference, which is a bug here) and be idempotent. A narrow width
//! makes it break lines often. Also run the static checker on it, which must
//! never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    if okrun::parser::parse(0, src).is_err() {
        return;
    }
    for width in [100, 20] {
        let once = okrun::fmt::format_with_width(src, width)
            .unwrap_or_else(|e| panic!("formatting a valid program failed: {}", e.message));
        let twice = okrun::fmt::format_with_width(&once, width).expect("formats again");
        assert_eq!(once, twice, "formatting is not idempotent");
    }
    let _ = okrun::check::check_source(src);
});
