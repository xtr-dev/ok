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
