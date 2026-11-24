//! `ok-fixture ACTION [ARGS...]`: a stand-in for system tools in ok's tests.
//! Tests must not depend on `echo`, `cat` or `sh`, which differ per OS or do
//! not exist on Windows.
//!
//! Actions:
//!   echo ARGS...        print the arguments joined by spaces
//!   args ARGS...        print each argument on its own line, as [arg]
//!   stderr TEXT...      print the text to stderr
//!   exit CODE [TEXT]    print TEXT to stderr, exit with CODE
//!   cat                 copy stdin to stdout
//!   upper               copy stdin to stdout in upper case
//!   lines N             print lines 1..=N
//!   forever             print lines until the pipe closes
//!   sleep MS            sleep for MS milliseconds
//!   env NAME            print the variable, or <unset>
//!   pwd                 print the working directory's last component
//!   bytes               print bytes that are not valid UTF-8

use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]).to_vec();
    let mut out = std::io::stdout().lock();
    let code = match args.first().map(String::as_str) {
        Some("echo") => {
            let _ = writeln!(out, "{}", rest.join(" "));
            0
        }
        Some("args") => {
            for a in &rest {
                let _ = writeln!(out, "[{a}]");
            }
            0
        }
        Some("stderr") => {
            eprintln!("{}", rest.join(" "));
            0
        }
        Some("exit") => {
            if rest.len() > 1 {
                eprintln!("{}", rest[1..].join(" "));
            }
            rest.first().and_then(|c| c.parse().ok()).unwrap_or(1)
        }
        Some("cat") | Some("upper") => {
            let mut s = String::new();
            let _ = std::io::stdin().read_to_string(&mut s);
            if args[0] == "upper" {
                s = s.to_uppercase();
            }
            let _ = out.write_all(s.as_bytes());
            0
        }
        Some("lines") => {
            let n: u64 = rest.first().and_then(|c| c.parse().ok()).unwrap_or(3);
            for i in 1..=n {
                let _ = writeln!(out, "line {i}");
            }
            0
        }
        Some("forever") => {
            let mut i = 0u64;
            loop {
                i += 1;
                if writeln!(out, "line {i}").is_err() {
                    break 0;
                }
            }
        }
        Some("sleep") => {
            let ms: u64 = rest.first().and_then(|c| c.parse().ok()).unwrap_or(100);
            std::thread::sleep(std::time::Duration::from_millis(ms));
            0
        }
        Some("env") => {
            let name = rest.first().cloned().unwrap_or_default();
            let _ = writeln!(
                out,
                "{}",
                std::env::var(&name).unwrap_or_else(|_| "<unset>".into())
            );
            0
        }
        Some("pwd") => {
            let dir = std::env::current_dir().unwrap_or_default();
            let _ = writeln!(
                out,
                "{}",
                dir.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
            0
        }
        Some("bytes") => {
            let _ = out.write_all(b"\xff\xfe\n");
            0
        }
        _ => {
            eprintln!("ok-fixture: unknown action {:?}", args.first());
            2
        }
    };
    let _ = out.flush();
    std::process::exit(code);
}
