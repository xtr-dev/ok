//! Files and paths (SPEC.md 12.1, 12.2).
//!
//! Mutating functions announce themselves for `--trace` and are skipped by
//! `--dry-run` (`Interp::mutation`). Results that list files are sorted by
//! their `/` display form so they are the same on every OS.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::builtins::{arg_err, eager, type_err};
use crate::error::{ErrKind, OkError};
use crate::interp::{Args, Builtin, Interp, R, io_reason};
use crate::list::{bool_named, count_args, named};
use crate::path::OkPath;
use crate::value::{Record, Value};

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    eager("copy", copy),
    eager("move", move_),
    eager("remove", remove),
    eager("mkdir", mkdir),
    eager("exists", exists),
    eager("is_file", is_file),
    eager("is_dir", is_dir),
    eager("glob", glob),
    eager("list_dir", list_dir),
    eager("stat", stat),
    eager("touch", touch),
    eager("tempdir", tempdir),
    eager("tempfile", tempfile),
    eager("chmod", chmod),
    eager("abs", abs),
    eager("relative_to", relative_to),
    eager("with_ext", with_ext),
    eager("parts", parts),
    eager("is_abs", is_abs),
];

pub(crate) fn path_at(args: &Args, i: usize, fname: &str) -> R<OkPath> {
    match args.pos.get(i) {
        Some(Value::Str(s)) => Ok(OkPath::parse(s)),
        Some(Value::Path(p)) => Ok((**p).clone()),
        Some(other) => type_err(
            args,
            format!("`{fname}` needs a path, got {}", other.a_type()),
        ),
        None => arg_err(args, format!("`{fname}` needs a path")),
    }
}

fn io_fail(args: &Args, what: &str, p: &OkPath, e: &std::io::Error) -> crate::interp::Flow {
    OkError::at(
        ErrKind::Io,
        args.span,
        format!("cannot {what} {}: {}", p.display(), io_reason(e)),
    )
    .into()
}

pub(crate) fn ensure_parent(p: &Path) -> std::io::Result<()> {
    match p.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => std::fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        ensure_parent(dst)?;
        std::fs::copy(src, dst).map(|_| ())
    }
}

fn copy(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "copy", &[])?;
    count_args(&args, "copy", 2, 2)?;
    let (src, dst) = (path_at(&args, 0, "copy")?, path_at(&args, 1, "copy")?);
    if !interp.mutation(&format!("copy {} {}", src.display(), dst.display())) {
        return Ok(Value::Null);
    }
    if !src.native().exists() {
        return Err(OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot copy {}: no such file or directory", src.display()),
        )
        .into());
    }
    copy_tree(&src.native(), &dst.native()).map_err(|e| io_fail(&args, "copy to", &dst, &e))?;
    Ok(Value::Null)
}

fn move_(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "move", &[])?;
    count_args(&args, "move", 2, 2)?;
    let (src, dst) = (path_at(&args, 0, "move")?, path_at(&args, 1, "move")?);
    if !interp.mutation(&format!("move {} {}", src.display(), dst.display())) {
        return Ok(Value::Null);
    }
    let (s, d) = (src.native(), dst.native());
    if !s.exists() && std::fs::symlink_metadata(&s).is_err() {
        return Err(OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot move {}: no such file or directory", src.display()),
        )
        .into());
    }
    ensure_parent(&d).map_err(|e| io_fail(&args, "move to", &dst, &e))?;
    if std::fs::rename(&s, &d).is_err() {
        // Across file systems a rename fails; copy and remove instead.
        copy_tree(&s, &d).map_err(|e| io_fail(&args, "move to", &dst, &e))?;
        remove_path(&s).map_err(|e| io_fail(&args, "remove", &src, &e))?;
    }
    Ok(Value::Null)
}

fn remove_path(p: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(p)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// Paths `remove` refuses to delete (SPEC.md 12.1).
fn is_protected(interp: &Interp, p: &Path) -> Option<&'static str> {
    let canon = std::fs::canonicalize(p).ok()?;
    if canon.parent().is_none() {
        return Some("the root of the file system");
    }
    if home_dir()
        .and_then(|h| std::fs::canonicalize(h).ok())
        .is_some_and(|h| h == canon)
    {
        return Some("your home directory");
    }
    if interp
        .project_root
        .as_ref()
        .and_then(|r| std::fs::canonicalize(r).ok())
        .is_some_and(|r| r == canon)
    {
        return Some("the project root");
    }
    None
}

