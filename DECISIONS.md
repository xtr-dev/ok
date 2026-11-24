# Decisions

A dated log of design decisions that the project brief (`PROMPT.md`) left
open. One paragraph each, with the reason. Newest entries go at the bottom of
their date. When a decision is reversed, add a new entry that says so; do not
edit the old one.

## 2026-11-24 (M0)

**Repository: one published package with a library and a thin binary.** The
crate `ok-lang` contains `src/lib.rs` (the language) and `src/main.rs` (the
`ok` binary, a few lines). A separate binary crate would force publishing two
crates to crates.io for no gain. Test-only helpers (fixture commands, the
fuzz targets) get their own unpublished crates when they arrive.

**Toolchain pinned to Rust 1.90.0, edition 2024.** `rust-toolchain.toml` pins
the version used locally and in CI so that clippy and rustfmt agree
everywhere. It is bumped deliberately, in its own commit.

**Licence tooling: cargo-deny and cargo-about.** cargo-deny rejects copyleft
licences (MPL included, since it is file-level copyleft) and unknown
sources; cargo-about generates `THIRD_PARTY.md` from a template. CI fails if
`THIRD_PARTY.md` is out of date.

**`and`, `or`, `not` instead of `&&`, `||`, `!`.** Words read correctly for
people who do not know C-family syntax, and `!` is already used by `run!`.
The lexer recognises `&&`, `||` and `!x` and suggests the words.

**No truthiness.** Conditions must be `bool`. Python and JavaScript disagree
about whether `[]` is false, so any truthiness rule misleads half the
readers. The error message suggests the explicit test.

**`/` on two ints gives a float; no integer-division operator.** `7 / 2 ==
3.5` is what most readers expect. `//` would read as a comment to JavaScript
users, so floor division is spelled `floor(a / b)`.

**Value semantics for lists and records.** Assigning or passing a list never
creates shared mutable state; `var` bindings can be updated element-wise,
which rebinds to a modified copy (copy-on-write in the implementation). This
removes aliasing bugs from scripts, the place they hurt most.

**Method syntax is uniform function call syntax.** `x.f(a)` calls a record's
function field `f` if there is one, else `f(x, a)`. The brief uses both
styles (`trim`, `.contains(...)`, `.abs()`), and UFCS makes them the same
thing, so there is one set of functions to learn.

**Implicit closures cover whole expressions.** `.age >= 18 and .active` is one
closure, bounded by the nearest argument. This is what the brief's
`where .contains("ERROR")` suggests a reader expects, and extends it to
comparisons without new syntax.

**Juxtaposed calls only in pipeline stages.** `| where .active` and `| take
5` read naturally; allowing `print "x"` everywhere would make `f (x)` and
`f(x)` mean different things across the language. Restricting it to stages
keeps it unambiguous.

**Arrow closures (`x => expr`) plus `fn(x) { }`.** Arrows are the short form
for pipelines; `fn` is the block form. The arrow body stops at `|` so that
`| map x => x * 2 | sum` groups as it reads.

**Pipe has the lowest precedence.** `let n = xs | count` works without
parentheses. Comparing a pipeline result needs parentheses, and the error for
`xs | count > 3` says so.

**`??` and `?` for nulls and failures; no optional chaining.** `x? ?? d` covers
the common cases with two small rules. `?.` would add a third rule for
readers to learn.

**Triple-quoted strings with dedent.** Bash scripts use heredocs for
multi-line text; rewriting them needs a readable equivalent. The dedent rule
is the one Swift and Java text blocks use.

**Format specs in interpolation (`{x:>8}`).** Report-style scripts align
columns constantly. Python's mini-language subset is widely known, and
`{fmt(x, ">8")}` with nested quotes is harder to read.

**A `time` value kind.** The brief lists the value kinds; `time` is added
because `now()`, `stat().modified` and backup rotation need instants that
subtract to durations (`now() - f.modified > 30d`). Representing times as
integers would make that arithmetic error-prone. Flagged for review in the M0
report.

**Command capture is decided by syntactic position.** A `run` whose value is
unused (an expression statement) streams live; anywhere else it captures.
This is the brief's rule ("with `let r =` they are captured") made precise,
and it does not depend on runtime behaviour. A `run` as the last statement of
a function body is still live, so `fn build() { run cargo build }` shows
cargo's output.

**Captured output has trailing newlines stripped.** `(run git rev-parse
HEAD).out` is used directly as a value far more often than byte-exact output
is needed; `bytes: true` gives the raw bytes.

**Command options use `with {...}` in command mode and named arguments in the
function form.** A trailing `{...}` would collide with `{}` interpolation.
`with` is only special when followed by `{`, so `run echo with care` still
passes `with` as an argument.

