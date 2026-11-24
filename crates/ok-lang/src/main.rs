//! Entry point of the `ok` binary. Kept thin: all behaviour lives in the
//! `ok_lang` library so it can be tested without spawning processes.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--version"] | ["self", "version"] => {
            println!("ok {}", ok_lang::VERSION);
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{}", ok_lang::NOT_IMPLEMENTED);
            ExitCode::FAILURE
        }
    }
}
