//! A deterministic, dependency-free stand-in for fuzzing that runs in every
//! CI build: random token soup and random mutations of the example programs
//! go through the lexer and parser, which must never panic. The cargo-fuzz
//! targets in `fuzz/` explore further.

use std::path::Path;

/// xorshift64*: small, deterministic, good enough to shuffle test input.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[rustfmt::skip]
const PIECES: &[&str] = &[
    "let ", "var ", "fn ", "task ", "if ", "else ", "for ", "in ", "while ", "match ", "try ", "catch ", "run ", "run! ", "run(",
    "import ", "return ", "{", "}", "(", ")", "[", "]", ",", ":", ".", "...", "|", "=>", "?", "??", "=", "==", "+", "-", "--", "*",
    "/", "%", "<", ">", ">>", "\"", "'", "\"\"\"", "'''", "\\", "{x}", "{...xs}", "\n", " ", "\t", "#", "x", "name", "1", "1.5",
    "5s", "1h30m", "0x", "\"a{b}c\"", "'raw'", "with {", "not ", "and ", "or ", ".field", "é", "\u{1F600}", "\r\n", "$(", "&&", ";",
];

fn check(src: &str) {
    let _ = okrun::lexer::lex(0, src);
    let _ = okrun::parser::parse(0, src);
}

#[test]
fn random_token_soup_never_panics() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let n = rng.below(40);
        let src: String = (0..n).map(|_| PIECES[rng.below(PIECES.len())]).collect();
        check(&src);
    }
}

#[test]
fn mutated_examples_never_panic() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut sources = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read dir") {
            let p = entry.expect("entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "ok") {
                sources.push(std::fs::read_to_string(&p).expect("read"));
            }
        }
    }
    let mut rng = Rng(42);
    for src in &sources {
        let chars: Vec<char> = src.chars().collect();
        for _ in 0..300 {
            let mut c = chars.clone();
            for _ in 0..1 + rng.below(4) {
                if c.is_empty() {
                    break;
                }
                let i = rng.below(c.len());
                match rng.below(3) {
                    0 => {
                        c.remove(i);
                    }
                    1 => c.insert(
                        i,
                        PIECES[rng.below(PIECES.len())]
                            .chars()
                            .next()
                            .unwrap_or('x'),
                    ),
                    _ => {
                        let j = rng.below(c.len());
                        c.swap(i, j);
                    }
                }
            }
            check(&c.iter().collect::<String>());
        }
        // Every prefix of a real program, which exercises unterminated input.
        for cut in (0..chars.len()).step_by(7) {
            check(&chars[..cut].iter().collect::<String>());
        }
    }
}

/// The `.ok` files under `dir` (recursively), sorted so runs are
/// reproducible.
fn ok_files(dir: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read dir") {
            let p = entry.expect("entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "ok") {
                paths.push(p);
            }
        }
    }
    paths.sort();
    paths
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("read"))
        .collect()
}

/// Small edits that keep a program mostly valid, so that evaluation, not
/// just parsing, sees unusual input: swapped lines, duplicated lines,
/// changed numbers and replaced tokens.
fn mutate_lines(src: &str, rng: &mut Rng) -> String {
    #[rustfmt::skip]
    const SWAPS: &[(&str, &str)] = &[
        ("0", "-1"), ("1", "9223372036854775807"), ("2", "1e308"), ("[", "[["), ("]", "]]"), ("+", "*"),
        ("==", "<"), ("true", "null"), ("\"", "\"\\u{0}"), ("(", "(["), (")", "])"), (".", "?."), ("for", "while"),
        ("let", "var"), ("x", "xs"), ("{", "{{"), ("sort", "uniq"), ("map", "where"), ("len", "str"),
    ];
    let mut lines: Vec<String> = src.lines().map(str::to_string).collect();
    if lines.is_empty() {
        return String::new();
    }
    for _ in 0..1 + rng.below(3) {
        let i = rng.below(lines.len());
        match rng.below(4) {
            0 => {
                let j = rng.below(lines.len());
                lines.swap(i, j);
            }
            1 => {
                let l = lines[i].clone();
                lines.insert(i, l);
            }
            _ => {
                let (from, to) = SWAPS[rng.below(SWAPS.len())];
                if let Some(at) = lines[i].find(from) {
                    lines[i].replace_range(at..at + from.len(), to);
                }
            }
        }
    }
    lines.join("\n")
}