**Redirect targets are one command-mode word.** `> build.log`, `> "a b.txt"`
and `> {out_dir / "dump.sql"}` all work. Treating the target as a general
expression would make `> build.log` an undefined-variable error, and would
make `run a < "in" | lines` ambiguous.

**No stderr redirection operators in v1.** Capturing (`let r = run ...`) and
`run!` cover the usual `2>/dev/null` and `2>&1` uses; adding `2>` would bring
in more shell syntax. Logged in IDEAS.md.

**Shell habits are errors with fixes.** `&&`, `;`, `FOO=1 cmd`, `$(...)` and
unquoted `(` in commands are rejected with an explanation, rather than being
passed as literal arguments that silently do the wrong thing.

**`.bat`/`.cmd` files are started through CreateProcess with Rust's hardened
batch quoting.** Windows cannot start a batch file without the batch
interpreter; that is the OS loading a script, not ok building a shell command
line. Arguments that cannot be quoted safely for a batch file are refused.

**`--dry-run` also skips mutating file and network operations.** A dry run
that skips `run` but still executes `remove(dist)` would be dangerous. Reads
still happen so the script can make decisions.

**Paths display with `/` everywhere and convert to native form at the
boundary.** Golden tests and logs are identical across OSes. `\` is a
separator only on Windows, so Unix file names containing `\` stay
representable. `..` is resolved lexically after a normal component, because
readers expect `a/b/../c` to be `a/c`.

**`path / absolute` gives the absolute path.** Same as Python's pathlib and
Rust's `Path::join`; it lets `cwd / user_input` accept absolute input.

**`remove` succeeds on missing paths and refuses dangerous roots.** It
behaves like `rm -rf` (returning whether anything was removed), but never
removes `/`, the home directory or the project root.

**`copy(src, dst)` never means "copy into".** The destination is always the
exact path. `cp`'s "into a directory if it exists" rule makes scripts behave
differently on the second run.

**`hash` rejects plain strings.** `hash("file.txt")` could mean the file or
the text; requiring `path()` or `bytes()` removes the guess.

**`fetch` raises on HTTP status 400 and above by default.** Consistent with
`run` failing on a nonzero exit. `check: false` returns the response instead.

**Lazy values memoise on second use.** Streaming is guaranteed for a pipeline
that consumes a `read` directly; a lazy value bound to a variable and used
twice is buffered so both uses see the same data. Re-reading the file would
make values change under the script.

**`read` of `.txt` gives text; `lines` streams it.** The brief's `read("log.txt")
| lines` keeps working and `read("notes.txt")` gives the whole text, as
readers expect. The `lines` format is available explicitly.

**`group` returns a record keyed by `str(key)`.** Records print, serialise and
iterate naturally (`for level, items in groups`). Integer keys become strings,
which is the JSON behaviour.

**`uniq` does not require sorted input.** It keeps first occurrences, which is
what people usually mean; Unix `uniq`'s adjacency rule is a frequent bug.

**`first`/`last`/`min`/`max` of an empty list are `null`.** Combined with `??`
this reads well (`xs | max ?? 0`); indexing stays strict.

**YAML: 1.2 core schema; library chosen in M2.** `serde_yaml` is archived, so
M2 picks a maintained permissively licensed parser (candidates:
`yaml-rust2`, `saphyr`).

**CSV writing takes the header from the first record.** Collecting the union
of keys would force materialising a lazy list; extra keys in later records are
errors so data is never silently dropped.

**`.env` files have no variable expansion.** Implementations disagree about
`${X}` syntax and escaping; not expanding is predictable.

**Libraries are declaration-only.** Any imported file may contain only `fn`,
constant `let` and `import`. Importing never runs code, in scripts or
projects, so import order cannot matter.

**Flag parameters are keyword-only.** `--name` parameters are passed as named
arguments in code and as `--name` on the command line, so a signature reads
the same in both places. `--dry-run` binds `dry_run`.

**No short flags in v1.** Declaring them needs extra syntax; logged in
IDEAS.md.

**`requires "0.3"` is a minimum version.** Caret semantics would make a
project refuse to run on 0.4, which surprises readers of the line.

**`env file` only in the root `ok.ok`, missing file ignored.** `.env` files
are usually untracked, so a fresh clone must still work. Process environment
wins over file values (the dotenv convention). `--env NAME` files must exist
because the user asked for them explicitly.

**`needs` resolves names in the same namespace first, then the root.**
Namespaced tasks usually depend on siblings. Dependencies run sequentially,
depth first, in listed order; parallelism is in IDEAS.md.

**All confirmations are asked before anything runs.** A dependency chain
should not do half its work and then ask.

**Up-to-date checks use modification times only in v1.** The brief makes a
content-hash mode optional; mtimes match Make and need no state file.

**Mounted sub-projects expose only their root tasks.** Keeps the one-level
namespace rule of v1; mounted tasks run in the sub-project's directory
because that is where their author expects them to run.

**Trust is content-addressed and recorded per root.** A fingerprint that was
trusted anywhere is trusted; the per-root snapshot exists to show a diff when
the content changes. Moving a project to a new directory therefore does not
re-prompt if its content is unchanged, and any change does.

**`--trust`/`OK_TRUST=1` do not record trust.** CI runs should not grow a
trust store; trust is recorded only by an explicit `ok trust` or the `t`
answer.

**Exit code 2 for usage errors, 1 for everything else ok raises.** Child
command failures keep the child's code, timeouts use 124 like `timeout(1)`.

**Release artefacts are raw binaries plus `SHA256SUMS`.** The bootstrap
wrappers then need no archive tool; `THIRD_PARTY.md` and `LICENSE` are
attached next to the binaries. The release workflow only creates a draft.

## 2026-11-24 (M0 review)

**The published package is named `okrun`.** Requested by Bas; the name is free
on crates.io. The library is `okrun`, the binary stays `ok`. This replaces the
`ok-lang` name used earlier today.

**Toolchain pinned to Rust 1.100.0.** Bumped from 1.90.0 to the current
stable release, which also lets the current cargo-deny run locally.

**A `quiet` option silences a command.** Requested in review: a `run` statement
streams its output live, and some commands (downloads, `cargo fetch`, `npm ci`)
are only noise. `with {quiet: true}` captures and discards stdout and stderr,
but a failure still reports the stderr tail, so silencing never hides why
something broke.

## 2026-11-24 (M1)

**Interpreter layout: one module per stage.** `lexer` (including command
mode), `parser`, `ast`, `interp` (evaluator and module loading), `value`,
`path`, `format` (format specs), `builtins`, `error`, `source`, `cli`. Nested
syntax (interpolations, command words) is lexed into nested token lists so
the parser never re-scans text.

**The interpreter runs on a thread with a 256 MiB stack, and calls nest at
most 2000 deep.** Default main-thread stacks differ per OS (1 MiB on Windows,
8 MiB on Linux), so without this the same script could overflow on one OS and
not another. The stack is reserved virtual memory, not allocated up front.
The parser limits expression nesting to 150 and the lexer interpolation
nesting to 32, so no input can crash them.

**Errors are not boxed in `Result`s.** Clippy's `result_large_err` is allowed
crate-wide: errors are the cold path, and boxing them everywhere would add
noise across the interpreter for no measurable gain.

**Records compare equal regardless of key order.** Two records with the same
keys and values describe the same data; insertion order matters for display
and serialisation, not identity.

**`round` rounds halves away from zero.** It is what most people learned at
school and what Rust, JavaScript's `Math.round` for positives, and spreadsheets
do; Python's banker's rounding surprises readers.

**Printing to a closed stdout ends the script quietly with exit code 0.** A
script piped into `head` should stop, not report an error the user caused on
purpose.

**Until M2, `range` builds a list and refuses more than ten million
numbers.** Lazy ranges belong with the lazy lists of M2; the cap keeps a typo
from exhausting memory in the meantime.

**Until M4, `main` runs only when it needs no arguments.** Command-line
parsing from the signature is M4's job; a script whose `main` needs arguments
reports that the feature is not implemented yet instead of guessing.

**Unimplemented features are reported, never silently skipped.** `run`,
project tasks, trust and most `ok self` commands parse, and evaluating them
gives "not implemented yet (milestone Mx)".

**Fuzzing: a cargo-fuzz crate outside the workspace plus an in-tree random
test.** cargo-fuzz needs nightly, so `fuzz/` is excluded from the workspace
and runs in its own CI job. Its dependency `libfuzzer-sys` is MIT/Apache-2.0
and NCSA (permissive); it is never linked into the shipped binary. A
deterministic random-input test in `tests/no_panic.rs` runs in every build.

**Help for names from other languages.** Unknown functions such as `length`,
`push` or `toUpperCase` get a pointer to the ok equivalent, in addition to the
edit-distance suggestions.

**GitHub Actions only run when started by hand, for now.** Requested by Bas
during M1: checks run locally instead. The workflows stay in the repository
with a `workflow_dispatch` trigger so they can be run on demand or switched
back on by restoring their triggers. The cost is that macOS and Windows
behaviour is not verified while CI is off.

## 2026-11-24 (M2)

**Format crates: serde_json, csv, toml and yaml-rust2.** All MIT/Apache-2.0
(csv is also Unlicense), well maintained, and pure Rust. serde_json and toml
use their `preserve_order` features so records keep the document's key order.
yaml-rust2 replaces the archived serde_yaml and implements the YAML 1.2 core
schema (`yes` and `no` stay strings). `.env` parsing is hand-written; no crate
matched the no-expansion rule.

**Lazy values are materialised when bound, stored or passed to a
non-streaming function, instead of being memoised on second use.** The M0
spec said a lazy value used twice keeps what was read so far. Materialising
at the boundary gives the same results with a much simpler rule ("a pipeline
streams; a variable holds the whole thing"), and it means laziness can never
leak into code that does not expect it. SPEC.md 13.2 now says so.

**A juxtaposed pipeline stage takes exactly one argument.** With
comma-separated juxtaposed arguments, `print(xs | count, ys | count)` parsed
as `count(xs, ys | count)`; readers expect two arguments to `print`. Stages
needing more arguments use parentheses, as all M0 examples already did.

**`write` refuses a likely argument swap.** `write("out.txt", "hello")` would
silently create a file named `hello`. When the value is a short string with
a known data-file extension and the path has none, `write` stops with a hint.
A real need to write such a string to an extension-less file can pass the
format explicitly or use a path value: `write("out.txt", path("hello"),
"text")` still triggers it, so the check is narrow by design and logged as a
known corner.

**YAML merge keys are applied.** `<<: *defaults` is technically YAML 1.1,
but CI and compose files rely on it everywhere; reading them without merging
would give surprising records.

**JSON integers beyond 64 bits become floats.** Matches SPEC.md 13.3; such
values (IDs from other systems) print without an exponent, as Rust formats
them, which keeps them readable.

**A `{` inside a double-quoted string gets a dedicated hint.** JSON written
in double quotes is the most common way to trip over interpolation, so the
error says to escape the brace or use a single-quoted string.

## 2026-11-25 (M3)

**Commands use `std::process` directly; program lookup is ok's own.** Rust's
`Command` does not search `PATHEXT` on Windows, so ok resolves the program
itself (PATH, then PATHEXT on Windows; never the current directory; on Unix
only executable files) and passes the full path. Batch files are then run by
the standard library, whose argument quoting for `.bat`/`.cmd` was hardened
for CVE-2024-24576 and refuses arguments it cannot pass safely.

**Exit codes 127 and 126 for "not found" and "cannot start".** The same
codes shells use, so a CI log reads the same as before the switch to ok.

**`env.set` keeps an overlay instead of changing the process environment.**
`std::env::set_var` is `unsafe` in Rust 2024 because it races with other
threads, and the workspace forbids `unsafe`. The overlay is consulted by
`env.NAME` and applied to every child, which is all a script can observe.

**`run a | run b` connects the pipes directly.** The stdout of `a` becomes
the stdin of `b` without passing through ok; other values piped into a
command are written to its stdin from a helper thread.

**A pipeline that stops reading kills the command.** `run yes | take 3`
would otherwise block forever. The command's exit status is then ignored
(SPEC.md 9.5).

**Network: ureq with rustls (ring) and the platform certificate verifier.**
No OpenSSL, so the binary stays static; certificates come from the OS store,
as users expect from curl. `rustls-platform-verifier` brings in
`webpki-root-certs` (CDLA-Permissive-2.0, a permissive data licence for the
Mozilla root list), which was added to the cargo-deny allow list.

**Archives: zip (deflate only), tar and flate2, all pure Rust.** Optional zip
codecs (bzip2, zstd, AES) are off to keep the binary small and the licences
simple. Extraction checks every member path and link target itself instead
of trusting the crates.

**Times: jiff.** It reads the system time zone on every OS without extra
crates and formats with strftime patterns. Golden tests set `TZ=UTC`.

**Hashing: RustCrypto sha2, sha1 and md-5; regexes: regex; globs: globset
with ok's own directory walk.** The walk starts at the pattern's literal
prefix, stops at the pattern's depth when there is no `**`, and prunes
excluded directories.

**Dot-files in `glob`: one rule for all levels.** They are included when the
pattern has any component starting with `.` or with `hidden: true`. Matching
per component, as the M0 spec put it, would be slower and harder to explain.

**`ok-fixture`: a test-only program instead of system tools.** Tests of
`run` call `ok-fixture echo|args|cat|exit|lines|forever|env|pwd|...`, which
behaves identically on every OS. It lives in its own unpublished crate.

## 2026-11-25 (M4)

**Arguments are parsed before any code runs.** A script with `main` gets its
command line checked against the signature first, so a typo in a flag exits
with code 2 before the top-level statements have done anything.

**`--help` intercepts only before `--`.** `ok tool.ok -- --help` passes
`--help` to the script as a positional value.

**Relative `path` arguments resolve against `invocation_dir`.** In script
mode that is the working directory, so the value stays relative and prints
as typed; in project mode, where tasks run in the project root, the path is
made absolute so it still means what the user typed.

**`list[T]` flags default to `[]`.** A repeatable flag that is never given is
an empty list, not a missing-argument error.

**`--flag=value` and `--flag value` are both accepted; switches accept
`--flag`, `--no-flag` and `--flag=true|false`.**
