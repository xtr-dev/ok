//! `ok self check`, `ok self fmt` and `ok self init` (SPEC.md 17, 18).

use std::path::{Path, PathBuf};

use crate::error::{Color, ErrKind, OkError, render};
use crate::interp::{FileKind, Flow, Interp, R, check_items};
use crate::path::OkPath;
use crate::value::Value;

/// The release targets, the wrapper's name for each, and the binary name.
pub const TARGETS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-musl", "ok-x86_64-unknown-linux-musl"),
    (
        "aarch64-unknown-linux-musl",
        "ok-aarch64-unknown-linux-musl",
    ),
    ("x86_64-apple-darwin", "ok-x86_64-apple-darwin"),
    ("aarch64-apple-darwin", "ok-aarch64-apple-darwin"),
    ("x86_64-pc-windows-msvc", "ok-x86_64-pc-windows-msvc.exe"),
    ("aarch64-pc-windows-msvc", "ok-aarch64-pc-windows-msvc.exe"),
];

fn usage(msg: &str) -> Flow {
    let mut e = OkError::new(ErrKind::Arg, msg);
    e.exit_code = Some(2);
    e.into()
}

/// The kind of an ok file judged by its place: `ok.ok` is a project root,
/// a file directly in an `ok/` directory next to an `ok.ok` is a namespace,
/// anything else is a script.
fn kind_of(path: &Path) -> FileKind {
    if path.file_name().is_some_and(|n| n == "ok.ok") {
        return FileKind::Project { root: true };
    }
    let parent = path.parent();
    let in_ok_dir = parent
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "ok");
    let has_root = parent
        .and_then(|p| p.parent())
        .is_some_and(|pp| pp.join("ok.ok").is_file());
    if in_ok_dir && has_root {
        let helper = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('_'));
        return if helper {
            FileKind::Library
        } else {
            FileKind::Project { root: false }
        };
    }
    FileKind::Script
}

/// Split the arguments of `ok self check` / `ok self fmt` into the allowed
/// options and the files.
fn options_and_files(
    args: &[String],
    allowed: &str,
    cmd: &str,
) -> Result<(bool, Vec<PathBuf>), Flow> {
    let mut flag = false;
    let mut files = Vec::new();
    for a in args {
        if a == allowed {
            flag = true;
        } else if a.starts_with('-') && a.len() > 1 {
            return Err(usage(&format!(
                "unknown option {a} for ok self {cmd} (it takes {allowed} and files)"
            )));
        } else {
            files.push(PathBuf::from(a));
        }
    }
    Ok((flag, files))
}

/// Read a file to check or format and register it in the source map, so
/// that findings can show its lines.
fn read_source(interp: &mut Interp, shown: &str, path: &Path) -> Result<(u32, String), OkError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        OkError::new(
            ErrKind::Io,
            format!("cannot read {shown}: {}", crate::interp::io_reason(&e)),
        )
    })?;
    let id = interp.sources.add(
        shown.to_string(),
        Some(path.to_path_buf()),
        text.clone().into(),
    );
    Ok((id, text))
}

/// Like [`read_source`], but the source map gets the text the formatter
/// works on: `\n` line endings, no byte order mark. Returns the id, the
/// text as read and the normalised text.
fn read_for_format(
    interp: &mut Interp,
    shown: &str,
    path: &Path,
) -> Result<(u32, String, String), OkError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        OkError::new(
            ErrKind::Io,
            format!("cannot read {shown}: {}", crate::interp::io_reason(&e)),
        )
    })?;
    let norm = crate::fmt::normalize(&text);
    let id = interp.sources.add(
        shown.to_string(),
        Some(path.to_path_buf()),
        norm.clone().into(),
    );
    Ok((id, text, norm))
}

