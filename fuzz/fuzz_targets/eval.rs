//! Evaluating any program that parses must end in a value, an `ok` error or
//! an exit: never a panic, a stack overflow, a hang or a huge allocation
//! (run with -timeout and -malloc_limit_mb=512). Programs run in the
//! interpreter's sandbox (`okrun::interp::run_sandboxed`): no commands,
//! network, file access, imports, terminal input, sleeping or clock, and a
//! budget of evaluation steps so that endless loops end.
//!
//! Formatting must not change what a program prints, so a program that
//! formats is run again in its formatted form. Error messages may quote
//! line and column numbers, which formatting moves, so digits are ignored
//! in the comparison.
#![no_main]

use libfuzzer_sys::fuzz_target;

const STEPS: u64 = 200_000;

fn without_digits(s: &str) -> String {
    s.chars().filter(|c| !c.is_ascii_digit()).collect()
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    if okrun::parser::parse(0, src).is_err() {
        return;
    }
    let first = okrun::interp::run_sandboxed(src, STEPS);
    let Ok(formatted) = okrun::fmt::format_source(src) else {
        return;
    };
    if formatted == src {
        return;
    }
    let second = okrun::interp::run_sandboxed(&formatted, STEPS);
    assert_eq!(
        without_digits(&first.output),
        without_digits(&second.output),
        "formatting changed what the program prints"
    );
    assert_eq!(
        without_digits(&first.outcome),
        without_digits(&second.outcome),
        "formatting changed how the program ends"
    );
});
