# Changelog

All notable changes to this project are documented in this file.

<!-- One line per paragraph and per bullet, no hard wrapping: each version's section is copied verbatim into its GitHub release notes, where every newline becomes a line break. -->

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Some of these changes can stop a project or script from loading: source files may no longer contain raw control or bidirectional characters, env files may no longer set `OK_`, `LD_` or `DYLD_` variables, a root `ok.ok` may not link outside the project, and projects with env files must be trusted again. Data and archives that used to be accepted can now be errors: archive members with names that are not portable, CSV files whose header repeats a column, TOML date-times after 2262, and data beyond the new size limits.

### Security

- The trust prompt's `[v]iew`, `[d]iff` and file list show control characters, bidirectional controls and zero-width characters as escapes (`\r`, `\u{1b}`, `\u{202e}`), so a carriage return, an ANSI or OSC sequence or a "Trojan Source" reordering can no longer make the reviewed code look different from what runs.
- Error messages, warnings, task listings, `ok TASK --help`, `--trace` and `--dry-run` lines and `= command:` details escape the same characters, so a fresh clone or a data file cannot write to your terminal or clipboard through ok's diagnostics. What a program prints itself (`print`, child output) is unchanged.
- Raw control characters (other than tab and line endings) and bidirectional control characters are syntax errors anywhere in source, strings and comments included; write a `\u{...}` escape instead.
- The env files a run loads are covered by trust: editing `.env`, or the first run with `--env NAME`, asks for approval again. The review and the trust store show only variable names and a hash, never values.
- Env files cannot set variables starting with `OK_`, `LD_` or `DYLD_` (such as `OK_TRUST`, `LD_PRELOAD`); the error names the file and the key.
- Content trust is bound to the root it was given for: the same files in another directory are asked about again.
- A trust store that cannot be read is an error instead of being treated as empty and overwritten. The store is created private to the user (0700 directories, 0600 files on Unix), and changes are made under a lock so concurrent `ok trust` runs keep each other's entries.
- A root `ok.ok` that is a symlink to a file outside the project is refused, like namespace files and imports.
- `ok self init` accepts only 64-hex-digit checksums and downloads `SHA256SUMS` only over https (pass `--checksums FILE` for other mirrors).
- The `./ok` and `ok.ps1` wrappers cache the binary under its version and checksum, so a binary cached for another pin is never reused; `ok.ps1` downloads to a unique file name, and `ok.cmd` starts PowerShell by its full path instead of searching the current directory. Run `ok self init` again to get the new wrappers.
- Traces, dry runs and fetch errors show a URL's password as `***`.
- `install.sh` and `install.ps1` run nothing until the whole script has been downloaded.
- `untar` no longer creates links that lead outside the destination through other links (`d -> .`, `e -> d/d/../..`): links are created after every other member, with their target normalised and checked to stay inside. Hard links become copies of a file extracted earlier.
- `unzip` and `untar` never write through a symbolic link already in the destination: a link where a member's directory would go is an error, and a link at a member's own path is replaced instead of followed.
- Archive member names must be portable on every OS: a name with `:`, `\` or a control character, one ending in a dot or space, or a Windows device name (`NUL`, `con.txt`, `COM1`) is refused. On Windows, `a/C:evil.txt` could write anywhere.
- Extracted files get the permissions of any new file (the umask applies) plus the execute bit of executable members; setuid, setgid, sticky and world-writable modes are no longer restored.
- `unzip` and `untar` stop at 4 GiB of file content or 1,000,000 members; change the limits with `max_size:` and `max_entries:`, or pass `null`.
- `read` and `stdin` limit whole documents to 1 GiB (after decompression, so a small `.json.gz` cannot exhaust memory) and lines and CSV records to 64 MiB; change them with `max_size:` and `max_line:`.
- YAML aliases are also limited by the text they copy (64 MiB), so one large string aliased many times is refused.
- `fetch` follows redirects itself: never from https to http, only to http(s) URLs, and a redirect to another host drops every header you passed (before, `x-api-key`, `private-token` and similar headers were sent on). The timeout now covers all redirects.
- `fetch` limits a response body to 1 GiB by default (`max_size:`); downloads with `to:` have no default limit but take one.
- The regex cache is bounded, so patterns built from data no longer grow memory.
- Member names in archive errors are escaped like other untrusted text.

### Changed

- `--trace` and `--dry-run` quote the paths of file operations and fetch like command arguments: `~ copy "my notes.txt" backup/`.
- `ok --env NAME trust` trusts a project together with the env files of `--env NAME`.
- A TOML offset date-time outside 1677-09-21 to 2262-04-11 is a `format` error instead of a wrong time (or a crash in debug builds).
- A CSV or TSV header that names a column twice is a `format` error naming it, instead of silently losing a column; read such files with `header: false`.
- `fetch` keeps repeated response headers: values are joined with `, `, and `set-cookie` is always a list of strings.
- `fetch(..., to:)` asks for the body without content encoding, so files are saved as the server stores them.
- `untar` skips devices and pipes, and refuses a member below a link of the same archive.

### Documentation

- SECURITY.md and the guide say plainly that `./ok` is a script from the repository that runs before any trust check, so in a repository you have not reviewed, run your installed `ok`.
- The commands guide warns that an interpolated value starting with `-` is read as an option by most programs, and shows `--`.
- The spec and the data guide say that JSON and YAML integers beyond the signed 64-bit range become floats, and the spec states the range of `time` (1677 to 2262).

## [0.3.0] - 2026-11-26

Some of these changes can break scripts written for 0.2.0: compact JSON no longer has spaces, `parts()` of an absolute path starts with its root, `remove` refuses the working directory and the directories above it, copying a file onto a directory is an error, an import that leaves the project stops the project from loading, and `--env NAME` with a missing file exits with 2.

### Added

- A user guide at https://okrun.dev/docs/: from a first script to tasks, with a standard library reference. Every example is tested against ok, and runs in the browser with a Run button.

### Changed

- `to(v, "json", compact: true)`, `write(..., compact: true)` and JSON Lines write JSON without spaces: `{"a":[1,2]}` instead of `{"a": [1, 2]}`.
- `p.parts()` of an absolute path starts with its root: `path("/a/b").parts()` is `["/", "a", "b"]` (`"C:/"` or `"//server/share/"` on Windows), so the parts rebuild the path.

### Fixed

- `THIRD_PARTY.md` no longer lists `ok-fixture`, a test helper that is not part of the binary.
- WebAssembly build: `import` and project mounts work in the browser playground (they failed with "Not supported"), and a missing import says "no such file".
- WebAssembly build: `touch` works in the browser; for an existing file where the host cannot set file times, the time is left unchanged.
- WebAssembly build: `tempdir()` and `tempfile()` no longer crash; they use `TMPDIR`, or `.ok-tmp` in the working directory, removed at exit.
- WebAssembly build: `tar` no longer crashes, and `untar` works in the browser; `os.exe` is `""` instead of `".wasm"`.
- Unsafe: `remove(".")`, `remove("..")` and `remove(cwd)` deleted the working directory's contents (or a parent's). `remove` now refuses the working directory and any directory that contains it, before deleting anything.
- Unsafe: a failed `write` (for example a function written as JSON, or a string written as CSV) emptied the existing file. `write`, `zip`, `tar` and `fetch(to:)` now write to a temporary file and replace the target only when complete, and a failed `append` adds nothing. As a result `read(p) | ... | write(p)` rewrites a file in place.
- Unsafe: `copy(p, p)` emptied the file, and copying or moving a directory into itself never ended or deleted the source; both are now errors.
- Copying a file onto an existing directory gives a portable error with a hint instead of the OS message; copying a directory onto an existing one merges them, as now documented. Messages for "is a directory", "not a directory", "not empty" and "already exists" are the same on every OS.
- Unsafe: an import that leaves the project root was only refused when a task ran; listing the tasks and `ok self check` accepted it, and `ok trust` recorded a fingerprint without the outside file, which could then change unnoticed. It is now a load-time error everywhere the project is loaded, before anything is trusted.
- Without a terminal, an untrusted project fails with `error[trust]: this project is not trusted` and a note naming the root, as the spec shows; when an earlier version was trusted, it says the project files changed since you trusted them and names the changed files.
- `ok self check` on a project whose loading fails (for example a bad `needs` argument) now ends with the summary line `ok: the project failed to load: 1 error`, and the spec describes the project form of the summary (`ok: 2 files and 3 tasks checked ...`).
- `ok self init` writes `requires` with the running version's major.minor (`"0.3"` in this release) instead of `"0.1"`.
- `ok self --help`, `ok trust --help` and `ok untrust --help` print their commands instead of failing with an unknown option.
- `--env NAME` with a missing `.env.NAME` exits with 2, the code for bad usage, instead of 1, and the help line says how to fix it.
- Error traces: a script's `main` and the task named on the command line end with ``= in `main`, started by ok`` instead of pointing at their own declaration, and a dependency that fails in a parallel worker is traced to its `needs` entry, as with `-j1`.
- A match guard that is a bare name or a parenthesised expression (`_ if flag =>`, `_ if (flag) =>`) is a guard; it was read as an arrow closure and failed with "expected `=>` after the pattern".
- Type errors name list elements in the plural: "must be a list of strings" instead of "a list of a string".
- "Did you mean" no longer suggests unrelated short names (`a` for `r.b`, `get` for `grep`): names of one or two characters only match a case-only difference, three or four characters allow one edit, longer names two.
- An unknown name piped into after a command (`run ls | grep x`) says to write `| run grep x`, before the command runs; `ok self check` reports it the same way.
- An unknown name inside `{...}` of a string (`"body { color: red }"`) explains that `{` starts an interpolation and how to write a literal brace, instead of suggesting `floor`; an interpolation that does not end (`glob("*.{md,txt}")`) gets the same help.
- The help for a non-bool condition repeats the condition, as SPEC.md 5.3 shows: `if len(files) > 0` or `if files != []` instead of `xs`.
- Writing TSV or CSV with `header: false` and records says that rows must be lists and names the format, instead of "CSV rows must be records, got a record".
- `pad(..., align: "middle")` is an `arg` error with a did-you-mean instead of a `type` error.
- The unknown-format help of `parse` lists `yaml_all`.
- A misspelled function of an imported namespace says "`math` has no function `quadd`" with a did-you-mean among its exports, instead of "no function `quadd` to call on a record".
- Importing the same file twice without `as` is a no-op instead of a clash of its names with themselves.
- `--dry-run` and `--trace` show `remove` of a missing path and `mkdir` of an existing directory, like any other file operation.
- `--help` lists a bool flag that defaults to true as `--[no-]compress  compress the archive [default: true]` instead of `--no-compress`, which read backwards.
- SPEC.md: the 12.1 and 12.4 tables list `floor`, `ceil`, `round`, `parse`, `to` and the `delimiter`, `header` and `compact` options; 9.8 says which arguments the trace quotes; 6.9 says `it` is not a name scripts can use; 9.5 examples split piped output with `lines`; 10.2 defines what `fail(x)` raises; 12.1.1 says to single-quote glob patterns with `{a,b}`.

## [0.2.0] - 2026-11-26

### Added

- ok builds for WebAssembly (`wasm32-wasip1`). This build runs the playground at https://okrun.dev/#try; there `run` and `fetch` report that they are unavailable, and `os.name` is `"wasi"`.
- stderr redirection in command mode: `2> file`, `2>> file` and `2>&1` (stderr merged into stdout, captured, piped or written together with it). `--trace` and `--dry-run` show every redirect.
- Independent task dependencies run in parallel, at most `--jobs N` at a time (default: the number of CPUs; `-j1` runs them one at a time). Tasks that run in parallel show their output line by line with a `task | ` prefix; after a failure ok starts nothing new, waits for the running tasks and exits with the failed task's exit code.
- Arguments to dependencies: `needs [build(release: true)]`. The same task with the same arguments runs once per invocation; different arguments run it again.
- `finally` for `try`: `try { ... } finally { ... }` and `try { ... } catch e { ... } finally { ... }`. The `finally` block runs however the `try` ends (value, error, `return`, `break`, `continue`, `exit()`); `finally` is now a keyword.
- List and record patterns in `match` arms: `[]`, `[first, ...rest]`, `{status: 200, body}`, nested patterns and `|` inside them.
- Choice types for parameters: `--role: "admin" | "user" = "user"`, with an optional final `| null`. Values are checked on the command line (with a did-you-mean hint) and on calls, shown in `--help`, and offered by shell completions.
- Short flags for script and task parameters: `--verbose, -v: bool`. Switches group (`-vq`), values follow as the next argument (`-o out.json`), and `--help` and completions show them. `-h`, `-C` and `-e` are reserved.
- Compressed files: `read("x.json.gz")` decompresses and picks the format from the inner extension, `write(v, "x.json.gz")` compresses (identical bytes on every OS), line formats stream, `append: true` adds a gzip member, and `gzip: true/false` overrides the name.
- `ok self check` finds unknown names (with did-you-mean help), calls with the wrong number of arguments or unknown named arguments (builtins, `fn`s, imports), missing imports, and warns about `write` arguments that look swapped. It checks every file of a project and reports all findings; warnings fail the check only with `--strict`.
- `ok self fmt` normalises spacing inside lines (operators, commas, colons, brackets, records and one-line blocks) and breaks lines longer than 100 columns: before each `|` of a pipeline, or one element per line with a trailing comma. It refuses to write a result whose syntax tree or comments differ from the original.

### Changed

Some of these changes can break scripts written for 0.1.0: a `match` arm whose alternatives bind different names, arguments starting with `-` and a letter (now short flags), and the same stream redirected twice.

- Redirecting the same stream twice in one command (`> a > b`) is an error instead of using the last target.
- A task whose dependencies can run at the same time now runs them in parallel by default, with prefixed output and no terminal input for those dependencies; `--jobs 1` keeps the previous one-at-a-time behaviour.
- A dependency with a required parameter is allowed when its `needs` entry gives the argument.
- The alternatives of a `match` arm (`p1 | p2`) must bind the same names.
- A script or task argument that starts with `-` and a letter is a short flag; pass such values after `--` (`-5` and `-` are still values).
- `ok self fmt` indents lines once per opening line instead of once per bracket, so `print(match x {` indents its arms one level; end-of-line comments keep their column; it needs files that parse, reports every file that cannot be formatted and continues with the others.
- `ok self check` and `ok self fmt` reject unknown options with exit code 2.
- `examples/largest-files/largest-files.ok` is reformatted (one long call split by the new line width).

### Fixed

- Security: reading or parsing YAML whose aliases repeat each other ("billion laughs") could exhaust memory: a 1 KB file expanded to hundreds of millions of values. ok now counts what aliases would add before building anything and fails with a `format` error above 1,000,000 values.
- The lexer no longer takes exponential time and memory on nested triple-quoted strings with interpolations (`"""{"""{...`, found by fuzzing); an interpolation that runs past the end of its `"""` string is now a clear syntax error.

## [0.1.0] - 2026-11-25

### Added

- M7 distribution: release workflow for six static targets with `SHA256SUMS` and a size check, `ok self init` with the `./ok`, `ok.cmd` and `ok.ps1` bootstrap wrappers (pinned version and checksums, user cache, `OK_DOWNLOAD_URL`), and an install section in the README.
- The examples run in the test suite with fake tools.
- M5 project mode: discovery of `ok.ok` upwards (or `-C DIR`), namespaces from `ok/*.ok` (`_` files are helpers), mounted sub-projects (`import "web/ok.ok" as web`), the declaration-only rule, `needs` with cycle detection, `sources`/`outputs` up-to-date checks and `--force`, `confirm` (all asked before anything runs, `--yes`), `hidden`, `env file` and `--env NAME`, `requires`, task arguments and `--help`, the task listing, `ok self tasks`, completions for bash, zsh, fish and PowerShell, `ok self check` and `ok self fmt`.
- M6 trust: content fingerprints, the trust store with snapshots, the prompt with view and diff, `ok trust [--dir] [--list]`, `ok untrust`, `--trust` and `OK_TRUST=1` for CI, a clear failure without a terminal, and import and symlink confinement to the project root.
- M4 script CLI: `main`'s signature becomes the command line (positional arguments, rest arguments, `--flags` with values, switches with `--no-name`, repeatable `list[T]` flags, `--`), values parsed by type, generated `--help` from the doc comment and trailing parameter comments, argument errors with suggestions and the usage line (exit code 2), and `#!/usr/bin/env ok` scripts.
- M3 commands: `run`, `run!` and the function form; live, captured and piped output by position; `|` between commands and into commands; `>`, `>>` and `<` redirects; options `cwd`, `env`, `stdin`, `timeout`, `bytes`, `quiet`; failures carry the command, exit code and stderr tail and exit with the child's code; `--trace` and `--dry-run`; PATHEXT lookup on Windows.
- M3 standard library: files (`copy`, `move`, `remove`, `mkdir`, `exists`, `is_file`, `is_dir`, `glob`, `list_dir`, `stat`, `touch`, `tempdir`, `tempfile`, `chmod`), paths (`abs`, `relative_to`, `with_ext`, `parts`, `is_abs`, `home`, `cwd`, `script_dir`, `invocation_dir`), environment (`env.NAME`, `env.get/has/set/remove/all`, `which`), text (`trim`, `split`, `replace`, `contains`, `starts_with`, `ends_with`, `upper`, `lower`, `chars`, `repeat`, `pad`, regex `match`/`matches`/`find`/`sub`), time (`now`, `time`, `format`, `sleep`, `duration`), `prompt`, `confirm`, `hash`, `fetch`, `zip`/`unzip`/`tar`/`untar`.
- M2 data: `read`, `write`, `parse`, `to` and `stdin` for JSON, JSON Lines, CSV, TSV, lines, text, TOML, YAML (including merge keys and `yaml_all`), `.env` and bytes, with the format taken from the file name or given explicitly; CSV/TSV options `header` and `delimiter`, JSON `compact`, `append` for writes.
- Streaming: line- and row-oriented reads, `lines`, `range` and `stdin()` are lazy; `where`, `map`, `take`, `skip`, `uniq`, `flat`, `enumerate` and `lines` stream, and consumers stop reading early.
- List and record functions: `where`, `map`, `each`, `sort`, `reverse`, `group`, `uniq`, `count`, `first`, `last`, `take`, `skip`, `sum`, `min`, `max`, `any`, `all`, `join`, `flat`, `enumerate`, `keys`, `values`, `entries`, `from_entries`, `get`, `has`, `lines`.
- M1 core interpreter: lexer (with command mode), parser for the full grammar, and a tree-walking evaluator for values, operators, strings with interpolation and format specs, lists, records, paths, durations, functions with typed, named, default and rest parameters, closures, implicit `.field` closures, pipelines, `if`, `for`, `while`, `match`, `try`/`catch`, `?`, `??`, destructuring and imports.
- Errors with file, line, column, an underlined source snippet, notes, help with suggestions for misspelled names and habits from other languages, and a call trace.
- `ok SCRIPT [ARGS...]` and `ok -e SOURCE`; `args` for scripts.
- Core builtins: `print`, `eprint`, `str`, `int`, `float`, `type`, `len`, `fail`, `exit`, `path`, `range`, `floor`, `ceil`, `round`, `fmt`, `bytes`, `text`, and the `os` record.
- Golden tests (`tests/cases/`, regenerate with `OK_BLESS=1 cargo test --test golden`), lexer/parser/path/format unit tests, a random-input no-panic test, and cargo-fuzz targets for the lexer and parser.

- Repository setup: Cargo workspace (`okrun` package, `ok` binary), MIT OR Apache-2.0 licence, cargo-deny licence policy, cargo-about third-party notices, CI for format, clippy and tests on Linux, macOS and Windows, and a release workflow for six targets.
- `SPEC.md`: the full language specification draft for v0.1.0.
- `DECISIONS.md`: log of design decisions the brief left open.
- `examples/`: real-world scripts and Makefiles rewritten in ok, each next to the original.
- `ok.ok`: the repository's own build and admin tasks (runnable from M5; use the documented `cargo` commands until then).
- `quiet` command option to hide a command's output unless it fails.
