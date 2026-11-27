# Security policy

## Reporting a vulnerability

Please report security problems privately, through GitHub: on https://github.com/xtr-dev/ok/security, choose **Report a vulnerability**. Do not open a public issue for them.

Say what an attacker controls (a script, a cloned project, a data file, an archive, a server `fetch` talks to, an environment variable) and what they gain, and include a small reproduction if you can. You will get a reply within a week. Fixes ship as a patch release, and the release notes credit you unless you ask otherwise.

## Supported versions

Only the latest release gets security fixes. `ok self init --wrappers` pins projects to a version, so update the pin after a security release.

## What ok defends against

ok runs scripts with the permissions of the user who starts them; a script is code you chose to run, like a shell script. The protections are for everything else:

- **Projects you have not reviewed.** `ok` in a cloned repository runs nothing from it until you have trusted the project, and trust covers every file that can run (SPEC.md §16). Listing tasks, `--help`, completions, `ok self check` and `ok self fmt` only parse.
- **Untrusted data.** Reading files in any supported format, extracting archives and calling HTTP servers must end in a value or an error: never code execution, files written outside the destination of `unzip`/`untar`, or a crash.
- **No shell.** `run` passes each word as one argument and never starts a shell, so interpolated values cannot inject commands (SPEC.md §9).
- **Binaries.** Release binaries come with SHA-256 checksums and signed build provenance; the install scripts and the `./ok` wrappers check the checksum before running a download. Verify provenance with `gh attestation verify ok-x86_64-unknown-linux-musl --repo xtr-dev/ok`.

The `./ok` bootstrap wrapper (and `ok.cmd`, `ok.ps1`) is a script inside the repository, and your shell runs it before ok, and so before any trust check. A repository you do not trust can make it do anything. In a repository you have not reviewed, run your installed `ok` instead of `./ok`.

## How ok is kept secure

Dependencies are checked against the RustSec advisory database on every push and daily (`cargo deny`), GitHub Actions are pinned to commit hashes, the lexer, parser, formatter, data formats and archive extraction are fuzzed (briefly in CI, at length before releases), and `unsafe` code is forbidden in ok itself.
