# Decisions

A dated log of design decisions that the project brief left open
(the brief was `PROMPT.md`, removed before the first public release; it is
in the git history). One paragraph each, with the reason. Newest entries go at the bottom of
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
wrappers then need no archive tool; `THIRD_PARTY.md` and the licence files are
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

## 2026-11-25 (M5)

**Tasks run with the process working directory set to the project root.**
Changing ok's own working directory (rather than only passing `cwd` to
commands) makes every relative path in a task, from `read` to `glob` to
`run`, mean the same thing. Mounted sub-project tasks switch to their
directory for their duration.

**A task's declarations load before its dependencies run, but only after
trust and confirmations.** Order: parse everything, pick the task, parse its
arguments, check trust, ask confirmations, load env files, evaluate
declarations, run the plan. Nothing a project contains runs before the user
approved it.

**`ok NAMESPACE` alone lists the namespace.** It is the natural "what can I
run here" question and costs nothing.

**`ok self fmt` changes whitespace only.** A formatter that rebuilds code from
the syntax tree would need comment preservation everywhere; one that only
re-indents cannot change a program's meaning. Every example is kept in this
format by a test.

**`ok self check` checks structure, not undefined names.** A sound name
check needs the full scope rules (hoisting, imports, implicit closures, match
bindings) and false positives would erode trust in it; it is left for a
later version and logged in IDEAS.md.

**The version moved to 0.1.0 during M5.** Project files say `requires
"0.1"`, which a 0.0.0 binary refuses; the version is the one being built.

## 2026-11-25 (M6)

**The trust store is JSON plus snapshot directories.** `trust.json` lists
entries (root, mode, fingerprint, time); `snapshots/<fingerprint>/` holds the
trusted files for diffs. Both are plain files a user can inspect or delete.

**The diff is computed in ok, not by a crate.** Project files are small, and
a longest-common-subsequence line diff with three lines of context is about
fifty lines of code; it avoids a dependency for one prompt.

**`OK_CONFIG_DIR` overrides the store location.** Tests need an isolated
store, and users with unusual setups can point it elsewhere.

**An empty answer to the trust prompt means no.** Pressing Enter must never
run someone else's code.

## 2026-11-25 (M7)

**`ok self init` pins checksums from the release's `SHA256SUMS`.** A binary
cannot contain its own checksum, so `init` downloads the list published next
to the binaries (or reads it with `--checksums FILE`) and writes the hashes
into the wrappers. The wrappers verify before running and never run a
mismatching file.

**`ok.cmd` delegates to `ok.ps1`.** All logic lives in one PowerShell file;
the `.cmd` file only exists so that `ok.cmd build` works from cmd.exe.

**The examples run in the test suite with fake tools.** `ok-fixture` copied
as `git`, `docker`, `psql`, ... logs each call and answers from per-case
rules, which shows exactly which commands an example runs and makes the
output identical on every OS. Examples that call fixed internet hosts are
parsed but not run.

**`v0.1.0` is tagged locally only.** Pushing a tag starts the release
workflow once CI is on again, and publishing is Bas's call.

## 2026-11-25 (after v0.1.0)

**`fetch` and the archive functions stay built in; ok does not delegate them
to `curl`, `wget`, `zip` or `tar`.** Bas weighed the cost of keeping that code
tested and secure against delegating to system tools. Delegating would make
scripts depend on what each host has installed: Windows and many Linux
systems have no `zip`/`unzip`, and GNU tar, bsdtar and different `curl`
versions behave differently. Error reporting, the archive path checks and
`--dry-run` would also be lost. Scripts that prefer the system tool can
still use `run curl ...` or `run tar ...`. The price is that the network and
archive dependencies need watching: a security advisory for rustls, ureq,
zip, tar or flate2 means a patched ok release (see CLAUDE.md, Security
upkeep).

**CI runs automatically again.** Bas switched the push and pull-request
triggers back on, so every push to `main` is checked on Linux, macOS and
Windows. The release workflow still runs only when started by hand.

**The licence is MIT OR Apache-2.0 instead of CC0 1.0.** Bas chose it before
the first release. It is the usual licence of Rust crates, is OSI-approved,
and, unlike CC0, does not exclude patent rights (Apache-2.0 grants them
explicitly), which some companies and distributions require. The texts are
`LICENSE-MIT` and `LICENSE-APACHE`; `PROMPT.md` still mentions CC0 as the
original brief. Dependencies under CC0 stay allowed in `deny.toml`.

