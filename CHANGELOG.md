# Changelog

All notable changes to this project are documented in this file.

<!-- One line per paragraph and per bullet, no hard wrapping: each version's section is copied verbatim into its GitHub release notes, where every newline becomes a line break. -->

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1] - 2026-11-28

A bug fix for `zip`, and a revision of the guide, the specification's wording and the comments in the source. No script or project needs changes.

### Fixed

- `zip` packs a directory that contains a symbolic link to a directory. Before, it failed with "cannot write out.zip: Is a directory". `zip` stores what a link points to, which is the content of a file or a directory with its entries. A link to a directory that contains the link is an `io` error that names the link. `tar` stores links as links, as before.

### Documentation

- The guide and the comments in the source were revised for accuracy and plain wording. About a dozen statements in the guide that did not match ok's behaviour are corrected, and every standard library entry has the same layout.
- `SPEC.md` 9.7 said that a command that times out raises a `command` error. It raises a `timeout` error, as 9.6 and 10.2 say and as ok does. The rest of the specification is reworded in places without a change of meaning.
- The README opens with a runnable example and lists what ok does not do. `CONTRIBUTING.md` no longer says that CI is switched off.

## [0.4.0] - 2026-11-27

Projects can now keep everything in one hidden `.ok/` directory or in a single `.ok` file, and `ok self init` writes the wrappers only with `--wrappers`. An `ok/` directory of namespaces still works for this release, with a warning, and `ok self promote` moves it. A directory with two root files is now an error. Much of this release makes ok safer to use as a default scripting language. It adds limits on untrusted data, archives and programs, makes files, processes and fetch safer, and makes trust stricter.

Some of these changes can stop a project or script from loading. Source files may no longer contain raw control or bidirectional characters, env files may no longer set `OK_`, `LD_` or `DYLD_` variables, a root `ok.ok` may not link outside the project, and projects with env files must be trusted again. Data and archives that used to be accepted can now be errors. This applies to archive members with names that are not portable, CSV files whose header repeats a column, TOML date-times after 2262, and data beyond the new size limits. Programs with expressions chained more than 1000 levels deep, values nested more than 1000 levels, or JSON, YAML and TOML output nested deeper than ok can read back are now errors. Scripts can also fail where they used to go on. `run a | run b` now fails when `a` does, environment variable names with `=` are refused, `.` in `PATH` is no longer searched, file names that are not UTF-8 are errors in `list_dir` and `glob`, and `copy` copies symlinks inside a directory as links.

### Security