fn remove(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "remove", &[])?;
    count_args(&args, "remove", 1, 1)?;
    let p = path_at(&args, 0, "remove")?;
    let native = p.native();
    if let Some(what) = is_protected(interp, &native) {
        return Err(OkError::at(
            ErrKind::Io,
            args.span,
            format!("refusing to remove {} because it is {what}", p.display()),
        )
        .into());
    }
    if std::fs::symlink_metadata(&native).is_err() {
        return Ok(Value::Bool(false));
    }
    if !interp.mutation(&format!("remove {}", p.display())) {
        return Ok(Value::Bool(true));
    }
    remove_path(&native).map_err(|e| io_fail(&args, "remove", &p, &e))?;
    Ok(Value::Bool(true))
}

fn mkdir(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "mkdir", &[])?;
    count_args(&args, "mkdir", 1, 1)?;
    let p = path_at(&args, 0, "mkdir")?;
    if !p.native().is_dir() && interp.mutation(&format!("mkdir {}", p.display())) {
        std::fs::create_dir_all(p.native())
            .map_err(|e| io_fail(&args, "create the directory", &p, &e))?;
    }
    Ok(Value::path(p))
}

fn exists(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "exists", &[])?;
    count_args(&args, "exists", 1, 1)?;
    let p = path_at(&args, 0, "exists")?;
    Ok(Value::Bool(std::fs::symlink_metadata(p.native()).is_ok()))
}

fn is_file(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "is_file", &[])?;
    count_args(&args, "is_file", 1, 1)?;
    Ok(Value::Bool(
        path_at(&args, 0, "is_file")?.native().is_file(),
    ))
}

fn is_dir(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "is_dir", &[])?;
    count_args(&args, "is_dir", 1, 1)?;
    Ok(Value::Bool(path_at(&args, 0, "is_dir")?.native().is_dir()))
}

fn list_dir(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "list_dir", &[])?;
    count_args(&args, "list_dir", 0, 1)?;
    let p = if args.pos.is_empty() {
        OkPath::parse(".")
    } else {
        path_at(&args, 0, "list_dir")?
    };
    let entries = std::fs::read_dir(p.native()).map_err(|e| io_fail(&args, "list", &p, &e))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    Ok(Value::list(
        names
            .into_iter()
            .map(|n| Value::path(p.join_str(&n)))
            .collect(),
    ))
}

fn system_time(t: std::io::Result<std::time::SystemTime>) -> Value {
    match t {
        Ok(t) => match t.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => Value::Time(d.as_nanos().min(i64::MAX as u128) as i64),
            Err(e) => Value::Time(-(e.duration().as_nanos().min(i64::MAX as u128) as i64)),
        },
        Err(_) => Value::Null,
    }
}

fn stat(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "stat", &[])?;
    count_args(&args, "stat", 1, 1)?;
    let p = path_at(&args, 0, "stat")?;
    let link = std::fs::symlink_metadata(p.native()).map_err(|e| io_fail(&args, "stat", &p, &e))?;
    let meta = std::fs::metadata(p.native()).unwrap_or_else(|_| link.clone());
    let mut r = Record::new();
    r.insert("size", Value::Int(meta.len() as i64));
    r.insert("modified", system_time(meta.modified()));
    r.insert("created", system_time(meta.created()));
    r.insert("is_file", Value::Bool(meta.is_file()));
    r.insert("is_dir", Value::Bool(meta.is_dir()));
    r.insert("is_symlink", Value::Bool(link.file_type().is_symlink()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        r.insert(
            "mode",
            Value::Int((meta.permissions().mode() & 0o7777) as i64),
        );
    }
    #[cfg(not(unix))]
    r.insert("mode", Value::Null);
    Ok(Value::record(r))
}

fn touch(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "touch", &[])?;
    count_args(&args, "touch", 1, 1)?;
    let p = path_at(&args, 0, "touch")?;
    if !interp.mutation(&format!("touch {}", p.display())) {
        return Ok(Value::Null);
    }
    let native = p.native();
    ensure_parent(&native).map_err(|e| io_fail(&args, "touch", &p, &e))?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&native)
        .map_err(|e| io_fail(&args, "touch", &p, &e))?;
    f.set_modified(std::time::SystemTime::now())
        .map_err(|e| io_fail(&args, "touch", &p, &e))?;
    Ok(Value::Null)
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_name(prefix: &str, ext: &str) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos:08x}-{n}{ext}",
        std::process::id()
    ))
}

fn tempdir(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "tempdir", &[])?;
    count_args(&args, "tempdir", 0, 0)?;
    let p = temp_name("ok", "");
    std::fs::create_dir_all(&p).map_err(|e| {
        OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot create a temporary directory: {}", io_reason(&e)),
        )
    })?;
    interp.temps.push(p.clone());
    Ok(Value::path(OkPath::from_native(&p)))
}

