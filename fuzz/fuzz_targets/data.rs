//! Reading untrusted data must end in a value or an `ok` error, never a
//! panic, a hang or unbounded memory (run with -timeout and -rss_limit_mb).
//! The first byte picks the format and whether the file is gzip-compressed;
//! the rest is the file. The value is then written back as JSON, so the
//! writers see whatever the readers produce.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

const FORMATS: &[&str] = &["json", "jsonl", "csv", "tsv", "toml", "yaml", "yaml_all", "env", "lines", "text"];

fn workdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("okrun-fuzz-data-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    dir
}

fuzz_target!(|data: &[u8]| {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(workdir);
    let Some((&sel, body)) = data.split_first() else { return };
    let format = FORMATS[sel as usize % FORMATS.len()];
    let gzip = sel & 0x80 != 0;
    let file = if gzip { "d.bin.gz" } else { "d.bin" };
    std::fs::write(dir.join(file), body).unwrap();
    let src = if format == "yaml_all" {
        format!("let v = parse(read(\"{file}\", \"text\"), \"yaml_all\")\nlet out = to(v, \"json\")\n")
    } else {
        format!("let v = read(\"{file}\", \"{format}\")\nlet out = to(v, \"json\")\n")
    };
    let mut interp = okrun::interp::Interp::new(Vec::new());
    let _ = interp.run_source("fuzz.ok", &src, dir.clone());
});