/// Generated programs that parse are evaluated in the sandbox (no files,
/// commands or network; a step budget ends endless loops). Evaluation must
/// end in a value, an ok error or an exit, never a panic or a crash. The
/// cargo-fuzz target `eval` explores the same ground further.
#[test]
fn mutated_programs_evaluate_without_panics() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = ok_files(&root.join("examples"));
    sources.extend(ok_files(&root.join("tests/cases")));
    // Each regression below once crashed the interpreter (stack overflow,
    // abort on allocation, or a panic in sorting).
    #[rustfmt::skip]
    sources.extend([
        "let rows = range(40) | map(fn(i) { {age: if i % 7 == 3 { null } else { i } } })\nprint(sort(rows, .age))",
        "print(pad(\"x\", 3000000000))",
        "var x = []\nfor i in range(100000) { x = [x] }\nprint(to(x, \"yaml\"))",
        "print(fmt(1.7e308, \",.2%\"))",
        "fn f(n) { return f(n + 1) + 0 + 0 + 0 + 0 + 0 + 0 + 0 + 0 + 0 + 0 + 0 + 0 }\nf(0)",
        "while true { }",
        "var s = \"ab\"\nwhile true { s = s + s }",
    ].map(str::to_string));
    let mut rng = Rng(7);
    let mut ran = 0;
    for src in &sources {
        for round in 0..4 {
            let program = if round == 0 {
                src.clone()
            } else {
                mutate_lines(src, &mut rng)
            };
            if okrun::parser::parse(0, &program).is_err() {
                continue;
            }
            okrun::interp::run_sandboxed(&program, 20_000);
            ran += 1;
        }
    }
    assert!(ran > 300, "only {ran} programs parsed");
}

#[test]
fn the_sandbox_denies_side_effects_and_ends_endless_loops() {
    use okrun::interp::run_sandboxed;
    for (src, what) in [
        ("run echo hi", "run"),
        ("run(\"echo\", \"hi\")", "run"),
        ("write(\"x\", \"f.txt\")", "write"),
        ("print(read(\"/etc/hostname\"))", "read"),
        ("import \"lib.ok\"", "import"),
        ("fetch(\"https://example.com\")", "fetch"),
        ("sleep(1s)", "sleep"),
        ("print(now())", "now"),
        ("print(hash(path(\"/dev/zero\")))", "hash"),
    ] {
        let r = run_sandboxed(src, 1000);
        assert!(
            r.outcome
                .contains(&format!("`{what}` is not available in the sandbox")),
            "{src}: {}",
            r.outcome
        );
    }
    let r = run_sandboxed("print(1)\nwhile true { }", 1000);
    assert_eq!((r.output.as_str(), r.outcome.as_str()), ("1\n", "exit 125"));
    let r = run_sandboxed("print(len(range(1000000000)))", 1000);
    assert_eq!(r.outcome, "exit 125");
    let r = run_sandboxed("while true { try { while true { } } catch e { } }", 1000);
    assert_eq!(r.outcome, "exit 125");
    let r = run_sandboxed("var s = \"ab\"\nwhile true { s = s + s }", 100_000);
    assert!(r.outcome.contains("too large"), "{}", r.outcome);
    let r = run_sandboxed("var s = [1]\nwhile true { s = s + s }", 100_000);
    assert!(r.outcome.contains("too large"), "{}", r.outcome);
}