/// `ok self check [--strict] [FILES...]`: parse and check without running
/// anything (SPEC.md 17). Errors exit with 1; warnings only with
/// `--strict`.
pub fn check(interp: &mut Interp, args: &[String], start: &Path) -> R<Value> {
    let (strict, files) = options_and_files(args, "--strict", "check")?;
    // (shown name, path, kind)
    let mut targets: Vec<(String, PathBuf, FileKind)> = Vec::new();
    let mut tasks = None;
    if files.is_empty() {
        let Some(root) = crate::project::discover(start) else {
            return Err(usage(
                "no ok.ok found here; pass the files to check: ok self check FILE...",
            ));
        };
        // A load-time error (SPEC.md 15, 16.6) stops the check, since the
        // project's files are only known once it loads; it is reported
        // like a finding, with a summary line.
        let project = match interp.load_project(&root) {
            Ok(p) => p,
            Err(Flow::Err(e)) => {
                eprint!("{}", render(&e, &interp.sources, Color::Never));
                eprintln!("ok: the project failed to load: 1 error");
                return Err(Flow::Exit(1));
            }
            Err(other) => return Err(other),
        };
        tasks = Some(project.tasks.len());
        for (rel, path) in &project.trust_files {
            // Files a project imports are libraries (SPEC.md 11).
            let kind = match kind_of(path) {
                FileKind::Script => FileKind::Library,
                k => k,
            };
            targets.push((rel.clone(), path.clone(), kind));
        }
    } else {
        for f in &files {
            let shown = OkPath::from_native(f).display();
            targets.push((shown, f.clone(), kind_of(f)));
        }
    }
    let (mut errors, mut warnings) = (0, 0);
    for (shown, path, kind) in &targets {
        let findings = match read_source(interp, shown, path) {
            Err(e) => vec![e],
            Ok((id, text)) => match crate::parser::parse(id, &text)
                .and_then(|prog| check_items(&prog, *kind).map(|()| prog))
            {
                Err(e) => vec![e],
                Ok(prog) => crate::check::check_file(&prog, &text, shown, path, *kind),
            },
        };
        for e in &findings {
            if e.warning {
                warnings += 1;
            } else {
                errors += 1;
            }
            eprint!("{}", render(e, &interp.sources, Color::Never));
        }
    }
    let n = targets.len();
    let what = match tasks {
        Some(t) => format!("{n} file{} and {t} task{} checked", plural(n), plural(t)),
        None => format!("{n} file{} checked", plural(n)),
    };
    if errors == 0 && warnings == 0 {
        println!("ok: {what}, no problems found");
        return Ok(Value::Null);
    }
    let mut found = Vec::new();
    if errors > 0 {
        found.push(format!("{errors} error{}", plural(errors)));
    }
    if warnings > 0 {
        found.push(format!("{warnings} warning{}", plural(warnings)));
    }
    eprintln!("ok: {what}: {}", found.join(" and "));
    if errors > 0 || strict {
        return Err(Flow::Exit(1));
    }
    Ok(Value::Null)
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `ok self fmt [--check] [FILES...]`.
pub fn fmt(interp: &mut Interp, args: &[String], start: &Path) -> R<Value> {
    let (check_only, mut files) = options_and_files(args, "--check", "fmt")?;
    if files.is_empty() {
        let Some(root) = crate::project::discover(start) else {
            return Err(usage(
                "no ok.ok found here; pass the files to format: ok self fmt FILE...",
            ));
        };
        let project = interp.load_project(&root)?;
        files = project.trust_files.iter().map(|(_, p)| p.clone()).collect();
    }
    let mut changed = Vec::new();
    let mut failed = 0;
    for f in &files {
        let shown = OkPath::from_native(f).display();
        let result = read_for_format(interp, &shown, f).and_then(|(id, text, norm)| {
            crate::fmt::format_file(id, &norm).map(|formatted| (text, formatted))
        });
        let (text, formatted) = match result {
            Ok(r) => r,
            Err(mut e) => {
                if e.span.is_none() {
                    e.message = format!("{shown}: {}", e.message);
                }
                eprint!("{}", render(&e, &interp.sources, Color::Never));
                failed += 1;
                continue;
            }
        };
        if formatted != text {
            changed.push(shown.clone());
            if !check_only {
                std::fs::write(f, formatted).map_err(|e| {
                    OkError::new(
                        ErrKind::Io,
                        format!("cannot write {shown}: {}", crate::interp::io_reason(&e)),
                    )
                })?;
                println!("formatted {shown}");
            }
        }
    }
    if check_only {
        for c in &changed {
            println!("would reformat {c}");
        }
    }
    if failed > 0 || (check_only && !changed.is_empty()) {
        return Err(Flow::Exit(1));
    }
    if changed.is_empty() {
        println!(
            "ok: {} file{} already formatted",
            files.len(),
            plural(files.len())
        );
    }
    Ok(Value::Null)
}

/// The starter `ok.ok`; `requires` is filled in with the running version's
/// major.minor, the oldest ok the file is known to work with (SPEC.md 15.7).
const STARTER: &str = r#"requires "{requires}"

# Say hello
task hello(name: string = "world") {
  print("hello, {name}!")
}
"#;

/// `0.2` for `0.2.0`: the form `requires` takes.
fn major_minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// `ok self init [--checksums FILE]`: a starter `ok.ok` and the bootstrap
/// wrappers, which pin this version and the checksums of its release.
pub fn init(args: &[String], start: &Path) -> R<Value> {
    let mut sums_file: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--checksums" => {
                i += 1;
                sums_file = Some(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| usage("--checksums needs a SHA256SUMS file"))?,
                );
            }
            other => {
                return Err(usage(&format!(
                    "unknown option {other} for ok self init (it takes --checksums FILE)"
                )));
            }
        }
        i += 1;
    }
    let version = crate::VERSION;
    let base = std::env::var("OK_DOWNLOAD_URL")
        .unwrap_or_else(|_| format!("https://github.com/xtr-dev/ok/releases/download/v{version}"));
    let sums_text = match &sums_file {
        Some(f) => std::fs::read_to_string(f).map_err(|e| OkError::new(ErrKind::Io, format!("cannot read {f}: {}", crate::interp::io_reason(&e))))?,
        None => download_text(&format!("{base}/SHA256SUMS")).map_err(|m| {
            OkError::new(ErrKind::Net, format!("cannot download the checksums of ok {version}: {m}"))
                .with_help("pass a downloaded SHA256SUMS file with --checksums FILE, or set OK_DOWNLOAD_URL to a mirror")
        })?,
    };
    let mut sums: Vec<(&str, String)> = Vec::new();
    for (target, file) in TARGETS {
        let sum = sums_text.lines().find_map(|l| {
            let mut parts = l.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (name == *file && hash.len() == 64).then(|| hash.to_ascii_lowercase())
        });
        match sum {
            Some(s) => sums.push((target, s)),
            None => {
                return Err(OkError::new(
                    ErrKind::Value,
                    format!("the checksums have no entry for {file}"),
                )
                .into());
            }
        }
    }
    let sum = |t: &str| {
        sums.iter()
            .find(|(x, _)| *x == t)
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    };
    let dir = start.to_path_buf();
    let outputs = [
        (
            "ok.ok",
            STARTER.replace("{requires}", &major_minor(version)),
        ),
        ("ok", sh_wrapper(version, &sum)),
        ("ok.ps1", ps_wrapper(version, &sum)),
        ("ok.cmd", CMD_WRAPPER.replace('\n', "\r\n")),
    ];
    for (name, _) in &outputs {
        if dir.join(name).exists() {
            return Err(OkError::new(
                ErrKind::Io,
                format!("{name} already exists; ok self init does not overwrite files"),
            )
            .into());
        }
    }
    for (name, content) in &outputs {
        let content = if *name == "ok.ps1" {
            content.replace('\n', "\r\n")
        } else {
            content.clone()
        };
        std::fs::write(dir.join(name), content).map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!("cannot write {name}: {}", crate::interp::io_reason(&e)),
            )
        })?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir.join("ok"), std::fs::Permissions::from_mode(0o755));
    }
    println!(
        "wrote ok.ok, ./ok, ok.cmd and ok.ps1 (pinned to ok {version})\nrun ./ok hello (or ok.cmd hello on Windows)"
    );
    Ok(Value::Null)
}