**`PROMPT.md` is removed from the repository.** Bas removed the original
brief before making the repository public: its setup notes were out of date
(crate name, licence), and everything that still matters is in `SPEC.md`,
`DECISIONS.md` and `CLAUDE.md`. It stays readable in the git history.

**A landing page and one-line install scripts on GitHub Pages.** `site/`
holds a static page and `install.sh` / `install.ps1`, served from
https://xtr-dev.github.io/ok/. Like the bootstrap wrappers, the install
scripts are a place where shell is acceptable: they only download the
latest release binary, check it against that release's `SHA256SUMS`, and
put it on the PATH. The Pages workflow runs both scripts on Linux, macOS and
Windows before publishing, so a broken script never goes live.

## 2026-11-26 (M9)

**stderr redirects are `2>`, `2>>` and `2>&1`, recognised only as a word
start.** These are the forms people already type, so they read without
explanation; the M0 decision to leave them out is reversed now that the
owner approved them. Only an unquoted `2` directly followed by `>` at the
start of a word is a stream number, so `run echo 2 > x` keeps passing the
argument `2` and `x2>y` passes `x2`. Other shell descriptor forms (`>&2`,
`1>`, `&>`, `3>`) stay errors or plain words rather than growing a
descriptor syntax nobody asked for.

**`2>&1` means "stderr goes where stdout goes", independent of order.** In a
shell, `2>&1 > f` and `> f 2>&1` differ, which is a classic trap. ok treats
redirects as a set of destinations, so both send everything to `f`. When ok
reads stdout itself (captured or piped), stdout and stderr share one pipe,
so the interleaving is the one the command produced.

**A stream takes at most one redirect.** `> a > b` silently used the last
target before; now it is an error with a span, like `2> e.log 2>&1`, because
a second redirect of the same stream is always a mistake.

