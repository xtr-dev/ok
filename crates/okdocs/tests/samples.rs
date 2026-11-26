//! The guide's code samples are tests: every ok snippet must parse and be in
//! canonical format, and every `output` block must be exactly what the
//! command prints. See the module comment of `src/lib.rs` for the fences.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs")
}

/// Build the `ok` binary of this workspace and return its path. A test of
/// one package cannot ask Cargo for another package's binary, so build it.
fn ok_binary() -> PathBuf {
    let out = Command::new(env!("CARGO"))
        .args([
            "build",
            "-p",
            "okrun",
            "--bin",
            "ok",
            "--message-format=json",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stderr(Stdio::inherit())
        .output()
        .expect("run cargo build");
    assert!(out.status.success(), "cargo build -p okrun failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "ok")
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .expect("cargo reported the ok executable")
}

/// Split a command line the way the playground does: whitespace separates
/// words, quotes group them, leading NAME=value words are environment.
fn split_command(line: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut quote = None;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.get_or_insert_default().push(c),
            (None, '"' | '\'') => {
                quote = Some(ch);
                word.get_or_insert_default();
            }
            (None, c) if c.is_whitespace() => words.extend(word.take()),
            (None, c) => word.get_or_insert_default().push(c),
        }
    }
    words.extend(word);
    let mut env = Vec::new();
    while let Some((k, v)) = words.first().and_then(|w| w.split_once('=')) {
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            break;
        }
        env.push((k.to_string(), v.to_string()));
        words.remove(0);
    }
    (env, words)
}

fn normalize(s: &str) -> String {
    let s = s.replace("\r\n", "\n");
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    lines.join("\n").trim_end().to_string()
}

#[test]
fn samples_parse_and_print_what_the_guide_says() {
    let (_, pages) = okdocs::load(&docs_dir()).expect("load docs");
    let ok = ok_binary();
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("okdocs-samples");
    let mut failures = Vec::new();
    let mut runs = 0;
    // OKDOCS_PAGE=slug[,slug...] checks only those pages, for working on one.
    let only: Option<Vec<String>> = std::env::var("OKDOCS_PAGE")
        .ok()
        .map(|v| v.split(',').map(String::from).collect());
    for page in pages
        .iter()
        .filter(|p| only.as_ref().is_none_or(|o| o.contains(&p.slug)))
    {
        let samples = match okdocs::samples(&page.markdown) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("docs/{}.md: {e}", page.slug));
                continue;
            }
        };
        for sample in samples {
            match sample {
                okdocs::Sample::Code { line, code } => {
                    let at = format!("docs/{}.md:{line}", page.slug);
                    match okrun::fmt::format_source(&code) {
                        Err(e) => {
                            let mut sources = okrun::source::SourceMap::default();
                            sources.add(at.clone(), None, code.as_str().into());
                            failures.push(format!(
                                "{at}: does not parse\n{}",
                                okrun::error::render(&e, &sources, okrun::error::Color::Never)
                            ));
                        }
                        Ok(formatted) if formatted != okrun::fmt::normalize(&code) => {
                            failures.push(format!(
                                "{at}: not in `ok self fmt` format; it formats to:\n{formatted}"
                            ));
                        }
                        Ok(_) => {}
                    }
                }
                okdocs::Sample::Run {
                    line,
                    files,
                    command,
                    exit,
                    expected,
                } => {
                    runs += 1;
                    let at = format!("docs/{}.md:{line}", page.slug);
                    let dir = scratch.join(format!("{}-{line}", page.slug));
                    let _ = std::fs::remove_dir_all(&dir);
                    std::fs::create_dir_all(&dir).unwrap();
                    for (name, body) in &files {
                        let path = dir.join(name);
                        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                        std::fs::write(path, body).unwrap();
                    }
                    let (env, words) = split_command(&command);
                    if words.first().map(String::as_str) != Some("ok") {
                        failures.push(format!("{at}: run=\"{command}\" must start with `ok`"));
                        continue;
                    }
                    // One file for stdout and stderr keeps their order.
                    let log_path = scratch.join(format!("{}-{line}.log", page.slug));
                    let log = std::fs::File::create(&log_path).unwrap();
                    let status = Command::new(&ok)
                        .args(&words[1..])
                        .envs(env)
                        .env("NO_COLOR", "1")
                        .env("OK_TRUST", "1")
                        .env("OK_CONFIG_DIR", dir.join(".config"))
                        .current_dir(&dir)
                        .stdin(Stdio::null())
                        .stdout(log.try_clone().unwrap())
                        .stderr(log)
                        .status()
                        .expect("run ok");
                    let actual = std::fs::read_to_string(&log_path).unwrap();
                    let code = status.code().unwrap_or(-1);
                    if normalize(&actual) != normalize(&expected) || code != exit {
                        failures.push(format!(
                            "{at}: `{command}` exited {code} (expected {exit}) and printed:\n{actual}\n--- the guide says:\n{expected}"
                        ));
                    }
                }
            }
        }
    }
    assert!(runs > 0 || only.is_some(), "the guide has no output blocks");
    assert!(
        failures.is_empty(),
        "{} sample(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn pages_render_and_links_between_them_resolve() {
    let (sections, pages) = okdocs::load(&docs_dir()).expect("load docs");
    let known: Vec<String> = pages.iter().map(okdocs::Page::path).collect();
    let mut broken = Vec::new();
    for i in 0..pages.len() {
        let html = okdocs::render_page(&sections, &pages, i);
        for link in html.split("href=\"/docs/").skip(1) {
            let target = format!("/docs/{}", link.split(['"', '#']).next().unwrap());
            if !known.contains(&target) {
                broken.push(format!("docs/{}.md links to {target}", pages[i].slug));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "links to pages that do not exist:\n{}",
        broken.join("\n")
    );
}

#[test]
fn highlighting_keeps_the_text() {
    let src =
        "# sum\nlet total = prices | sum\nprint(\"total: {total}\")\nrun cargo build --release\n";
    let html = okdocs::highlight(src);
    assert!(html.contains("<span class=\"k\">let</span>"), "{html}");
    assert!(html.contains("<span class=\"c\"># sum</span>"), "{html}");
    assert!(html.contains("<span class=\"f\">print</span>"), "{html}");
    assert!(html.contains("<span class=\"k\">run</span>"), "{html}");
    let text: String = {
        let mut out = String::new();
        let mut in_tag = false;
        for c in html.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                c if !in_tag => out.push(c),
                _ => {}
            }
        }
        out.replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    };
    assert_eq!(text, src);
}