- The trust prompt's `[v]iew`, `[d]iff` and file list show control characters, bidirectional controls and zero-width characters as escapes such as `\r`, `\u{1b}` and `\u{202e}`. A carriage return, an ANSI or OSC sequence or a "Trojan Source" reordering can no longer make the reviewed code look different from the code that runs.
- Error messages, warnings, task listings, `ok TASK --help`, `--trace` and `--dry-run` lines and `= command:` details escape control characters, bidirectional controls and zero-width characters, so a fresh clone or a data file cannot write to your terminal or clipboard through ok's diagnostics. What a program prints itself, with `print` or as the output of a command it runs, is unchanged.
- Raw control characters other than tab and line endings, and bidirectional control characters, are syntax errors anywhere in source, including strings and comments. Write a `\u{...}` escape in their place.
- Trust covers the env files that a run loads. Editing `.env`, or the first run with `--env NAME`, asks for approval again. The review and the trust store show only variable names and a hash, never values.
- Env files cannot set variables whose names start with `OK_`, `LD_` or `DYLD_`, such as `OK_TRUST` and `LD_PRELOAD`. The error names the file and the key.
- Trust in a project's content is bound to the project root it was given for, so ok asks again about the same files in another directory.
- A trust store that cannot be read is an error. Before, it was treated as empty and overwritten. The store is created private to the user, with mode 0700 for directories and 0600 for files on Unix. Changes are made under a lock, so `ok trust` runs that happen at the same time keep each other's entries.
- A root `ok.ok` that is a symlink to a file outside the project is refused, like namespace files and imports.
- `ok self init` accepts only checksums of 64 hexadecimal digits and downloads `SHA256SUMS` only over https. Pass `--checksums FILE` for other mirrors.
- The `./ok` and `ok.ps1` wrappers cache the binary under its version and checksum, so a binary cached for another pinned version or checksum is never reused. `ok.ps1` downloads to a unique file name. `ok.cmd` starts PowerShell by its full path and no longer searches the current directory for it. Run `ok self init` again to get the new wrappers.
- Traces, dry runs and fetch errors show a URL's password as `***`.
- `install.sh` and `install.ps1` run nothing until the whole script has been downloaded.
- `untar` no longer creates links that lead outside the destination through other links, such as `d -> .` and `e -> d/d/../..`. It creates links after every other member, normalises each link's target and checks that the target stays inside the destination. A hard link becomes a copy of a file extracted earlier.
- `unzip` and `untar` never write through a symbolic link that is already in the destination. A link where a member's directory would go is an error. A link at a member's own path is replaced, not followed.
- Archive member names must be portable to every OS. A name is refused when it contains `:`, `\` or a control character, ends in a dot or a space, or is a Windows device name such as `NUL`, `con.txt` or `COM1`. Before, on Windows, `a/C:evil.txt` could write anywhere.
- Extracted files get the permissions of any new file, so the umask applies, plus the execute bit of executable members. The setuid, setgid, sticky and world-writable modes are no longer restored.
- `unzip` and `untar` stop at 4 GiB of file content or 1,000,000 members. Change the limits with `max_size:` and `max_entries:`, or pass `null`.
- `read` and `stdin` limit a whole document to 1 GiB and a line or CSV record to 64 MiB. The document limit applies after decompression, so a small `.json.gz` cannot exhaust memory. Change the limits with `max_size:` and `max_line:`.
- YAML aliases are also limited by the amount of text they copy, which may be at most 64 MiB, so one large string aliased many times is refused.
- `fetch` follows redirects itself. It never follows a redirect from https to http, and it follows only redirects to http and https URLs. A redirect to another host drops every header you passed. Before, `x-api-key`, `private-token` and similar headers were sent on. The timeout now covers all redirects.
- `fetch` limits a response body to 1 GiB by default. Set another limit with `max_size:`. A download with `to:` has no default limit, but you can give it one.
- The regex cache is bounded, so patterns built from data no longer make memory use grow.
- Member names in archive errors are escaped like other untrusted text.
- A command's `timeout` stops the command together with every process it started, using a process group on Unix and a job object on Windows. It also covers reading the command's output. `run sh -c "sleep 60 & ..." with {timeout: 1s}` no longer runs on past the deadline, and no longer keeps ok waiting past it. The timeout of a piped command is enforced while ok reads from the command, and it reports the right duration.
- Once a script has started a command with a timeout or made a temporary file, Ctrl-C, SIGTERM and SIGHUP make ok pass SIGTERM on to those commands' process groups, remove the files of `tempdir()` and `tempfile()`, and exit with 130.
- `tempdir()` and `tempfile()` use unpredictable names, never reuse an existing entry, and are private to the user (0700 and 0600 on Unix).
- `write`, `zip`, `tar` and `fetch(to:)` write a temporary file first and then rename it. That temporary file is private to the user (0600) until it is complete and has an unpredictable name, and when a name is taken ok tries another. The finished file keeps the permission bits of the file it replaces, without setuid, setgid and sticky.
- `copy` copies symlinks inside a directory as links. Before, it copied what they point to. It also never writes through a symlink in the destination. A symlink where a file goes is replaced, and a symlink where a directory goes is an error. The same holds for `move` when it has to copy from one device to another.
- File names that are not valid UTF-8 are refused with an error in `list_dir`, in `zip` and `tar`, and in `glob` when the pattern selects them. Before, such a name was converted into the name of another file, which a later `remove` could have deleted.
- Environment variable names that are empty or contain `=` or NUL are refused in `with {env}`, `env.set` and `env.remove`, as are values that contain NUL. Before, the name `"FOO=BAR"` set `FOO`.
- On Unix, only the variable named exactly `PATH` is the search path. `with {env: {path: "bin"}}` no longer makes ok run `bin/ls`.
- Empty and `.` entries of `PATH` are skipped. Relative entries are resolved against ok's working directory, also for a command with a `cwd` option. ok starts the program it found by its absolute path, so the file that was checked is the file that runs.
- `remove` refuses a directory that contains the home directory, such as `remove("/home")`, as it does for the working directory.
- Every project file and env file is read once per run. The bytes that trust fingerprints, shows and snapshots are the same bytes that ok parses, imports and loads, so a file that is replaced while ok starts cannot run unreviewed.
- A parallel task's output lines are forwarded in pieces of at most 64 KiB, so a parallel task that prints endless text without newlines cannot exhaust ok's memory.
- Programs can no longer crash ok by exhausting the stack. An expression tree may be at most 1000 levels tall, and every link of a chain such as `a + b + ...`, `x | f | g`, `.a.b.c` or `else if` counts as a level. A taller tree is a syntax error. Evaluation nests at most 50000 levels across calls. Lists and records nest at most 1000 levels, so `x = [x]` in a loop is a `value` error. Before, 150000 chained `+ 1`, 50000 `.field` accesses or a value wrapped in itself a million times aborted ok when it was evaluated, formatted, displayed, compared, sorted or written.
- `sort` no longer aborts when some keys cannot be compared, such as rows where some `age` are `null`. It raises the type error.
- `pad`, `repeat` and `+` on strings and lists refuse a result over 1 GiB with an error, and so does reading a whole stream into memory, as in `len(range(10_000_000_000))`. Before, ok aborted when it allocated the memory. A value that would take more than 1 GiB written out is a `value` error, and shared parts count each time they occur, as with `x = [x, x]` in a loop. Before, ok aborted when such a value was printed or written.
- A key lookup with a huge unknown name no longer allocates a quadratic table for its "did you mean" suggestion.

### Added

- `ok self promote` moves a project to the `.ok/` layout. It moves a `.ok` root file to `.ok/ok.ok` and the files of an old `ok/` directory into `.ok/`. It checks every move first, undoes them if one fails, lists imports that need a new path, and says that the project needs trust again.
- A new fuzz target, `eval`, runs generated programs in a sandbox of the interpreter, which has no files, commands or network and limits the number of steps. `tests/no_panic.rs` does the same deterministically on every test run.

### Changed

- **Project layout (can break projects and scripts):** a project's root file can be `.ok/ok.ok`, which is recommended, a single file named `.ok`, or `ok.ok` as before. Namespaces live in `.ok/`, so `.ok/db.ok` is `ok db`. A directory with two root files is an error. Namespaces in `ok/` still load for one release, with a warning, and `ok self promote` moves them. A project that keeps `ok.ok` keeps its trust. Files that move must be trusted again.
- `ok self init` writes `.ok/ok.ok`. It writes the wrappers `./ok`, `ok.cmd` and `ok.ps1` only with `--wrappers`. Without that option nothing is downloaded and `--checksums` is a usage error. In a directory that already has a root file, `ok self init` is an error and `ok self init --wrappers` writes only the wrappers.
- An import of a file named `.ok` or `.ok/ok.ok` mounts a sub-project, as an import of `ok.ok` does. Mounts from `.ok/ok.ok` are relative to `.ok/`, like every import, for example `import "../web/ok.ok" as web`. The mounted file must be the only root file of its project.
- `--trace` and `--dry-run` quote the paths of file operations and fetch in the same way as command arguments, for example `~ copy "my notes.txt" backup/`.
- `ok --env NAME trust` trusts a project together with the env files of `--env NAME`.
- A TOML offset date-time outside 1677-09-21 to 2262-04-11 is a `format` error. Before, it gave a wrong time, or a crash in debug builds.
- A CSV or TSV header that names a column twice is a `format` error that names the column. Before, a column was lost without a message. Read such files with `header: false`.
- `fetch` keeps response headers that occur more than once. Their values are joined with `, `, and `set-cookie` is always a list of strings.
- `fetch(..., to:)` asks for the body without content encoding, so files are saved as the server stores them.
- `untar` skips devices and pipes, and refuses a member below a link of the same archive.
- In a pipeline of commands such as `run a | run b`, a failure of `a` is raised once `b` has finished, as with `set -o pipefail` in a shell. When several commands fail, the leftmost failure is raised. A command that ended because the next one stopped reading has not failed. `run!` applies to the command it is written on.
- `write` through a symlink to a missing file creates that file and keeps the link. Before, it replaced the link with a regular file.
- A NUL character in a command argument is reported as a NUL character. Before, the error described a problem with batch files.
- Ints and floats compare by exact value. `9007199254740993 == 9007199254740992.0` is `false`, where it was `true`. As a result `==`, `<`, `sort`, `min`, `max` and `uniq` agree on every mix of numbers, and `uniq([1, 1.0])` gives `[1]`.
- `to` and `write` refuse JSON nested more than 127 levels, YAML more than 255 and TOML more than 81. These are the depths that ok accepts when it reads, so a file that ok wrote can be read back.

### Deprecated

- An `ok/` directory of namespaces still loads, with a warning, for this release. Move its files to `.ok/`, which `ok self promote` does for you. An `ok/` directory without `*.ok` files in it is left alone.

### Fixed

- The `%` format type prints huge floats as digits instead of `inf%`.
- `ok self fmt` keeps a `#!` comment that is not the shebang off the first line, and it keeps a name `run` from being followed by a space that would start a command. Both cases failed with an internal error. It also formats long `else if` chains and long files in linear time. Before, 1000 one-line branches took 5 s.
- A `\` at the very end of a file without a final newline, alone or at the end of a word, continues the command onto nothing, as it does when the newline is there. Before, it was an argument, and `ok self fmt` failed with an internal error on such a file.
- `ok self fmt` keeps the space between a unary `-` and a command, as in `- run a`. Before, it wrote `-run a`, where `run` is a name, and failed with an internal error.
- `ok self fmt` adds no trailing comma after a command that ends an argument list broken over several lines. The comma would become part of the command's last word.
- `ok self fmt` keeps one blank after a `\` argument, or after a word that ends in `\`, at the end of a line, also where it breaks a pipeline. Without the blank the `\` would continue the line.
- `ok self fmt` keeps the spacing after a keyword used as a field name, as in `s.or[0]` and `x.and(1)`. Before, it spaced them like the operators and failed with an internal error.
- A project can have both namespaces and the `./ok` wrapper. Namespaces live in `.ok/`, so they no longer need a directory named `ok`, which is also the wrapper's name. Before, `ok self init` failed next to an `ok/` directory with "ok already exists".

### Documentation

- The guide presents the `.ok/` directory as the project layout, the single `.ok` file for small projects, and `ok.ok` as still supported, with `ok self promote` for moving between them.
- SECURITY.md and the guide state that `./ok` is a script from the repository that runs before any trust check, so in a repository you have not reviewed, run your installed `ok`.
- The commands guide warns that an interpolated value starting with `-` is read as an option by most programs, and shows `--`.
- The spec and the data guide say that JSON and YAML integers beyond the signed 64-bit range become floats, and the spec states the range of `time` (1677 to 2262).

## [0.3.0] - 2026-11-26

Some of these changes can break scripts written for 0.2.0. Compact JSON no longer has spaces, `parts()` of an absolute path starts with its root, `remove` refuses the working directory and the directories above it, copying a file onto a directory is an error, an import that leaves the project stops the project from loading, and `--env NAME` with a missing file exits with 2.

### Added

- ok has a user guide at https://okrun.dev/docs/ that goes from a first script to tasks and has a standard library reference. Every example is tested against ok, and runs in the browser with a Run button.

### Changed

- `to(v, "json", compact: true)`, `write(..., compact: true)` and JSON Lines write JSON without spaces. The output is `{"a":[1,2]}`, where it was `{"a": [1, 2]}`.
- `p.parts()` of an absolute path starts with its root, so the parts rebuild the path. `path("/a/b").parts()` is `["/", "a", "b"]`, and on Windows the first part is `"C:/"` or `"//server/share/"`.

### Fixed

- `THIRD_PARTY.md` no longer lists `ok-fixture`, a test helper that is not part of the binary.
- In the WebAssembly build, `import` and project mounts work in the browser playground, where they failed with "Not supported". A missing import says "no such file".
- In the WebAssembly build, `touch` works in the browser. For an existing file where the host cannot set file times, the time is left unchanged.
- In the WebAssembly build, `tempdir()` and `tempfile()` no longer crash. They use `TMPDIR`, or `.ok-tmp` in the working directory, removed at exit.
- In the WebAssembly build, `tar` no longer crashes and `untar` works in the browser. `os.exe` is `""`, where it was `".wasm"`.
- Unsafe: `remove(".")`, `remove("..")` and `remove(cwd)` deleted the contents of the working directory or of a parent. `remove` now refuses the working directory and any directory that contains it, before deleting anything.
- Unsafe: a failed `write`, for example of a function as JSON or of a string as CSV, emptied the existing file. `write`, `zip`, `tar` and `fetch(to:)` now write to a temporary file and replace the target only when the temporary file is complete. A failed `append` adds nothing. As a result `read(p) | ... | write(p)` rewrites a file in place.
- Unsafe: `copy(p, p)` emptied the file, and copying or moving a directory into itself never ended or deleted the source. Both are now errors.
- Copying a file onto an existing directory gives a portable error with a hint, where it gave the OS message. Copying a directory onto an existing one merges them, and the documentation now says so. Messages for "is a directory", "not a directory", "not empty" and "already exists" are the same on every OS.
- Unsafe: an import that leaves the project root was refused only when a task ran. Listing the tasks and `ok self check` accepted it, and `ok trust` recorded a fingerprint without the outside file, which could then change unnoticed. Such an import is now an error at load time everywhere the project is loaded, before anything is trusted.
- Without a terminal, an untrusted project fails with `error[trust]: this project is not trusted` and a note that names the root, as the spec shows. When an earlier version was trusted, the error says that the project files changed since you trusted them and names the changed files.
- `ok self check` on a project that fails to load, for example because of a bad `needs` argument, now ends with the summary line `ok: the project failed to load: 1 error`. The spec describes the form that the summary has for a project, `ok: 2 files and 3 tasks checked ...`.
- `ok self init` writes `requires` with the major and minor number of the running version, which is `"0.3"` in this release. Before, it wrote `"0.1"`.
- `ok self --help`, `ok trust --help` and `ok untrust --help` print their commands instead of failing with an unknown option.
- `--env NAME` with a missing `.env.NAME` exits with 2, the code for bad usage, instead of 1, and the help line says how to fix it.
- In error traces, a script's `main` and the task named on the command line end with ``= in `main`, started by ok``. Before, they pointed at their own declaration. A dependency that fails in a parallel worker is traced to its `needs` entry, as with `-j1`.
- A match guard that is a bare name or a parenthesised expression, as in `_ if flag =>` and `_ if (flag) =>`, is a guard. Before, it was read as an arrow closure and failed with "expected `=>` after the pattern".
- Type errors name list elements in the plural. They say "must be a list of strings", where they said "a list of a string".
- "Did you mean" no longer suggests unrelated short names, such as `a` for `r.b` or `get` for `grep`. Names of one or two characters match only a difference in case, names of three or four characters allow one edit, and longer names allow two.
- An unknown name piped into after a command, as in `run ls | grep x`, says to write `| run grep x`, before the command runs. `ok self check` reports it the same way.
- An unknown name inside `{...}` of a string, as in `"body { color: red }"`, explains that `{` starts an interpolation and how to write a literal brace. Before, the error suggested `floor`. An interpolation that does not end, as in `glob("*.{md,txt}")`, gets the same help.
- The help for a condition that is not a bool repeats the condition, as SPEC.md 5.3 shows. It says `if len(files) > 0` or `if files != []`, where it said `xs`.
- Writing TSV or CSV with `header: false` and records says that rows must be lists and names the format. Before, it said "CSV rows must be records, got a record".
- `pad(..., align: "middle")` is an `arg` error with a "did you mean" suggestion instead of a `type` error.
- The unknown-format help of `parse` lists `yaml_all`.
- A misspelled function of an imported namespace says "`math` has no function `quadd`" and suggests a similar name among the namespace's exports. Before, it said "no function `quadd` to call on a record".
- Importing the same file twice without `as` does nothing the second time. Before, the file's names clashed with themselves.
- `--dry-run` and `--trace` show `remove` of a missing path and `mkdir` of an existing directory, like any other file operation.
- `--help` lists a bool flag that defaults to true as `--[no-]compress  compress the archive [default: true]` instead of `--no-compress`, which read backwards.
- SPEC.md: the 12.1 and 12.4 tables list `floor`, `ceil`, `round`, `parse`, `to` and the `delimiter`, `header` and `compact` options. Section 9.8 says which arguments the trace quotes. Section 6.9 says `it` is not a name scripts can use. The examples in 9.5 split piped output with `lines`. Section 10.2 defines what `fail(x)` raises. Section 12.1.1 says to single-quote glob patterns with `{a,b}`.

## [0.2.0] - 2026-11-26

### Added

- ok builds for WebAssembly (`wasm32-wasip1`). This build runs the playground at https://okrun.dev/#try. There `run` and `fetch` report that they are unavailable, and `os.name` is `"wasi"`.
- Command mode can redirect stderr with `2> file`, `2>> file` and `2>&1`. The last one merges stderr into stdout, so the two are captured, piped or written together. `--trace` and `--dry-run` show every redirect.
- Independent task dependencies run in parallel, at most `--jobs N` at a time. The default is the number of CPUs, and `-j1` runs them one at a time. Tasks that run in parallel show their output line by line with a `task | ` prefix. After a failure ok starts nothing new, waits for the running tasks and exits with the failed task's exit code.
- A dependency can take arguments, as in `needs [build(release: true)]`. The same task with the same arguments runs once per invocation, and different arguments run it again.
- `try` can have a `finally` block, written `try { ... } finally { ... }` or `try { ... } catch e { ... } finally { ... }`. The `finally` block runs however the `try` ends, whether with a value, an error, `return`, `break`, `continue` or `exit()`. `finally` is now a keyword.
- `match` arms accept list and record patterns: `[]`, `[first, ...rest]`, `{status: 200, body}`, nested patterns and `|` inside them.
- A parameter can have a choice type, as in `--role: "admin" | "user" = "user"`, with an optional final `| null`. Values are checked on the command line, with a "did you mean" hint, and on calls. They are shown in `--help` and offered by shell completions.
- Script and task parameters can have a short flag, as in `--verbose, -v: bool`. Switches group (`-vq`), values follow as the next argument (`-o out.json`), and `--help` and completions show the short flags. `-h`, `-C` and `-e` are reserved.
- ok reads and writes gzip-compressed files. `read("x.json.gz")` decompresses and picks the format from the inner extension. `write(v, "x.json.gz")` compresses, with identical bytes on every OS. Line formats stream, `append: true` adds a gzip member, and `gzip: true/false` overrides the name.
- `ok self check` finds unknown names and gives "did you mean" help for them. It finds calls with the wrong number of arguments or with unknown named arguments, for builtins, `fn`s and imports, and it finds missing imports. It warns about `write` arguments that look swapped. It checks every file of a project and reports all findings. Warnings fail the check only with `--strict`.
- `ok self fmt` normalises spacing inside lines, for operators, commas, colons, brackets, records and one-line blocks. It breaks lines longer than 100 columns, either before each `|` of a pipeline or with one element per line and a trailing comma. It refuses to write a result whose syntax tree or comments differ from the original.

### Changed

Some of these changes can break scripts written for 0.1.0. They affect a `match` arm whose alternatives bind different names, arguments that start with `-` and a letter, which are now short flags, and the same stream redirected twice.

- Redirecting the same stream twice in one command (`> a > b`) is an error instead of using the last target.
- A task whose dependencies can run at the same time now runs them in parallel by default. Those dependencies get prefixed output and no terminal input. `--jobs 1` keeps the previous behaviour of running them one at a time.
- A dependency with a required parameter is allowed when its `needs` entry gives the argument.
- The alternatives of a `match` arm (`p1 | p2`) must bind the same names.
- A script or task argument that starts with `-` and a letter is a short flag. Pass such values after `--`. `-5` and `-` are still values.
- `ok self fmt` indents lines once per opening line instead of once per bracket, so `print(match x {` indents its arms one level. End-of-line comments keep their column. It needs files that parse, reports every file that cannot be formatted and continues with the others.
- `ok self check` and `ok self fmt` reject unknown options with exit code 2.
- `examples/largest-files/largest-files.ok` is reformatted. One long call is split by the new line width.

### Fixed

- Security: reading or parsing YAML whose aliases repeat each other, known as "billion laughs", could exhaust memory. A 1 KB file expanded to hundreds of millions of values. ok now counts what aliases would add before building anything and fails with a `format` error above 1,000,000 values.
- ok no longer takes exponential time and memory to read nested triple-quoted strings with interpolations, such as `"""{"""{...`. Fuzzing found this. An interpolation that runs past the end of its `"""` string is now a clear syntax error.

## [0.1.0] - 2026-11-25

### Added

- Distribution (milestone M7): a release workflow for six static targets with `SHA256SUMS` and a size check, and `ok self init` with the `./ok`, `ok.cmd` and `ok.ps1` bootstrap wrappers. The wrappers have a pinned version and checksums, a user cache and `OK_DOWNLOAD_URL`. The README has an install section.
- The examples run in the test suite with fake tools.
- Project mode (milestone M5): discovery of `ok.ok` upwards, or in `-C DIR`. Namespaces come from `ok/*.ok`, where `_` files are helpers, and `import "web/ok.ok" as web` mounts a sub-project. Also added are the declaration-only rule, `needs` with cycle detection, `sources`/`outputs` up-to-date checks and `--force`, `confirm` with all questions asked before anything runs and with `--yes`, `hidden`, `env file` and `--env NAME`, `requires`, task arguments and `--help`, the task listing, `ok self tasks`, completions for bash, zsh, fish and PowerShell, `ok self check` and `ok self fmt`.
- Trust (milestone M6): content fingerprints, the trust store with snapshots, the prompt with view and diff, `ok trust [--dir] [--list]`, `ok untrust`, `--trust` and `OK_TRUST=1` for CI, and a clear failure without a terminal. Imports and symlinks are confined to the project root.
- Script command line (milestone M4): the signature of `main` becomes the command line, with positional arguments, rest arguments, `--flags` with values, switches with `--no-name`, repeatable `list[T]` flags and `--`. Values are parsed by type. `--help` is generated from the doc comment and the trailing parameter comments. Argument errors come with suggestions and the usage line, and exit with code 2. Scripts can start with `#!/usr/bin/env ok`.
- Commands (milestone M3): `run`, `run!` and the function form. Output is live, captured or piped, depending on the command's position. `|` works between commands and into commands, and `>`, `>>` and `<` redirect. The options are `cwd`, `env`, `stdin`, `timeout`, `bytes` and `quiet`. A failure carries the command, the exit code and the tail of stderr, and ok exits with the child's code. Also added are `--trace`, `--dry-run` and PATHEXT lookup on Windows.
- Standard library (milestone M3): files (`copy`, `move`, `remove`, `mkdir`, `exists`, `is_file`, `is_dir`, `glob`, `list_dir`, `stat`, `touch`, `tempdir`, `tempfile`, `chmod`), paths (`abs`, `relative_to`, `with_ext`, `parts`, `is_abs`, `home`, `cwd`, `script_dir`, `invocation_dir`), environment (`env.NAME`, `env.get/has/set/remove/all`, `which`), text (`trim`, `split`, `replace`, `contains`, `starts_with`, `ends_with`, `upper`, `lower`, `chars`, `repeat`, `pad`, regex `match`/`matches`/`find`/`sub`), time (`now`, `time`, `format`, `sleep`, `duration`), `prompt`, `confirm`, `hash`, `fetch`, `zip`/`unzip`/`tar`/`untar`.
- Data (milestone M2): `read`, `write`, `parse`, `to` and `stdin` for JSON, JSON Lines, CSV, TSV, lines, text, TOML, YAML (including merge keys and `yaml_all`), `.env` and bytes. The format is taken from the file name or given explicitly. The options are `header` and `delimiter` for CSV and TSV, `compact` for JSON, and `append` for writes.
- Streaming: line- and row-oriented reads, `lines`, `range` and `stdin()` are lazy. `where`, `map`, `take`, `skip`, `uniq`, `flat`, `enumerate` and `lines` stream, and consumers stop reading early.
- List and record functions: `where`, `map`, `each`, `sort`, `reverse`, `group`, `uniq`, `count`, `first`, `last`, `take`, `skip`, `sum`, `min`, `max`, `any`, `all`, `join`, `flat`, `enumerate`, `keys`, `values`, `entries`, `from_entries`, `get`, `has`, `lines`.
- Core interpreter (milestone M1): a lexer with command mode, a parser for the full grammar, and a tree-walking evaluator for values, operators, strings with interpolation and format specs, lists, records, paths, durations, functions with typed, named, default and rest parameters, closures, implicit `.field` closures, pipelines, `if`, `for`, `while`, `match`, `try`/`catch`, `?`, `??`, destructuring and imports.
- Errors with file, line, column, an underlined source snippet, notes, help with suggestions for misspelled names and habits from other languages, and a call trace.
- `ok SCRIPT [ARGS...]` and `ok -e SOURCE` run a program, and `args` holds a script's arguments.
- Core builtins: `print`, `eprint`, `str`, `int`, `float`, `type`, `len`, `fail`, `exit`, `path`, `range`, `floor`, `ceil`, `round`, `fmt`, `bytes`, `text`, and the `os` record.
- Golden tests (`tests/cases/`, regenerate with `OK_BLESS=1 cargo test --test golden`), lexer/parser/path/format unit tests, a random-input no-panic test, and cargo-fuzz targets for the lexer and parser.

- Repository setup: Cargo workspace (`okrun` package, `ok` binary), MIT OR Apache-2.0 licence, cargo-deny licence policy, cargo-about third-party notices, CI for format, clippy and tests on Linux, macOS and Windows, and a release workflow for six targets.
- `SPEC.md`: the full language specification draft for v0.1.0.
- `DECISIONS.md`: log of design decisions the brief left open.
- `examples/`: real-world scripts and Makefiles rewritten in ok, each next to the original.
- `ok.ok`: the repository's own build and admin tasks. They can be run from milestone M5, and until then the documented `cargo` commands are used.
- `quiet` command option to hide a command's output unless it fails.