fn tempfile(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "tempfile", &["ext"])?;
    count_args(&args, "tempfile", 0, 0)?;
    let ext = match opts.get("ext") {
        None => String::new(),
        Some(Value::Str(s)) if s.is_empty() => String::new(),
        Some(Value::Str(s)) => format!(".{}", s.trim_start_matches('.')),
        Some(other) => {
            return type_err(
                &args,
                format!("`ext` must be a string, got {}", other.a_type()),
            );
        }
    };
    let p = temp_name("ok", &ext);
    std::fs::File::create(&p).map_err(|e| {
        OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot create a temporary file: {}", io_reason(&e)),
        )
    })?;
    interp.temps.push(p.clone());
    Ok(Value::path(OkPath::from_native(&p)))
}

fn chmod(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "chmod", &[])?;
    count_args(&args, "chmod", 2, 2)?;
    let p = path_at(&args, 0, "chmod")?;
    let Value::Int(mode) = args.pos[1] else {
        return type_err(
            &args,
            format!(
                "`chmod` needs a mode like 0o755, got {}",
                args.pos[1].a_type()
            ),
        );
    };
    if !(0..=0o7777).contains(&mode) {
        return arg_err(
            &args,
            format!("{mode:o} is not a file mode; use an octal number like 0o755"),
        );
    }
    if !interp.mutation(&format!("chmod {:o} {}", mode, p.display())) {
        return Ok(Value::Null);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p.native(), std::fs::Permissions::from_mode(mode as u32))
            .map_err(|e| io_fail(&args, "chmod", &p, &e))?;
    }
    #[cfg(not(unix))]
    {
        // Windows has no permission bits; chmod does nothing (SPEC.md 12.1),
        // but the file must exist so mistakes are still reported.
        std::fs::metadata(p.native()).map_err(|e| io_fail(&args, "chmod", &p, &e))?;
    }
    Ok(Value::Null)
}

// ----- glob --------------------------------------------------------------------

fn compile(pattern: &str, args: &Args) -> R<globset::GlobMatcher> {
    globset::GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| {
            OkError::at(
                ErrKind::Value,
                args.span,
                format!("invalid glob pattern {pattern:?}: {}", e.kind()),
            )
            .into()
        })
}

/// The literal directory prefix of a pattern (components without glob
/// characters), where the walk starts.
fn literal_prefix(pattern: &str) -> (String, usize) {
    let comps: Vec<&str> = pattern.split('/').collect();
    let mut prefix = Vec::new();
    for c in &comps[..comps.len().saturating_sub(1)] {
        if c.contains(['*', '?', '[', '{']) {
            break;
        }
        prefix.push(*c);
    }
    let depth = if pattern.contains("**") {
        usize::MAX
    } else {
        comps.len() - prefix.len()
    };
    (prefix.join("/"), depth)
}

fn glob(_: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "glob", &["root", "hidden", "exclude"])?;
    count_args(&args, "glob", 1, 1)?;
    let Value::Str(pattern) = &args.pos[0] else {
        return type_err(
            &args,
            format!(
                "`glob` needs a pattern string like \"src/**/*.rs\", got {}",
                args.pos[0].a_type()
            ),
        );
    };
    let pattern = pattern.replace('\\', "/");
    let root = match opts.get("root") {
        None => OkPath::parse("."),
        Some(Value::Str(s)) => OkPath::parse(s),
        Some(Value::Path(p)) => (**p).clone(),
        Some(other) => {
            return type_err(
                &args,
                format!("`root` must be a path, got {}", other.a_type()),
            );
        }
    };
    let hidden = bool_named(&opts, "hidden", &args)?;
    let mut excludes = Vec::new();
    match opts.get("exclude") {
        None => {}
        Some(Value::List(items)) => {
            for i in items.iter() {
                let Value::Str(s) = i else {
                    return type_err(&args, "`exclude` must be a list of pattern strings");
                };
                excludes.push(compile(s, &args)?);
            }
        }
        Some(Value::Str(s)) => excludes.push(compile(s, &args)?),
        Some(other) => {
            return type_err(
                &args,
                format!(
                    "`exclude` must be a list of patterns, got {}",
                    other.a_type()
                ),
            );
        }
    }
    if pattern.starts_with('/') || OkPath::parse(&pattern).is_abs() {
        return arg_err(
            &args,
            "glob patterns are relative; pass the base directory as root: glob(\"*.txt\", root: dir)",
        );
    }
    // Dot-files are matched only when the pattern names them or hidden: true.
    let allow_hidden = hidden
        || pattern
            .split('/')
            .any(|c| c.starts_with('.') && c != "." && c != "..");
    let matcher = compile(&pattern, &args)?;
    let (prefix, depth) = literal_prefix(&pattern);
    let mut found: Vec<String> = Vec::new();
    let start_rel = prefix.clone();
    let start = if start_rel.is_empty() {
        root.native()
    } else {
        root.join_str(&start_rel).native()
    };
    walk(
        &start,
        &start_rel,
        depth,
        allow_hidden,
        &matcher,
        &excludes,
        &mut found,
    );
    found.sort();
    found.dedup();
    Ok(Value::list(
        found
            .into_iter()
            .map(|rel| Value::path(root.join_str(&rel)))
            .collect(),
    ))
}

