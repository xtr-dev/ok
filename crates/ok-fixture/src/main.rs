//! `ok-fixture ACTION [ARGS...]`: a stand-in for system tools in ok's tests.
//! Tests must not depend on `echo`, `cat` or `sh`, which differ per OS or do
//! not exist on Windows.
//!
//! Actions:
//!   echo ARGS...        print the arguments joined by spaces
//!   args ARGS...        print each argument on its own line, as [arg]
//!   stderr TEXT...      print the text to stderr
//!   exit CODE [TEXT]    print TEXT to stderr, exit with CODE
//!   mixed [CODE]        print "out 1", "err 1", "out 2" alternating between
//!                       stdout and stderr (flushing each), exit with CODE
//!   cat                 copy stdin to stdout
//!   upper               copy stdin to stdout in upper case
//!   drain CODE          read stdin to the end, then exit with CODE
//!   lines N             print lines 1..=N
//!   forever             print lines until the pipe closes
//!   sleep MS            sleep for MS milliseconds
//!   spawn FILE ACTION.. start `ok-fixture ACTION..` as a child sharing
//!                       stdout and stderr, write its process id to FILE,
//!                       wait for it and exit with its code
//!   env NAME            print the variable, or <unset>
//!   pwd                 print the working directory's last component
//!   bytes               print bytes that are not valid UTF-8
//!
//! Fake-tool mode: a copy of this program named after another tool (`git`,
//! `docker`, ...) prints `$ tool args` to stderr and answers from the rules
//! in the JSON file named by `OK_FAKES`:
//!
//! ```json
//! {"git": [{"args": ["rev-parse", "HEAD"], "stdout": "abc\n", "exit": 0}]}
//! ```
//!
//! A rule matches when its `args` are a prefix of the actual arguments
//! (no `args` matches everything). Fields: `stdout`, `stderr`, `exit`,
//! `cat` (copy stdin to stdout) and `write` (files to create, relative to
//! the working directory). Without a matching rule the tool succeeds
//! silently.

use std::io::{Read, Write};

fn fake(tool: &str, args: &[String]) -> i32 {
    let shown: Vec<String> = args
        .iter()
        .map(|a| {
            if a.is_empty() || a.contains(' ') {
                format!("{a:?}")
            } else {
                a.clone()
            }
        })
        .collect();
    eprintln!("$ {tool} {}", shown.join(" "));
    let rules: serde_json::Value = std::env::var("OK_FAKES")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null);
    let Some(list) = rules.get(tool).and_then(|r| r.as_array()) else {
        return 0;
    };
    for rule in list {
        let want: Vec<&str> = rule
            .get("args")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        if want.len() > args.len() || want.iter().zip(args).any(|(w, a)| w != a) {
            continue;
        }
        let mut out = std::io::stdout().lock();
        if rule.get("cat").and_then(|c| c.as_bool()) == Some(true) {
            let mut s = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut s);
            let _ = out.write_all(&s);
        }
        if let Some(files) = rule.get("write").and_then(|w| w.as_object()) {
            for (path, content) in files {
                let p = std::path::Path::new(path);
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(p, content.as_str().unwrap_or(""));
            }
        }
        if let Some(text) = rule.get("stdout").and_then(|x| x.as_str()) {
            let _ = out.write_all(text.as_bytes());
        }
        let _ = out.flush();
        if let Some(text) = rule.get("stderr").and_then(|x| x.as_str()) {
            eprint!("{text}");
        }
        return rule.get("exit").and_then(|e| e.as_i64()).unwrap_or(0) as i32;
    }
    0
}

fn main() {
    let tool = std::env::args().next().map(|a| {
        let p = std::path::PathBuf::from(a);
        p.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    if let Some(tool) = tool.filter(|t| t != "ok-fixture") {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::process::exit(fake(&tool, &args));
    }
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
        Some("mixed") => {
            let _ = writeln!(out, "out 1");
            let _ = out.flush();
            eprintln!("err 1");
            let _ = writeln!(out, "out 2");
            let _ = out.flush();
            rest.first().and_then(|c| c.parse().ok()).unwrap_or(0)
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
        Some("drain") => {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            rest.first().and_then(|c| c.parse().ok()).unwrap_or(0)
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
        Some("spawn") if rest.len() >= 2 => {
            let me = std::env::current_exe().expect("own path");
            let mut child = std::process::Command::new(me)
                .args(&rest[1..])
                .spawn()
                .expect("start the child");
            let _ = std::fs::write(&rest[0], child.id().to_string());
            child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
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
