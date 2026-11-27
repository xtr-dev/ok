# Trust

ok runs the tasks of a project only after you approve the exact content of its files. This page explains what ok checks, what the prompt offers and how to run tasks in CI.

When you clone a repository and type `ok build`, the `build` task is code that someone else wrote, and it runs with your permissions. So ok asks once before it runs any task of a project it has not seen, and asks again whenever the project's files change. The exact rules are in [section 16 of the spec](../SPEC.md#16-trust).

## Trust is not a sandbox

Approving a project does not make it safe. A trusted task can delete files, read your keys and use the network, like any program you start. Trust records that you agreed to run this code. It protects you from running tasks you have never looked at, for example right after `git clone` or `git pull`, and from running files that changed without your knowledge. Reading the files before you trust them remains your job.

## What needs trust

Running a task needs trust. That includes `ok --dry-run TASK`, because a dry run still evaluates the code of the task.

These commands only parse the project files, and they work without trust:

- `ok` with no arguments (the task listing) and `ok self tasks`
- `ok TASK --help`
- shell completion
- `ok self check` and `ok self fmt` (see [Formatter, checker and wrappers](tools.md))

Loading a project never runs any of its code (see [Tasks](tasks.md#project-files-hold-declarations)).

A script that you run by its path, such as `ok tools/release.ok`, needs no trust either. By naming the file you have decided to run it.

## What is trusted

ok trusts a project root together with a **fingerprint**: one SHA-256 hash over the paths and the contents of the project files and the env files. The paths are relative to the project root.

The **project files** are:

- the root file (`.ok/ok.ok`, `.ok` or `ok.ok`);
- every `.ok` file directly in `.ok/`, helper files included (and, in a project with the deprecated layout, those in `ok/`);
- every file that those files import, directly or indirectly, including the root files of mounted sub-projects.

The **env files** are the files that a run loads: the file named by `env file ".env"` and, with `--env NAME`, `.env.NAME` (see [Tasks](tasks.md#environment-files)). They are part of the fingerprint because a variable such as `NODE_OPTIONS` or `PATH` changes what every program does that a task starts. Env files often hold secrets, so the prompt shows only the names of their variables, never the values, and the trust store keeps only the names. A run with `--env staging` loads one more file than a run without it, so you approve it separately. `ok --env staging trust` approves it ahead of time.

A change to any of these files, even to a comment, changes the fingerprint, and ok asks again. Moving a file changes the fingerprint too, which is why the project needs trust again after [`ok self promote`](tools.md#moving-to-ok-with-ok-self-promote).

Files that tasks read or run, such as a `Cargo.toml` or a shell script started with `run`, are not part of the fingerprint. The fingerprint covers ok code and env files only.

Trust belongs to the directory it was given in. ok asks again about the same files in a copy of the directory, or in another checkout of the repository.

ok remembers each version that you approve. When you switch back to a branch whose project files you trusted before, ok runs its tasks without asking.

In a project of your own, where you are the one who changes the files, you can trust the directory instead of one fingerprint with `ok trust --dir`. ok then does not ask again for that directory, whatever its files contain.

## The prompt

When you run a task on a terminal in a project that you have not trusted, ok shows this prompt:

```text
This project is not trusted yet.

  root:   /home/you/src/myapp
  files:  .ok/admin.ok, .ok/db.ok, .ok/ok.ok
  sha256: 9f2c4e...

Tasks are code that runs with your permissions. Trust is not a sandbox.
[v]iew files, [t]rust and run, [n]o:
```

The three choices are:

- `v` prints every project file and asks again. Control characters, bidirectional controls and invisible characters are shown as escapes such as `\r` or `\u{202e}`, so that a file cannot look different from the code that runs. An env file is shown as the names of its variables.
- `t` records trust for this fingerprint and runs the task.
- `n` runs nothing and exits with code 1.

If you trusted an earlier version of the project, ok says so and adds a `d` choice. It prints a diff against the copy that ok kept of the trusted version, so you review only the changes:

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

### Without a terminal

Without a terminal on standard input, as in CI, a git hook or a pipe, ok does not wait for an answer. It fails immediately:

```text
error[trust]: this project is not trusted
  = note: the project is at /home/you/src/myapp
  = help: review the files, then run `ok trust`; in CI set OK_TRUST=1 or pass --trust
```

If you trusted an earlier version, the error says that the files changed and names them, because there is no prompt that could show the diff:

```text
error[trust]: the project files changed since you trusted them
  = note: the project is at /home/you/src/myapp
  = note: changed: .ok/ok.ok
  = help: review the files, then run `ok trust`; in CI set OK_TRUST=1 or pass --trust
```

## Trusting from the command line

You do not have to wait for the prompt. Read the project files, then run `ok trust` in the project:

```text
$ ok trust
trusted /home/you/src/myapp
  files:  .ok/ok.ok, web/ok.ok
  sha256: 1956c16a52ea3f783cca2f931b7c44151a47b4c63c929b4288ab14635ebe6242
```

`ok trust` and `ok untrust` work on the project around the current directory, or on the project in the directory you name, as in `ok trust ../other`. The commands are:

| Command | What it does |
|---|---|
| `ok trust [DIR]` | Trusts the current content of the project. |
| `ok trust --dir [DIR]` | Trusts the directory, whatever its content. For your own projects. |
| `ok trust --list` | Lists the trusted roots, each with its fingerprint or its directory trust and the time you approved it. |
| `ok untrust [DIR]` | Removes every entry for the project root: its fingerprints and its directory trust. |

With nothing trusted yet, the list says so:

```output run="ok trust --list"
nothing is trusted yet
```

`trust`, `untrust` and `self` are the only first arguments that ok reserves, so no task or namespace can have one of these names.

## CI and other non-interactive use

A CI job runs the tasks of the commit it checked out, and nobody is there to answer a prompt. Set `OK_TRUST=1` in the environment of the job:

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

Or pass `--trust` before the task name:

```sh
ok --trust test
```

Both run the tasks for that invocation only and record nothing. `OK_TRUST` must be `1` or `true`, and ok ignores any other value.

Use them where the job is meant to run the code it checked out, such as CI for your own repository. Do not set `OK_TRUST=1` in your shell profile. That turns the check off for every repository you clone.

Other things that run tasks without a terminal, such as a git hook, an editor integration or a cron job, have the same two options. Trust the project once by hand with `ok trust` (or `ok trust --dir` for a project that only you edit), or pass `--trust` in the command that the tool runs.

## Where trust is stored

The trust store is a directory in your configuration folder:

| OS | Location |
|---|---|
| Linux | `~/.config/ok/` (or `$XDG_CONFIG_HOME/ok/`) |
| macOS | `~/Library/Application Support/ok/` |
| Windows | `%APPDATA%\ok\` |

It holds two things:

- `trust.json` has one entry for each trusted root with its fingerprint, or with its directory trust.
- `snapshots/` has a copy of each trusted version, which the `[d]iff` choice compares against. For an env file, the copy holds only the names of its variables.

`OK_CONFIG_DIR` makes ok use another directory. Tests that need an empty trust store set it.

## Imports stay inside the project

The fingerprint must cover every file that a project loads. So in a project, every import and every namespace file must resolve inside the project root, after following symlinks. Two examples of what ok refuses are an import that leaves the root, such as `import "../../shared/lib.ok"` in `.ok/ok.ok`, and a file in `.ok/` that is a symlink to a file outside the root.

ok reports the error when it loads the project. Listing the tasks, `ok self check` and `ok trust` therefore fail in the same way as running a task, and ok never trusts a file outside the root:

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

To share code, keep it inside the project root, in a helper file such as `.ok/_lib.ok` or in a sub-project that the root file mounts (see [Mounting a sub-project](tasks.md#mounting-a-sub-project)).
