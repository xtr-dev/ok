# Security policy

## Reporting a vulnerability

Report security problems privately through GitHub. On https://github.com/xtr-dev/ok/security, choose **Report a vulnerability**. Do not open a public issue for them.

Say what an attacker controls (a script, a cloned project, a data file, an archive, a server `fetch` talks to, an environment variable) and what they gain, and include a small reproduction if you can. You will get a reply within a week. Fixes ship as a patch release, and the release notes credit you unless you ask otherwise.

## Supported versions

Only the latest release gets security fixes. `ok self init --wrappers` pins projects to a version, so update the pin after a security release.

## What ok defends against

ok runs scripts with the permissions of the user who starts them. A script is code you chose to run, like a shell script. The protections cover everything else:

- **Projects you have not reviewed.** `ok` in a cloned repository runs nothing from it until you have trusted the project, and trust covers every file that can run (SPEC.md §16). Listing tasks, `--help`, completions, `ok self check` and `ok self fmt` only parse.
- **Untrusted data.** Reading files in any supported format, extracting archives and calling HTTP servers must end in a value or an error. They must never execute code, write files outside the destination of `unzip`/`untar`, or crash.
- **Command injection.** `run` passes each word as one argument and never starts a shell, so interpolated values cannot inject commands (SPEC.md §9).
- **Binaries.** Release binaries come with SHA-256 checksums and signed build provenance. The install scripts and the `./ok` wrappers check the checksum before running a download. Verify provenance with `gh attestation verify ok-x86_64-unknown-linux-musl --repo xtr-dev/ok`.

The `./ok` bootstrap wrapper (and `ok.cmd`, `ok.ps1`) is a script inside the repository. Your shell runs it before ok starts, and so before any trust check. A repository you do not trust can make it do anything. In a repository you have not reviewed, run your installed `ok` instead of `./ok`.

## How ok is kept secure

- `cargo deny` checks the dependencies against the RustSec advisory database on every push and daily.
- GitHub Actions are pinned to commit hashes.
- The lexer, parser, formatter, evaluator, data formats and archive extraction are fuzzed, for 45 seconds per target in CI and for 30 minutes per target before a release.
- `unsafe` code is forbidden in ok itself.