**Failures of commands with redirected stderr still explain themselves.**
With `2>&1` and captured output, the error shows the last 20 lines of the
merged output (labelled `output`, and given as the error record's `stderr`);
when stderr went to a file, the error names the file. A failure should never
leave the reader without a place to look.

**No stderr options in the function form.** `run(...)` has no `>` either;
command mode with `{...argv}` expresses every function-form call, so a
second spelling (`stderr: path`, `merge: true`) would add a way to say the
same thing without making any script clearer.

**Independent dependencies run in parallel by default, limited by
`--jobs N`.** The default is the number of CPUs ok may use; `-j1` restores
one-at-a-time running. Parallel by default is what makes `needs [lint,
test, build]` fast without anyone having to know a flag, and `-j1` is the
familiar spelling from make and cargo for the rare project that needs order.
Entries of one list are not ordered against each other; a task that needs
another first says so with its own `needs`.

**Parallel dependencies run in worker processes, not threads.** The
interpreter is a single-threaded tree-walker with `Rc` scopes, and a running
task owns process-wide state: the working directory (tasks `cd` into their
project or mounted sub-project), stdout and the terminal. A second `ok`
process started with an internal option gets all of that separately for
free, prints its own errors with its own source map, and needs no change to
the interpreter or a new dependency. Starting one costs a few milliseconds
and a re-parse of the project, which is small next to the commands a task
runs.

**Whether an instance runs in a worker is decided from the graph.** An
instance that is ordered against every other one (a chain, a single
dependency) runs in ok's own process, unprefixed and with the terminal,
exactly as in v0.1.0; only instances that really can overlap go to
workers. Deciding by timing (for example "in process when nothing else
happens to run") would make the output format and terminal access change
from run to run.

**Worker output is prefixed line by line, not buffered per task.** Each line
appears as `LABEL | line` as soon as it is complete, with labels padded to
the same width, which is the docker compose convention many readers know.
Buffering each task's output until it finishes would keep blocks together,
but long builds would show nothing for minutes and a hanging task would
show nothing at all.

**Tasks that run in parallel have no terminal for input.** Their stdin is
empty, so `prompt()` takes its default or fails and `confirm()` needs
`--yes`, with a note naming `--jobs 1`. Serialising questions across
processes would need a protocol between ok and its workers and would still
let other tasks' output interrupt a question; failing clearly can never
deadlock or garble the terminal. `confirm "text"` modifiers are unaffected,
since all of them are asked before anything runs.

**After a failure ok waits for the running workers instead of killing
them.** Killing an ok worker would not stop the commands it started (a
child process outlives its parent on every OS ok supports), and interrupting a command halfway can
leave files half written. No new task starts after the first failure, the
running ones finish with their output shown, and ok then reports the first
failure with that task's exit code.

**`--dry-run` always runs dependencies one at a time.** A dry run is read
as a plan, and a plan whose lines come out in a different order every time
is hard to read or compare.

**Workers check trust again, without asking, and the fingerprint.** A
worker runs the normal trust check non-interactively (the parent's approval
is in the store, or `--trust` is passed on), so starting a worker by hand
needs exactly the trust a normal run needs. The parent also passes the
fingerprint of the files it checked; a worker that reads different content
refuses to run, so only approved content runs although workers read the
files again.

**Dependency arguments are named, constant, and checked twice.**
`needs [build(release: true)]` uses the named-argument syntax of calls,
including for positional parameters, because a dependency list is read
far from the task's signature and a bare `true` would not say what it
means. Values are constant expressions, so loading and planning never run
project code. Names and required parameters are checked at load time with
spans; types are checked when the invocation is planned, before anything
runs, with the span of the value.

**An instance is a task plus the arguments given, compared as values.** The
same task with equal arguments runs once; different arguments run again,
which is what a build matrix (`build(target: "linux")`, `build(target:
"wasm32")`) needs. Defaults are not filled in for the comparison:
`build` and `build(release: false)` are two instances. Filling them in
would require evaluating parameter defaults, which may be any expression,
before the task runs; "the arguments as written" is easy to predict.

## 2026-11-26 (M10)

**`finally` is a keyword, and `try` takes `catch`, `finally`, or both in
that order.** A contextual word would have to be recognised after a closing
brace on the next line, which makes a statement starting with a variable
named `finally` change meaning; a keyword is unambiguous, and like every
keyword it still works as a field name and record key (`r.finally`). The
fixed order `catch` then `finally` matches how the blocks run.

**`finally` always runs, including on `exit()`; an error in `finally`
replaces the pending outcome.** Cleanup code (removing a lock file, stopping
a container) is written in `finally` precisely so that it runs on every way
out, and scripts often call `exit(code)` from helpers, so skipping
`finally` on `exit` would leak exactly what it exists to clean up. Temporary
files from `tempdir()`/`tempfile()` are removed at exit anyway (12.8), after
`finally` blocks have run. When `finally` fails, its error is the one that
continues, because it is the most recent problem and the one the reader of
the `finally` block can act on; the error it replaced is kept as a note so
nothing is lost silently. Ctrl-C still ends ok at once.

**`return`, `break` and `continue` cannot leave a `finally` block.** In
languages that allow it, a `return` in `finally` silently swallows the
error being propagated, which is a classic source of hidden failures. A
syntax error with a hint is clearer; inside a loop or function nested in
the `finally` block they work as usual. The value of a `finally` block is
discarded: the `try` expression yields the value of its `try` or `catch`
block, so adding cleanup never changes a result.

**`match` gets list and record patterns that test instead of fail.** They
use the shapes of `let` destructuring (7.1) so a reader recognises them, but
where `let` raises an error on a missing key or a wrong length, a pattern
simply does not match and the next arm is tried. Record patterns allow extra
keys, because records from JSON, `fetch` or `run` carry more fields than an
arm cares about. `..._` ignores the rest of a list, reusing `_` as in other
patterns instead of adding a bare `...`. Keys may be strings, as in record
literals, so headers like `"content-type"` can be matched.

**Every alternative of `|` must bind the same names, and a name is bound
once per pattern.** Otherwise the arm's body could refer to a name that the
matching alternative did not bind, which would only show up at run time.
This also applies to top-level alternatives such as `"a" | x =>`, which
were accepted before but could never be used safely; no example or test
used them. Exhaustiveness is not checked: a `match` with no matching arm
stays a run-time error that shows the value, as in 6.6.

**Choice types are ordinary type annotations, allowed on every parameter.**
`"admin" | "user"` is checked on calls of any `fn` like `int` or `path`, so
a helper that receives the value from `main` can declare the same type and a
typo in code fails at the call. ok has no annotations on `let`, so there is
no `let x: "a" | "b"`; adding annotations to bindings would be a separate
language change. Choices are plain string literals, case-sensitive and
without duplicates. Optional choices are written with a final `| null`
rather than `?`, because `"a" | "b"?` reads as if only `"b"` were optional.
A string literal default outside the choices is a load-time error, since
`--help` would otherwise advertise a default that fails.

**Choices show as the value placeholder of a flag (`--role <admin|user>`),
and as `[choices: ...]` for a positional argument.** The placeholder is
where a reader looks for what to type; a positional argument keeps its name
in the usage line because the name says what it is. A wrong value is an
argument error (exit code 2) listing every choice, with a suggestion from a
unique prefix (`prod` for `production`) or else from edit distance.
Completion offers the choices after a choice flag, after `--flag=`, and for
the next positional parameter.

**Short flags are declared after the long name and a comma:
`--verbose, -v: bool`.** This is the order and punctuation of the Options
column in `--help` and of most tools' documentation (`-v, --verbose`, with
the readable name first in a signature), so it reads as one parameter with
two spellings. An annotation or a separate declaration would put the two
names apart. A positional parameter cannot start with `-`, so the parser
never confuses the short form with the next parameter. The short form is
for the command line only; code still passes `verbose: true`.

**Switches group (`-vq`); a short flag with a value stands alone
(`-o out.json`).** Grouping switches is what users type out of habit. Values
glued to the letter (`-oout.json`) or grouped behind switches (`-vo x`) are
hard to read in a command line, so they are argument errors that show the
right form instead of being guessed. Before `--`, any argument that starts
with `-` and a letter is now a flag, as in other tools; `-5` and `-` stay
values, and other values that start with a dash go after `--`. This changes
scripts whose positional arguments started with `-` and a letter, which
used to be accepted as values; the error says to use `--`.

**`-h`, `-C` and `-e` cannot be declared as short flags.** `-h` prints help
everywhere, and ok's own `-C` and `-e` are kept free so a short option has
one meaning on any ok command line, and the "ok's own options go before the
name" hint keeps working. ok's other options have no short form, so their
long names may still be declared by tasks as before. A short form on a
`bool` flag that defaults to `true` is rejected too, because it could only
repeat the default. The usage line keeps the long names, which say what a
flag does; `--help` lists the short forms, and completion offers them only
after a single `-`, so the default candidate list stays readable.

**Compression is chosen by the `.gz` name, the format by the name without
it, and `gzip:` overrides the name.** `read("events.jsonl.gz")` should mean
what it says without extra arguments, and the inner extension is the
reader's natural cue for the format. An explicit format replaces only the
format, so `read(p, "bytes")` on a `.gz` file gives the decompressed bytes,
consistent with every other format; the compressed bytes as stored (for an
upload, say) are `read(p, "bytes", gzip: false)`, and `gzip: true` covers
names that do not end in `.gz`. Detection by content was rejected because
the name already tells the reader, and a plain file that happens to start
with the gzip magic bytes must not change meaning. Only gzip is supported:
`flate2` is already a dependency for `tar`, and other formats (zstd, xz,
bzip2) would need new ones; they are noted in IDEAS.md.

**Compressed reading streams and accepts several gzip members; appending
adds a member; output is byte-identical everywhere.** Line formats stay
lazy because large logs are the main reason to keep files compressed.
Reading all members (`MultiGzDecoder`) makes `append: true` and files joined
with `cat` work. The gzip header is written with no name, time 0, OS byte
255 ("unknown") and level 6, so the same value gives the same file on every
OS, following "same output everywhere". Corrupt data is reported as
"not valid gzip data" with the decoder's reason. `stdin()`, `parse` and
`to` do not decompress: they have no file name to go by.

## 2026-11-26 (M8)

**`ok self check` reports unknown names and wrong arguments, with no false
positives.** This replaces the M5 decision to check structure only. The
checker models the interpreter's scopes exactly (hoisted functions,
imports, `let`/`var` in order, parameters with defaults that see the
parameters before them, loop and match bindings, `catch` names) and reports
a problem only when the code fails every time it runs. A check that is
sometimes wrong would be ignored, so every place where the answer depends
on run-time values is left alone (17.1). Each finding in the unit tests is
also run through the interpreter to prove it really fails.