fn download_text(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

fn sh_wrapper(version: &str, sum: &dyn Fn(&str) -> String) -> String {
    format!(
        r#"#!/bin/sh
# ok bootstrap wrapper, written by `ok self init`.
# It downloads the pinned ok release for this machine once, checks its
# SHA-256, keeps it in the user cache, and runs it with all arguments.
set -eu
OK_VERSION="{version}"
OK_BASE_URL="${{OK_DOWNLOAD_URL:-https://github.com/xtr-dev/ok/releases/download/v$OK_VERSION}}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)                target=x86_64-unknown-linux-musl;  sum={lx} ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl; sum={la} ;;
  Darwin-x86_64)               target=x86_64-apple-darwin;        sum={mx} ;;
  Darwin-arm64)                target=aarch64-apple-darwin;       sum={ma} ;;
  *) echo "ok: no prebuilt ok for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

cache="${{XDG_CACHE_HOME:-$HOME/.cache}}/ok/$OK_VERSION"
bin="$cache/ok-$target"
if [ ! -x "$bin" ]; then
  mkdir -p "$cache"
  tmp="$bin.download.$$"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$OK_BASE_URL/ok-$target" -o "$tmp"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$OK_BASE_URL/ok-$target" -O "$tmp"
  else
    echo "ok: curl or wget is needed to download ok" >&2; exit 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp" | cut -d' ' -f1)
  else
    actual=$(shasum -a 256 "$tmp" | cut -d' ' -f1)
  fi
  if [ "$actual" != "$sum" ]; then
    rm -f "$tmp"
    echo "ok: checksum mismatch for ok-$target (expected $sum, got $actual); not running it" >&2
    exit 1
  fi
  chmod +x "$tmp"
  mv "$tmp" "$bin"
