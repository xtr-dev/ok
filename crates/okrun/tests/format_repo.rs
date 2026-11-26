//! `ok self fmt` over every `.ok` file in the repository: formatting must
//! keep the syntax tree (positions aside) and the comments, and formatting
//! the result again must change nothing. Files with deliberate syntax errors
//! (some golden cases) are skipped, since the formatter refuses them.

use std::path::{Path, PathBuf};

fn ok_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "out") {
                continue;
            }
            ok_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ok") {
            out.push(path);
        }
    }
}

fn repo_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![root.join("ok.ok")];
    ok_files(&root.join("examples"), &mut files);
    ok_files(&root.join("tests"), &mut files);
    files.sort();
    files
}

fn comments(src: &str) -> Vec<(String, bool)> {
    okrun::lexer::lex(0, src)
        .expect("lexes")
        .comments
        .into_iter()
        .map(|c| (c.text, c.own_line))
        .collect()
}

#[test]
fn formatting_keeps_meaning_and_is_idempotent() {
    let files = repo_files();
    assert!(files.len() > 100, "found only {} files", files.len());
    let mut checked = 0;
    let mut problems = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f)
            .expect("read")
            .replace("\r\n", "\n");
        let Ok(before) = okrun::parser::parse(0, &src) else {
            continue;
        };
        checked += 1;
        let once = match okrun::fmt::format_source(&src) {
            Ok(s) => s,
            Err(e) => {
                problems.push(format!("{}: {}", f.display(), e.message));
                continue;
            }
        };
        let after = okrun::parser::parse(0, &once).expect("formatted output parses");
        if okrun::fmt::tree_key(&before) != okrun::fmt::tree_key(&after) {
            problems.push(format!("{}: the syntax tree changed", f.display()));
        }
        if comments(&src) != comments(&once) {
            problems.push(format!("{}: the comments changed", f.display()));
        }
        let twice = okrun::fmt::format_source(&once).expect("formats again");
        if twice != once {
            problems.push(format!("{}: formatting is not idempotent", f.display()));
        }
    }
    assert!(checked > 100, "checked only {checked} files");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Narrow widths make the formatter break almost every line, which
/// exercises the line-breaking rules on real code. The formatter checks the
/// syntax tree and comments of its output itself and refuses on a mismatch.
#[test]
fn line_breaking_keeps_meaning_at_any_width() {
    let mut problems = Vec::new();
    for f in &repo_files() {
        let src = std::fs::read_to_string(f)
            .expect("read")
            .replace("\r\n", "\n");
        if okrun::parser::parse(0, &src).is_err() {
            continue;
        }
        for width in [60, 30, 10] {
            match okrun::fmt::format_with_width(&src, width) {
                Ok(once) => {
                    let twice = okrun::fmt::format_with_width(&once, width).expect("formats again");
                    if twice != once {
                        problems.push(format!("{} at width {width}: not idempotent", f.display()));
                    }
                }
                Err(e) => problems.push(format!("{} at width {width}: {}", f.display(), e.message)),
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