**Code that runs later sees whole scopes; code that runs in place sees
only earlier bindings.** Function, closure and task bodies may be called
after every binding of their enclosing scopes exists, so treating names
declared further down as unknown there would be false positives; code that
runs where it stands cannot see a `let` below it, so `print(x)` before `let
x` is reported, with a help line that says so instead of a misleading
did-you-mean. The price is that a closure called before a binding it uses
is not caught.

**Errors inside `try` bodies and `?` chains are not reported.** Both turn
errors into values, and tests and scripts use them to probe on purpose
(`int(s)?`, `try { risky() } catch ...`); reporting them would flag working
code.

**Method calls are checked unless the receiver may hold a function in that
field.** `x.f()` calls a record's function field `f` before the function
`f` (6.4), and records can be passed anywhere, so the checker collects
every key that may hold a function in the file and its imports (record
literal keys with non-literal values, assigned fields, the exports of
namespace imports) and skips calls of those names; a computed key or
`from_entries` disables the check for non-literal receivers. Method names
of `env` are skipped too, since `let e = env` would make them valid. This
keeps the common `s.trim()` and `xs.where(...)` checked while records of
callbacks stay legal.

**Builtin signatures are a table next to the builtins, verified by calling
every builtin.** Rewriting the 99 builtins to declare their parameters
would touch every module for no behaviour change; instead `builtins.rs`
lists each builtin's positional range and named arguments, and a test
calls every builtin with too few, too many, the allowed and an unknown
named argument and checks the builtin's own error messages, so the table
cannot drift from the code.