fi
exec "$bin" "$@"
"#,
        lx = sum("x86_64-unknown-linux-musl"),
        la = sum("aarch64-unknown-linux-musl"),
        mx = sum("x86_64-apple-darwin"),
        ma = sum("aarch64-apple-darwin"),
    )
}

fn ps_wrapper(version: &str, sum: &dyn Fn(&str) -> String) -> String {
    format!(
        r#"# ok bootstrap wrapper for Windows, written by `ok self init`.
# It downloads the pinned ok release for this machine once, checks its
# SHA-256, keeps it in the user cache, and runs it with all arguments.
$ErrorActionPreference = 'Stop'
$Version = '{version}'
$Base = if ($env:OK_DOWNLOAD_URL) {{ $env:OK_DOWNLOAD_URL }} else {{ "https://github.com/xtr-dev/ok/releases/download/v$Version" }}
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($Arch) {{
    'X64'   {{ $Target = 'x86_64-pc-windows-msvc';  $Sum = '{wx}' }}
    'Arm64' {{ $Target = 'aarch64-pc-windows-msvc'; $Sum = '{wa}' }}
    default {{ Write-Error "ok: no prebuilt ok for Windows $Arch"; exit 1 }}
}}
$Cache = Join-Path $env:LOCALAPPDATA "ok\$Version"
$Bin = Join-Path $Cache "ok-$Target.exe"
if (-not (Test-Path $Bin)) {{
    New-Item -ItemType Directory -Force -Path $Cache | Out-Null
    $Tmp = "$Bin.download"
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/ok-$Target.exe" -OutFile $Tmp
    $Actual = (Get-FileHash -Algorithm SHA256 $Tmp).Hash.ToLower()
    if ($Actual -ne $Sum) {{
        Remove-Item $Tmp
        Write-Error "ok: checksum mismatch for ok-$Target.exe (expected $Sum, got $Actual); not running it"
        exit 1
    }}
    Move-Item -Force $Tmp $Bin
}}
& $Bin @args
exit $LASTEXITCODE
"#,
        wx = sum("x86_64-pc-windows-msvc"),
        wa = sum("aarch64-pc-windows-msvc"),
    )
}

const CMD_WRAPPER: &str = "@echo off\nrem ok bootstrap wrapper for cmd.exe, written by `ok self init`; the work is done by ok.ps1.\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0ok.ps1\" %*\nexit /b %ERRORLEVEL%\n";
