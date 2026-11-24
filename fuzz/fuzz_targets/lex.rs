//! The lexer must return tokens or an error for any input, never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(src) = std::str::from_utf8(data) {
        let _ = okrun::lexer::lex(0, src);
    }
});