/// Paths under `root` matching `pattern`, as native paths, sorted. Used by
/// the `sources`/`outputs` up-to-date checks.
pub fn glob_native(pattern: &str, root: &Path) -> Result<Vec<PathBuf>, String> {
    let pattern = pattern.replace('\\', "/");
    let matcher = globset::GlobBuilder::new(&pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| format!("invalid glob pattern {pattern:?}: {}", e.kind()))?;
    let allow_hidden = pattern
        .split('/')
        .any(|c| c.starts_with('.') && c != "." && c != "..");
    let (prefix, depth) = literal_prefix(&pattern);
    let start = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(OkPath::parse(&prefix).native())
    };
    let mut found = Vec::new();
    walk(
        &start,
        &prefix,
        depth,
        allow_hidden,
        &matcher,
        &[],
        &mut found,
    );
    found.sort();
    Ok(found
        .into_iter()
        .map(|rel| root.join(OkPath::parse(&rel).native()))
        .collect())
}

fn walk(
    dir: &Path,
    rel: &str,
    depth: usize,
    hidden: bool,
    m: &globset::GlobMatcher,
    ex: &[globset::GlobMatcher],
    out: &mut Vec<String>,
) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && !hidden {
            continue;
        }
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        if ex.iter().any(|e| e.is_match(&child_rel)) {
            continue;
        }
        if m.is_match(&child_rel) {
            out.push(child_rel.clone());
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            walk(
                &entry.path(),
                &child_rel,
                depth.saturating_sub(1),
                hidden,
                m,
                ex,
                out,
            );
        }
    }
}

// ----- paths ------------------------------------------------------------------

pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn absolute(p: &OkPath) -> OkPath {
    if p.is_abs() {
        return p.clone();
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    OkPath::from_native(&cwd).join(p)
}

fn abs(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "abs", &[])?;
    count_args(&args, "abs", 1, 1)?;
    Ok(Value::path(absolute(&path_at(&args, 0, "abs")?)))
}

fn relative_to(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "relative_to", &[])?;
    count_args(&args, "relative_to", 2, 2)?;
    let (p, base) = (
        path_at(&args, 0, "relative_to")?,
        path_at(&args, 1, "relative_to")?,
    );
    let (pa, ba) = if p.is_abs() == base.is_abs() {
        (p.clone(), base.clone())
    } else {
        (absolute(&p), absolute(&base))
    };
    let (pd, bd) = (pa.display(), ba.display());
    let rest = if bd == "." {
        Some(pd.clone())
    } else if pd == bd {
        Some(".".to_string())
    } else if bd.ends_with('/') {
        pd.strip_prefix(&bd).map(str::to_string)
    } else {
        pd.strip_prefix(&format!("{bd}/")).map(str::to_string)
    };
    match rest {
        Some(r) => Ok(Value::path(OkPath::parse(&r))),
        None => Err(OkError::at(
            ErrKind::Value,
            args.span,
            format!("{} is not inside {}", p.display(), base.display()),
        )
        .into()),
    }
}

fn with_ext(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "with_ext", &[])?;
    count_args(&args, "with_ext", 2, 2)?;
    let p = path_at(&args, 0, "with_ext")?;
    let Value::Str(ext) = &args.pos[1] else {
        return type_err(
            &args,
            format!(
                "the extension must be a string, got {}",
                args.pos[1].a_type()
            ),
        );
    };
    let name = p.name();
    if name.is_empty() {
        return arg_err(
            &args,
            format!("{} has no file name to give an extension", p.display()),
        );
    }
    let ext = ext.trim_start_matches('.');
    let new_name = if ext.is_empty() {
        p.stem()
    } else {
        format!("{}.{ext}", p.stem())
    };
    Ok(Value::path(p.parent().join_str(&new_name)))
}

fn parts(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "parts", &[])?;
    count_args(&args, "parts", 1, 1)?;
    let p = path_at(&args, 0, "parts")?;
    Ok(Value::list(
        p.parts().iter().map(|s| Value::str(s.as_str())).collect(),
    ))
}

fn is_abs(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "is_abs", &[])?;
    count_args(&args, "is_abs", 1, 1)?;
    Ok(Value::Bool(path_at(&args, 0, "is_abs")?.is_abs()))
}