**An import the checker cannot read disables the name checks of that
file.** A missing library is a sure load error and is reported; a library
that does not parse is reported when that file is checked. Either way the
library could have defined any name, and guessing would produce false
positives.

**Lints are warnings; warnings do not fail the check unless `--strict`.**
A lint flags code that runs but is very likely wrong; failing CI on a
heuristic would push people to stop running the check. `--strict` makes
warnings fail for teams that want that, mirroring `-D warnings` in Rust.
Errors exit with 1, like other load errors (2.3).

**The swapped-`write` lint needs a literal path first and no path-like
second argument.** `write("out.json", data)` is the mistake; the runtime
check only sees it when the value is a short string. Statically a string
or `path(...)` literal that looks like a file name in first place is
enough, unless the second argument also looks like one (`write("a.txt",
"b.txt")` is ambiguous), so a lint never fires on `write(text, out_path)`.
It uses the same file-name test as `write` (`formats::looks_like_file_name`).

**The formatter lays out tokens, guided by the syntax tree, and checks its
own output.** A pretty-printer that rebuilds text from the tree would have
to carry every comment, literal spelling (`0x1F`, `1_000`, raw strings) and
closure form through the tree; the lexer already keeps comments with
positions and every token's text. So the formatter re-emits the source
tokens with normalised whitespace, uses the tree to tell records from
blocks, call arguments from groups, pipeline `|` from match alternatives
and binary from prefix `-`, and before returning parses its output and
compares the tree (positions removed) and the comments with the original.
If they differ it refuses, so a formatter bug can never change a program;
tests run it over every `.ok` file of the repository, also at widths of 60,
30 and 10 to exercise line breaking everywhere. It now requires a file that
parses, where the old one only needed it to lex.

**The author's line breaks are kept; lines are only ever split.** Joining
lines would undo deliberate layouts (one-line `if` bodies, aligned
parameter lists) and make the result depend on the width in both
directions; splitting only long lines keeps diffs small and the existing
examples canonical.

**Lines are at most 100 columns of code; pipelines break first, then the
largest bracket group.** 100 is the width the examples already respect (one
line in `largest-files.ok` was reformatted). A pipeline broken before each
`|` is the style the examples use; for other lines the largest group gives
the biggest gain in one step, and the new lines are checked again. Lines
with an end-of-line comment are never broken, because a parameter's help
text is the comment at the end of its line and moving it would change
`--help`. A group holding one long string is not split, since that only
moves the string.

