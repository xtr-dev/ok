# Trust

ok runs a project's tasks only after you approve the exact content of its files. How the prompt works, `ok trust`, and running in CI.

When you clone a repository and type `ok build`, the `build` task is code someone else wrote, and it runs with your permissions. ok therefore asks once before it runs any task of a project it has not seen, and asks again whenever the project's files change. This page explains what is checked, what the prompt offers, and how to skip it where that is safe. The exact rules are in [section 16 of the spec](../SPEC.md#16-trust).

## Trust is not a sandbox

Approving a project does not make it safe. A trusted task can delete files, read your keys and talk to the network, like any program you start. Trust answers one question: "did I agree to run this code?". It protects you from running tasks you have never looked at, for example right after `git clone` or `git pull`, and from files that changed behind your back. Reading the files before you trust them is still your job.

## What needs trust

Only running a task needs trust, and that includes `ok --dry-run TASK`, because a dry run still evaluates the task's code. Everything that merely reads the project works without it:

- `ok` (the task listing) and `ok self tasks`
- `ok TASK --help`
- shell completion
- `ok self check` and `ok self fmt`

These only parse the files; loading a project never runs any of its code (see [Tasks](tasks.md#project-files-hold-declarations)). A script you run by its path, such as `ok tools/release.ok`, is not subject to trust either: naming a file explicitly is your decision to run it.

## What is trusted

The **project files** are the root file (`.ok/ok.ok`, `.ok` or `ok.ok`), every `.ok` file directly in `.ok/` (helpers included), and every file those import, directly or indirectly, including mounted sub-projects. ok computes one SHA-256 **fingerprint** over their paths, relative to the project root, and their contents. Moving a file, as `ok self promote` does, therefore changes the fingerprint too. Trust is recorded for a project root together with that fingerprint.

The **env files** a run loads (`env file ".env"` and, with `--env NAME`, `.env.NAME`, see [Tasks](tasks.md#environment-files)) are part of the fingerprint too: a variable such as `NODE_OPTIONS` or `PATH` changes what every program a task starts does, so a changed `.env` is a changed project. Since env files often hold secrets, the prompt shows only their variable names, never the values, and the trust store keeps only the names. A run with `--env staging` loads one more file than a run without, so it is approved separately; `ok --env staging trust` approves it ahead of time.

So any change to any of these files, even a comment, means the project is no longer the one you approved, and ok asks again. Files that tasks read or run, such as a `Cargo.toml` or a shell script called with `run`, are not part of the fingerprint. Only ok code and env files are.

Trust belongs to the directory it was given in. The same files copied to another directory, or another checkout of the repository, are asked about again.

Each version you approve is remembered. If you switch back to a branch whose project files you trusted before, ok runs its tasks without asking.

For your own projects, where you are the one changing the files, you can trust the directory instead of a particular content with `ok trust --dir`. Then ok never asks for that directory again.

## The prompt

Running a task in a project you have not trusted, on a terminal, shows:

```text
This project is not trusted yet.

  root:   /home/you/src/myapp
  files:  .ok/admin.ok, .ok/db.ok, .ok/ok.ok
  sha256: 9f2c4e...

Tasks are code that runs with your permissions. Trust is not a sandbox.
[v]iew files, [t]rust and run, [n]o:
```

- `v` prints every project file, then asks again. Control characters, bidirectional controls and invisible characters are shown as escapes such as `\r` or `\u{202e}`, so that nothing in a file can make it look different from what runs. Env files are shown as their variable names.
- `t` records trust for this content and runs the task.
- `n` runs nothing and exits with code 1.

If an earlier version of the project was trusted, ok says so and offers a diff against the copy it kept, so you only have to review what changed:

```text
The project files changed since you trusted them.

  root:   /home/you/src/myapp
  files:  .ok/ok.ok
  sha256: f856460707c9...

Tasks are code that runs with your permissions. Trust is not a sandbox.
[d]iff, [v]iew files, [t]rust and run, [n]o: d
--- trusted/.ok/ok.ok
+++ current/.ok/ok.ok
@@
   print("hello {name}")
 }
+# deploy the docs too
[d]iff, [v]iew files, [t]rust and run, [n]o:
```

ok never waits for an answer that cannot come. Without a terminal on standard input, as in CI, a git hook or a pipe, it fails immediately:

```text
error[trust]: this project is not trusted
  = note: the project is at /home/you/src/myapp
  = help: review the files, then run `ok trust`; in CI set OK_TRUST=1 or pass --trust
```

If an earlier version was trusted, the error says the files changed and names them, since there is no prompt to show the diff:

```text
error[trust]: the project files changed since you trusted them
  = note: the project is at /home/you/src/myapp
  = note: changed: .ok/ok.ok
  = help: review the files, then run `ok trust`; in CI set OK_TRUST=1 or pass --trust
```

## Trusting from the command line

You do not have to wait for the prompt. Read the files, then trust them:

```text
$ ok trust
trusted /home/you/src/myapp
  files:  .ok/ok.ok, web/ok.ok
  sha256: 1956c16a52ea3f783cca2f931b7c44151a47b4c63c929b4288ab14635ebe6242
```

`ok trust` and `ok untrust` work on the project around the current directory, or on the one you name: `ok trust ../other`. The commands are:

| Command | What it does |
|---|---|
| `ok trust [DIR]` | Trust the current content of the project. |
| `ok trust --dir [DIR]` | Trust the directory, whatever its content. For your own projects. |
| `ok trust --list` | List trusted roots with their fingerprints, or directory trust, and when you approved them. |
| `ok untrust [DIR]` | Forget every entry for the project's root, content and directory trust alike. |

With nothing trusted yet, the list says so:

```output run="ok trust --list"
nothing is trusted yet
```

`trust`, `untrust` and `self` are the only words ok reserves as first arguments, which is why no task or namespace may use them.

## CI and other non-interactive use

A CI job runs the tasks of the commit it checked out, and nobody is there to answer a prompt. Set `OK_TRUST=1` in the job's environment, or pass `--trust` before the task name:

```yaml
# .github/workflows/ci.yml
jobs:
  test:
    runs-on: ubuntu-latest
    env:
      OK_TRUST: "1"
    steps:
      - uses: actions/checkout@v4
      - run: ./ok test
```

```sh
ok --trust test
```

Both run the tasks for that invocation only and record nothing. `OK_TRUST` must be `1` or `true`; any other value is ignored. Use them where the code being run is already what the job is meant to run, such as CI for your own repository. Do not set `OK_TRUST=1` in your shell profile: that turns the check off for every repository you clone.

The same applies to anything else that runs tasks without a terminal: a git hook, an editor integration, a cron job. Either trust the project once by hand (`ok trust`, or `ok trust --dir` for a project only you edit), or pass `--trust` in the command the tool runs.

## Where trust is stored

The trust store is a small directory in your configuration folder:

| OS | Location |
|---|---|
| Linux | `~/.config/ok/` (or `$XDG_CONFIG_HOME/ok/`) |
| macOS | `~/Library/Application Support/ok/` |
| Windows | `%APPDATA%\ok\` |

It holds `trust.json`, with one entry per trusted root and fingerprint (or directory), and `snapshots/`, a copy of each trusted version that the `[d]iff` choice compares against (for env files, only their variable names). `OK_CONFIG_DIR` points ok at a different directory, which is useful in tests that need an empty store.

## Imports stay inside the project

For the fingerprint to mean anything, every file a project loads has to be part of it. So in project mode, every import and every namespace file must resolve inside the project root, after following symlinks. An import that leaves the root, such as `import "../../shared/lib.ok"` in `.ok/ok.ok`, or a `.ok/` file that is a symlink to somewhere else, is an error as soon as ok loads the project: listing the tasks, `ok self check` and `ok trust` refuse it as well as running a task, so nothing outside the root is ever trusted by mistake:

```text
error[trust]: "../../shared/lib.ok" is outside the project
 --> .ok/ok.ok:1:8
  |
1 | import "../../shared/lib.ok"
  |        ^^^^^^^^^^^^^^^^^^^^^
  |
  = note: it resolves to /home/you/src/shared/lib.ok
  = help: project files may only import files inside the project root
```

To share code, keep it inside the project root: in a helper file such as `.ok/_lib.ok`, or in a sub-project that the root file mounts.
