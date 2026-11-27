//! Every `.ok` file under `examples/` must parse: the examples are the
//! design review of the language, so a parse failure means the grammar and
//! the examples disagree.

use std::path::{Path, PathBuf};

fn ok_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read examples dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            ok_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ok")
            || path
                .file_name()
                .is_some_and(|n| n == ".ok" || n == "pre-commit")
        {
            out.push(path);
        }
    }
}

#[test]
fn all_examples_parse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    ok_files(&root, &mut files);
    assert!(
        files.len() >= 15,
        "expected the M0 examples, found {}",
        files.len()
    );
    let mut failures = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).expect("read example");
        if let Err(e) = okrun::parser::parse(0, &src) {
            let mut sources = okrun::source::SourceMap::default();
            sources.add(f.display().to_string(), None, src.into());
            failures.push(okrun::error::render(
                &e,
                &sources,
                okrun::error::Color::Never,
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "examples that do not parse:\n{}",
        failures.join("\n")
    );
}

#[test]
fn repo_ok_file_parses() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ok.ok");
    let src = std::fs::read_to_string(path).expect("read ok.ok");
    okrun::parser::parse(0, &src).expect("ok.ok parses");
}

#[test]
fn examples_are_in_canonical_format() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    ok_files(&root, &mut files);
    files.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ok.ok"));
    let mut problems = Vec::new();
    for f in &files {
        let src = std::fs::read_to_string(f).expect("read example");
        let formatted = okrun::fmt::format_source(&src).expect("format");
        if formatted != src {
            let first = src
                .lines()
                .zip(formatted.lines())
                .enumerate()
                .find(|(_, (a, b))| a != b);
            problems.push(format!("{}: {:?}", f.display(), first));
        }
    }
    assert!(
        problems.is_empty(),
        "examples that `ok self fmt` would change:\n{}",
        problems.join("\n")
    );
}