**Indentation counts bracket lines, not brackets.** Two brackets opened on
one line (`print(match x {`) used to indent the following lines twice;
indenting relative to the line that opened the innermost bracket gives one
level, and the closing line returns to that line's level.

**Where whitespace has meaning, the formatter keeps it.** A space before
`(` or `[` turns a call into a juxtaposed argument, around `.` it decides
between a field and an implicit closure, and `-` without spaces may be part
of `--dry-run` or a task name; there the formatter only collapses several
spaces to one. Everywhere else spacing follows the style of the examples:
spaces around binary operators and after `,` and `:`, none inside records
and brackets, spaces inside one-line blocks.

**End-of-line comments keep their column.** Aligned comment blocks (the
parameter help in the examples) stay aligned when the code before them is
reindented; when the code grows into the comment, two spaces separate them.
Own-line comments take the indentation of the code below them, which puts
a comment above a `| stage` line at the stage's level.

## 2026-11-26 (site)

**The site moves to https://okrun.dev/.** The landing page and install
scripts are served from the custom domain `okrun.dev` (GitHub Pages, HTTPS
enforced); `https://xtr-dev.github.io/ok/` redirects there. The install
one-liners in the README and on the page use the new address.

**A playground on the website runs ok compiled to WebAssembly.** ok builds
for `wasm32-wasip1` without code changes except three: the interpreter runs
on the main thread there (WASI has no threads; `.cargo/config.toml` gives it
an 8 MiB stack, beyond which the browser engine's own stack limit is reached
first), and `run` and `fetch` fail with a message saying this build cannot
start programs or open connections, instead of a misleading "command not
found" or network error. In the browser, `@bjorn3/browser_wasi_shim`
(MIT OR Apache-2.0, loaded pinned from jsDelivr) provides WASI with an
in-memory file system, and the script runs in a Web Worker so a time limit
(10 s) can stop a script that never ends. The examples are copied from
`examples/` at deploy time, so the page never drifts from the repository.
The playground runs ok as built from `main`, not the latest release: the
release does not have the WebAssembly changes yet. Once a release has them,
building it from the latest release tag would keep the page and the
installed binary in step.

**YAML aliases may add at most 1,000,000 values per file.** `yaml-rust2`
copies an anchor's value at every alias, so a 1 KB file of aliases that
repeat each other expands to hundreds of millions of values and exhausts
memory (found while reviewing whether `read` is safe on untrusted files).
A first pass over the parser's events computes what the aliases would add
without building it, and `read`/`parse` refuse above the limit. The limit
counts only alias copies, so large YAML files without aliases are not
affected, and real files (a few defaults blocks merged into jobs or
services) stay far below it. The other parsers were checked at the same
time: JSON, YAML and TOML stop deep nesting with an error (128, 256 and
TOML's own limit), and gzip data that expands beyond available memory ends
in an `out of memory` error rather than a crash.

**The playground runs the latest release.** Since v0.2.0 has the
WebAssembly build, the Pages workflow builds `ok.wasm` and copies the
examples from the latest release's tag instead of `main`, fills the
release name into the page's badge, and runs again when a release is
published. The page then shows the same ok that the install scripts
install, and unreleased changes never reach it early.

## 2026-11-26 (guide)

**A user guide at https://okrun.dev/docs/, generated from Markdown with tested samples.** The site's Docs link pointed at `SPEC.md`, which defines the language but does not teach it. The guide is Markdown in `docs/` (readable on GitHub too), turned into pages styled like the landing page by `crates/okdocs`, a small unpublished crate using `pulldown-cmark` (MIT) and ok's own lexer for highlighting. Code fences say what a block is: an ok snippet (must parse and be in canonical format), a named file, or the output of a command. `cargo test -p okdocs` runs every command against the real binary on all three OSes and compares the output, so the guide cannot drift from the language. In the browser each output block gets a Run button that runs the command with the WebAssembly build, using the section's files, which the reader can edit. mdBook was considered: quicker to set up, but it would look unlike the site and would neither test nor run the samples. The guide is built from the latest release when that release has `docs/`, and from `main` until then, so the pages describe the ok the Run buttons run. `THIRD_PARTY.md` is now generated for the `okrun` package only (`-m crates/okrun/Cargo.toml`), so the guide's build tools and the test helper `ok-fixture` are not listed as parts of the binary.

