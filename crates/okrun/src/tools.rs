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

/// `ok self check [FILES...]`: parse and check without running anything.
pub fn check(interp: &mut Interp, files: &[String], start: &Path) -> R<Value> {
    if files.is_empty() {
        let Some(root) = crate::project::discover(start) else {
            return Err(usage(
                "no ok.ok found here; pass the files to check: ok self check FILE...",
            ));
        };
        let project = interp.load_project(&root)?;
        let tasks = project.tasks.len();
        println!(
            "ok: {} file{} and {tasks} task{} checked, no problems found",
            project.trust_files.len(),
            plural(project.trust_files.len()),
            plural(tasks)
        );
        return Ok(Value::Null);
    }
    let mut failed = 0;
    for f in files {
        let path = Path::new(f);
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("error: cannot read {f}: {}", crate::interp::io_reason(&e));
                failed += 1;
                continue;
            }
        };
        let id = interp
            .sources
            .add(f.clone(), Some(path.to_path_buf()), text.clone().into());
        let result =
            crate::parser::parse(id, &text).and_then(|prog| check_items(&prog, kind_of(path)));
        if let Err(e) = result {
            eprint!("{}", render(&e, &interp.sources, Color::Never));
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(Flow::Exit(1));
    }
    println!(
        "ok: {} file{} checked, no problems found",
        files.len(),
        plural(files.len())
    );
    Ok(Value::Null)
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `ok self fmt [--check] [FILES...]`.
pub fn fmt(interp: &mut Interp, args: &[String], start: &Path) -> R<Value> {
    let check_only = args.iter().any(|a| a == "--check");
    let mut files: Vec<PathBuf> = args
        .iter()
        .filter(|a| *a != "--check")
        .map(PathBuf::from)
        .collect();
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
    for f in &files {
        let shown = OkPath::from_native(f).display();
        let text = std::fs::read_to_string(f).map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!("cannot read {shown}: {}", crate::interp::io_reason(&e)),
            )
        })?;
        let formatted = crate::fmt::format_source(&text).map_err(|mut e| {
            e.message = format!("{shown}: {}", e.message);
            e.span = None;
            e
        })?;
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
    if check_only && !changed.is_empty() {
        for c in &changed {
            println!("would reformat {c}");
        }
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

const STARTER: &str = r#"requires "0.1"

# Say hello
task hello(name: string = "world") {
  print("hello, {name}!")
}
"#;

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
        ("ok.ok", STARTER.to_string()),
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
