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

/// A file being written beside its final path, so that a failure before
/// `commit` leaves an existing file untouched.
///
/// `write` (without `append`), `zip`, `tar` and `fetch(to:)` serialise into a
/// hidden temporary file in the target's directory and rename it over the
/// target at the end. Writing through a symlink replaces the file it points
/// to, and the new file keeps the old file's permissions. A target that
/// exists but is not a regular file (`/dev/null`, a named pipe), or sits in
/// a directory where no new file may be created, is written directly.
/// Dropping the value without committing removes the temporary file.
pub(crate) struct Replacement {
    /// `None` when the target is written directly.
    tmp: Option<PathBuf>,
    target: PathBuf,
    done: bool,
}

impl Replacement {
    /// Create the temporary file for `target`, whose parent must exist.
    /// Fails as writing `target` itself would when it is a directory or
    /// cannot be written.
    pub(crate) fn create(target: &Path) -> std::io::Result<(Replacement, std::fs::File)> {
        let target = match std::fs::symlink_metadata(target) {
            Ok(m) if m.file_type().is_symlink() => {
                crate::path::canonical(target).unwrap_or_else(|_| target.to_path_buf())
            }
            _ => target.to_path_buf(),
        };
        if let Ok(meta) = std::fs::metadata(&target) {
            if meta.is_dir() {
                return Err(std::io::ErrorKind::IsADirectory.into());
            }
            if !meta.is_file() {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&target)?;
                let r = Replacement {
                    tmp: None,
                    target,
                    done: true,
                };
                return Ok((r, file));
            }
            // A read-only file stays protected: renaming over it would
            // otherwise succeed wherever its directory is writable.
            std::fs::OpenOptions::new().write(true).open(&target)?;
        }
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let created =
            create_unique(|| target.with_file_name(format!(".{name}.ok-tmp-{}", unique_suffix())));
        let (tmp, file) = match created {
            Ok(f) => f,
            // A writable file in a directory where no new file may be
            // created can still be written, as before, just not safely.
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && target.is_file() => {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&target)?;
                let r = Replacement {
                    tmp: None,
                    target,
                    done: true,
                };
                return Ok((r, file));
            }
            Err(e) => return Err(e),
        };
        Ok((
            Replacement {
                tmp: Some(tmp),
                target,
                done: false,
            },
            file,
        ))
    }

    /// Move the finished file into place. The caller must have flushed and
    /// dropped its handle (Windows cannot rename an open file).
    pub(crate) fn commit(mut self) -> std::io::Result<()> {
        let Some(tmp) = self.tmp.clone() else {
            return Ok(());
        };
        if let Some(perms) = final_permissions(&self.target) {
            let _ = std::fs::set_permissions(&tmp, perms);
        }
        if std::fs::rename(&tmp, &self.target).is_err() {
            // Some WASI hosts cannot rename; the bytes are complete, so
            // copying them over the target is the next best thing.
            std::fs::copy(&tmp, &self.target)?;
            let _ = std::fs::remove_file(&tmp);
        }
        self.done = true;
        Ok(())
    }
}

impl Drop for Replacement {
    fn drop(&mut self) {
        if let (false, Some(tmp)) = (self.done, &self.tmp) {
            let _ = std::fs::remove_file(tmp);
        }
    }
}

/// A name part that other users cannot predict, so they cannot claim the
/// name first: the process id, a counter, and 64 bits from the randomly
/// keyed hasher of the standard library.
pub(crate) fn unique_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(n);
    if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        h.write_u128(d.as_nanos());
    }
    format!("{}-{n}-{:016x}", process_id(), h.finish())
}

/// Create a new file that only its owner can open (mode 0600 on Unix), at
/// the first free name `name()` gives. A name that exists already, whether
/// by chance or planted, is skipped rather than opened.
pub(crate) fn create_unique(
    mut name: impl FnMut() -> PathBuf,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    let mut last = None;
    for _ in 0..16 {
        let p = name();
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        match o.open(&p) {
            Ok(f) => return Ok((p, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::ErrorKind::AlreadyExists.into()))
}

/// The permissions a finished replacement gets: those of the file it
/// replaces, without setuid, setgid and sticky bits (the new file may have
/// a different owner), or for a new file what creating it normally gives.
/// `None` leaves the temporary file's own permissions.
fn final_permissions(target: &Path) -> Option<std::fs::Permissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = match std::fs::metadata(target) {
            Ok(meta) => meta.permissions().mode() & 0o777,
            Err(_) => 0o666 & !umask(),
        };
        Some(std::fs::Permissions::from_mode(mode))
    }
    // Windows has only the read-only flag, which a writable target does
    // not have and a new file does not get.
    #[cfg(not(unix))]
    {
        std::fs::metadata(target).ok().map(|m| m.permissions())
    }
}

/// The process's file mode creation mask, read once. Reading it means
/// setting it, so it is set back at once; the threads ok runs meanwhile
/// (output readers and timeout watchdogs) create no files.
#[cfg(unix)]
fn umask() -> u32 {
    use nix::sys::stat::{Mode, umask};
    static MASK: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MASK.get_or_init(|| {
        let old = umask(Mode::from_bits_truncate(0o077));
        umask(old);
        // `mode_t` is u32 on Linux but u16 on macOS.
        #[allow(clippy::useless_conversion)]
        u32::from(old.bits())
    })
}

/// Why a copy stopped.
enum CopyFail {
    Io(std::io::Error),
    /// A conflict in the destination, described.
    Refused(String),
}

impl From<std::io::Error> for CopyFail {
    fn from(e: std::io::Error) -> CopyFail {
        CopyFail::Io(e)
    }
}

/// Copy a file, link or directory tree (SPEC.md 12.1). `src` itself is
/// followed when it is a symlink and `follow` is set (`copy` copies what a
/// link names; `move` moves the link). Below `src`, symlinks are copied as
/// links, never followed, so a link in the source cannot pull files from
/// outside it into the copy. Below `dst`, existing links are never written
/// through or descended into: a link where a file goes is replaced, a link
/// where a directory goes is an error.
fn copy_tree(src: &Path, dst: &Path, follow: bool) -> Result<(), CopyFail> {
    let meta = if follow {
        std::fs::metadata(src)?
    } else {
        std::fs::symlink_metadata(src)?
    };
    if meta.file_type().is_symlink() {
        ensure_parent(dst)?;
        return copy_link(src, dst);
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dst)?;
        copy_entries(src, dst)
    } else {
        ensure_parent(dst)?;
        std::fs::copy(src, dst)?;
        Ok(())
    }
}

fn copy_entries(src: &Path, dst: &Path) -> Result<(), CopyFail> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let (s, d) = (entry.path(), dst.join(entry.file_name()));
        let kind = entry.file_type()?;
        let existing = std::fs::symlink_metadata(&d).ok();
        let shown = || OkPath::from_native(&d).display();
        match &existing {
            Some(m) if m.file_type().is_symlink() && kind.is_dir() => {
                return Err(CopyFail::Refused(format!(
                    "{} is a symbolic link, and copy never writes through links",
                    shown()
                )));
            }
            Some(m) if m.is_dir() && !kind.is_dir() => {
                return Err(CopyFail::Refused(format!("{} is a directory", shown())));
            }
            Some(m) if !m.is_dir() && !m.file_type().is_symlink() && kind.is_dir() => {
                return Err(CopyFail::Refused(format!(
                    "{} is a file, not a directory",
                    shown()
                )));
            }
            // A link where a file or link goes is replaced, not followed.
            Some(m) if m.file_type().is_symlink() => std::fs::remove_file(&d)?,
            _ => {}
        }
        if kind.is_symlink() {
            copy_link(&s, &d)?;
        } else if kind.is_dir() {
            if existing.is_none() {
                std::fs::create_dir(&d)?;
            }
            copy_entries(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

/// Recreate the symlink `src` at `dst`, with the same target text.
fn copy_link(src: &Path, dst: &Path) -> Result<(), CopyFail> {
    let target = std::fs::read_link(src)?;
    if std::fs::symlink_metadata(dst).is_ok_and(|m| !m.is_dir()) {
        std::fs::remove_file(dst)?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, dst)?;
        Ok(())
    }
    // Windows has separate links for files and directories. Creating one
    // may need a privilege; without it the copy fails instead of copying
    // what the link points to.
    #[cfg(windows)]
    {
        let made = if std::fs::metadata(src).is_ok_and(|m| m.is_dir()) {
            std::os::windows::fs::symlink_dir(&target, dst)
        } else {
            std::os::windows::fs::symlink_file(&target, dst)
        };
        made.map_err(|e| {
            CopyFail::Refused(format!(
                "cannot create the symbolic link {}: {}",
                OkPath::from_native(dst).display(),
                io_reason(&e)
            ))
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = target;
        Err(CopyFail::Refused(format!(
            "{} is a symbolic link, which cannot be created here",
            OkPath::from_native(src).display()
        )))
    }
}

fn copy(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "copy", &[])?;
    count_args(&args, "copy", 2, 2)?;
    let (src, dst) = (path_at(&args, 0, "copy")?, path_at(&args, 1, "copy")?);
    if !interp.mutation(&format!(
        "copy {} {}",
        crate::command::display_arg(&src.display()),
        crate::command::display_arg(&dst.display())
    )) {
        return Ok(Value::Null);
    }
    let (s, d) = (src.native(), dst.native());
    if !s.exists() {
        return Err(OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot copy {}: no such file or directory", src.display()),
        )
        .into());
    }
    check_not_into_itself(&args, "copy", &src, &dst)?;
    match (s.is_dir(), std::fs::metadata(&d).map(|m| m.is_dir())) {
        (false, Ok(true)) => {
            // `dst` is the exact destination (SPEC.md 12.1): a file cannot
            // replace a directory, and ok never copies "into" one.
            return Err(OkError::at(
                ErrKind::Io,
                args.span,
                format!(
                    "cannot copy {} to {}: {} is a directory",
                    src.display(),
                    dst.display(),
                    dst.display()
                ),
            )
            .with_help(format!(
                "`copy` names the exact destination; to copy into the directory, write copy({}, {})",
                Value::str(src.display()).repr(),
                Value::str(dst.join_str(&src.name()).display()).repr()
            ))
            .into());
        }
        (true, Ok(false)) => {
            return Err(OkError::at(
                ErrKind::Io,
                args.span,
                format!(
                    "cannot copy the directory {} to {}: {} is a file",
                    src.display(),
                    dst.display(),
                    dst.display()
                ),
            )
            .with_help(format!(
                "remove the file first: remove({})",
                Value::str(dst.display()).repr()
            ))
            .into());
        }
        _ => {}
    }
    copy_tree(&s, &d, true).map_err(|e| copy_fail(&args, "copy", &src, &dst, e))?;
    Ok(Value::Null)
}

fn copy_fail(
    args: &Args,
    what: &str,
    src: &OkPath,
    dst: &OkPath,
    e: CopyFail,
) -> crate::interp::Flow {
    match e {
        CopyFail::Io(e) => io_fail(args, &format!("{what} to"), dst, &e),
        CopyFail::Refused(msg) => OkError::at(
            ErrKind::Io,
            args.span,
            format!(
                "cannot {what} {} to {}: {msg}",
                src.display(),
                dst.display()
            ),
        )
        .into(),
    }
}

/// Where `p` is or would be: the canonical form of its nearest existing
/// ancestor with the rest of the path joined on.
fn resolved_target(p: &Path) -> PathBuf {
    let mut cur = crate::path::lexical_absolute(p);
    let mut tail = Vec::new();
    loop {
        if std::fs::symlink_metadata(&cur).is_ok() {
            let mut out = crate::path::canonical(&cur).unwrap_or(cur);
            out.extend(tail.iter().rev());
            return out;
        }
        match cur.file_name() {
            Some(name) => tail.push(name.to_os_string()),
            None => return crate::path::lexical_absolute(p),
        }
        cur.pop();
    }
}

/// Refuse to copy or move a path onto itself or a directory into itself:
/// copying a file onto itself would truncate it, and copying a directory
/// into itself would never end.
fn check_not_into_itself(args: &Args, what: &str, src: &OkPath, dst: &OkPath) -> R<()> {
    let (s, d) = (
        resolved_target(&src.native()),
        resolved_target(&dst.native()),
    );
    let msg = if s == d {
        format!("cannot {what} {} onto itself", src.display())
    } else if d.starts_with(&s) {
        format!(
            "cannot {what} {} into itself: {} is inside it",
            src.display(),
            dst.display()
        )
    } else {
        return Ok(());
    };
    Err(OkError::at(ErrKind::Io, args.span, msg).into())
}

fn move_(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "move", &[])?;
    count_args(&args, "move", 2, 2)?;
    let (src, dst) = (path_at(&args, 0, "move")?, path_at(&args, 1, "move")?);
    if !interp.mutation(&format!(
        "move {} {}",
        crate::command::display_arg(&src.display()),
        crate::command::display_arg(&dst.display())
    )) {
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
    if resolved_target(&s) != resolved_target(&d) {
        // Moving a path onto itself is a harmless no-op, but moving a
        // directory into itself would end in the copy fallback below.
        check_not_into_itself(&args, "move", &src, &dst)?;
    }
    ensure_parent(&d).map_err(|e| io_fail(&args, "move to", &dst, &e))?;
    if std::fs::rename(&s, &d).is_err() {
        // Across file systems a rename fails; copy and remove instead.
        copy_tree(&s, &d, false).map_err(|e| copy_fail(&args, "move", &src, &dst, e))?;
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
    let meta = std::fs::symlink_metadata(p).ok()?;
    let canon = crate::path::canonical(p).unwrap_or_else(|_| crate::path::lexical_absolute(p));
    // Removing a symlink removes only the link, so the directory it points
    // to is safe; a real directory may hold the working directory, which
    // `remove(".")`, `remove("..")` or `remove(cwd)` would delete from under
    // the script.
    if meta.is_dir()
        && let Ok(cwd) = std::env::current_dir()
    {
        let cwd = crate::path::canonical(&cwd).unwrap_or(cwd);
        if cwd == canon {
            return Some("the working directory");
        }
        if cwd.starts_with(&canon) {
            return Some("a directory that contains the working directory");
        }
    }
    if canon.parent().is_none() {
        return Some("the root of the file system");
    }
    if let Some(home) = home_dir().and_then(|h| crate::path::canonical(&h).ok()) {
        if home == canon {
            return Some("your home directory");
        }
        // `/home` or `/Users` holds every user's files.
        if meta.is_dir() && home.starts_with(&canon) {
            return Some("a directory that contains your home directory");
        }
    }
    if interp
        .project_root
        .as_ref()
        .and_then(|r| crate::path::canonical(r).ok())
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
        let err = OkError::at(
            ErrKind::Io,
            args.span,
            format!("refusing to remove {} because it is {what}", p.display()),
        );
        let err = match what {
            "the working directory" => err.with_help(format!(
                "to empty it, remove its entries instead: for f in list_dir({}) {{ remove(f) }}",
                Value::str(p.display()).repr()
            )),
            "a directory that contains your home directory" => err.with_help(
                "remove the entries you mean one by one; ok never removes a directory your home directory is in",
            ),
            "a directory that contains the working directory" => err.with_help(format!(
                "run the script from a directory outside {}, or remove the entries you mean one by one",
                p.display()
            )),
            _ => err,
        };
        return Err(err.into());
    }
    // The operation is traced (and shown by --dry-run) whether or not the
    // path exists, so the plan a dry run prints does not depend on what
    // earlier, skipped steps would have created (SPEC.md 9.9).
    let exists = std::fs::symlink_metadata(&native).is_ok();
    if !interp.mutation(&format!(
        "remove {}",
        crate::command::display_arg(&p.display())
    )) || !exists
    {
        return Ok(Value::Bool(exists));
    }
    remove_path(&native).map_err(|e| io_fail(&args, "remove", &p, &e))?;
    Ok(Value::Bool(true))
}

fn mkdir(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "mkdir", &[])?;
    count_args(&args, "mkdir", 1, 1)?;
    let p = path_at(&args, 0, "mkdir")?;
    // Traced like `remove`, also when the directory already exists.
    if interp.mutation(&format!(
        "mkdir {}",
        crate::command::display_arg(&p.display())
    )) && !p.native().is_dir()
    {
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
    let mut names: Vec<String> = Vec::new();
    for e in entries.filter_map(|e| e.ok()) {
        match e.file_name().into_string() {
            Ok(n) => names.push(n),
            Err(raw) => return Err(not_utf8(&args, &format!("list {}", p.display()), &raw)),
        }
    }
    names.sort();
    Ok(Value::list(
        names
            .into_iter()
            .map(|n| Value::path(p.join_str(&n)))
            .collect(),
    ))
}

/// A file name that is not UTF-8 cannot be a string or a path in ok;
/// converting it lossily would name a different file, which a later
/// `remove` could then delete.
pub(crate) fn not_utf8(args: &Args, what: &str, name: &std::ffi::OsStr) -> crate::interp::Flow {
    OkError::at(
        ErrKind::Io,
        args.span,
        format!("cannot {what}: the name {name:?} is not valid UTF-8"),
    )
    .with_help(
        "ok works only with UTF-8 file names; rename the file, or exclude it from the pattern",
    )
    .into()
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
    if !interp.mutation(&format!(
        "touch {}",
        crate::command::display_arg(&p.display())
    )) {
        return Ok(Value::Null);
    }
    let native = p.native();
    let existed = std::fs::symlink_metadata(&native).is_ok();
    ensure_parent(&native).map_err(|e| io_fail(&args, "touch", &p, &e))?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&native)
        .map_err(|e| io_fail(&args, "touch", &p, &e))?;
    if !existed {
        // A file that was just created already has the current time.
        return Ok(Value::Null);
    }
    match f.set_modified(std::time::SystemTime::now()) {
        Ok(()) => Ok(Value::Null),
        // Browser WASI hosts cannot set file times; there `touch` of an
        // existing file leaves its time unchanged (SPEC.md 19).
        Err(e) if cfg!(target_os = "wasi") && e.kind() == std::io::ErrorKind::Unsupported => {
            Ok(Value::Null)
        }
        Err(e) => Err(io_fail(&args, "touch", &p, &e)),
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, unpredictable name in the temporary directory.
fn temp_name(prefix: &str, ext: &str) -> PathBuf {
    temp_root().join(format!("{prefix}-{}{ext}", unique_suffix()))
}

/// The id of this process, for unique temporary names. WASI has no process
/// ids (`std::process::id` panics there) and runs one ok per instance, so
/// the WebAssembly build uses 0.
pub(crate) fn process_id() -> u32 {
    if cfg!(target_os = "wasi") {
        0
    } else {
        std::process::id()
    }
}

/// The directory `tempdir()` and `tempfile()` create their entries in.
///
/// WASI has no system temporary directory (`std::env::temp_dir` panics
/// there), so the WebAssembly build uses `TMPDIR` when it is set and
/// otherwise `.ok-tmp` in the working directory (SPEC.md 19).
fn temp_root() -> PathBuf {
    if cfg!(target_os = "wasi") {
        match std::env::var_os("TMPDIR").filter(|v| !v.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => crate::path::lexical_absolute(Path::new(WASI_TEMP)),
        }
    } else {
        std::env::temp_dir()
    }
}

/// The temporary directory of the WebAssembly build when `TMPDIR` is unset.
const WASI_TEMP: &str = ".ok-tmp";

/// Create the parent of a new temporary entry. When that parent is the
/// WebAssembly build's own `.ok-tmp` and did not exist yet, it is registered
/// for removal at exit, so a script leaves no trace in the working directory.
fn prepare_temp_parent(interp: &mut Interp, p: &Path) -> std::io::Result<()> {
    let Some(parent) = p.parent() else {
        return Ok(());
    };
    if parent.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(parent)?;
    if cfg!(target_os = "wasi") && parent.file_name().is_some_and(|n| n == WASI_TEMP) {
        // `cleanup` removes entries in reverse order, so this goes last.
        interp.temps.insert(0, parent.to_path_buf());
        crate::procs::track_temp(parent.to_path_buf());
    }
    Ok(())
}

fn tempdir(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "tempdir", &[])?;
    count_args(&args, "tempdir", 0, 0)?;
    // Only the owner may enter the directory (0700 on Unix), so another
    // user can neither read what the script puts in it nor swap its files.
    let created = prepare_temp_parent(interp, &temp_name("ok", "")).and_then(|()| {
        let mut last = None;
        for _ in 0..16 {
            let p = temp_name("ok", "");
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                b.mode(0o700);
            }
            match b.create(&p) {
                Ok(()) => return Ok(p),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.unwrap_or_else(|| std::io::ErrorKind::AlreadyExists.into()))
    });
    let p = created.map_err(|e| {
        OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot create a temporary directory: {}", io_reason(&e)),
        )
    })?;
    let Some(shown) = OkPath::try_from_native(&p) else {
        crate::procs::remove_temp(&p);
        return Err(not_utf8(&args, "make a temporary entry", p.as_os_str()));
    };
    interp.track_temp(p);
    Ok(Value::path(shown))
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
    // Created only if the name is free, readable only by its owner.
    let p = prepare_temp_parent(interp, &temp_name("ok", &ext))
        .and_then(|()| create_unique(|| temp_name("ok", &ext)))
        .map(|(p, _)| p)
        .map_err(|e| {
            OkError::at(
                ErrKind::Io,
                args.span,
                format!("cannot create a temporary file: {}", io_reason(&e)),
            )
        })?;
    let Some(shown) = OkPath::try_from_native(&p) else {
        crate::procs::remove_temp(&p);
        return Err(not_utf8(&args, "make a temporary entry", p.as_os_str()));
    };
    interp.track_temp(p);
    Ok(Value::path(shown))
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
    if !interp.mutation(&format!(
        "chmod {:o} {}",
        mode,
        crate::command::display_arg(&p.display())
    )) {
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
    )
    .map_err(|raw| not_utf8(&args, &format!("glob {pattern:?}"), &raw))?;
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
    )
    .map_err(|raw| format!("cannot glob {pattern:?}: the name {raw:?} is not valid UTF-8"))?;
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
) -> Result<(), std::ffi::OsString> {
    let mut w = Walk { hidden, m, ex, out };
    w.dir(dir, rel, depth, None)
}

/// One glob walk. Names that are not UTF-8 are matched in their lossy
/// form, and only a match below such a name is an error, so a stray file
/// that the pattern does not select cannot break the glob, and a selected
/// one is never returned under a wrong name.
struct Walk<'a> {
    hidden: bool,
    m: &'a globset::GlobMatcher,
    ex: &'a [globset::GlobMatcher],
    out: &'a mut Vec<String>,
}

impl Walk<'_> {
    fn dir(
        &mut self,
        dir: &Path,
        rel: &str,
        depth: usize,
        bad: Option<&std::ffi::OsStr>,
    ) -> Result<(), std::ffi::OsString> {
        if depth == 0 {
            return Ok(());
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(());
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let raw = entry.file_name();
            let name = raw.to_string_lossy().into_owned();
            if name.starts_with('.') && !self.hidden {
                continue;
            }
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if self.ex.iter().any(|e| e.is_match(&child_rel)) {
                continue;
            }
            let bad = bad.or_else(|| raw.to_str().is_none().then_some(raw.as_os_str()));
            if self.m.is_match(&child_rel) {
                if let Some(b) = bad {
                    return Err(b.to_os_string());
                }
                self.out.push(child_rel.clone());
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                self.dir(&entry.path(), &child_rel, depth.saturating_sub(1), bad)?;
            }
        }
        Ok(())
    }
}

// ----- paths ------------------------------------------------------------------

pub fn home_dir() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// `p` joined to the working directory, or the working directory's name
/// when it is not UTF-8.
fn absolute(p: &OkPath) -> Result<OkPath, std::ffi::OsString> {
    if p.is_abs() {
        return Ok(p.clone());
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    match OkPath::try_from_native(&cwd) {
        Some(c) => Ok(c.join(p)),
        None => Err(cwd.into_os_string()),
    }
}

fn abs(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "abs", &[])?;
    count_args(&args, "abs", 1, 1)?;
    let p = path_at(&args, 0, "abs")?;
    let a = absolute(&p)
        .map_err(|raw| not_utf8(&args, &format!("make {} absolute", p.display()), &raw))?;
    Ok(Value::path(a))
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
        let fail = |raw: std::ffi::OsString| not_utf8(&args, "make the paths absolute", &raw);
        (absolute(&p).map_err(fail)?, absolute(&base).map_err(fail)?)
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
        p.parts_with_root()
            .iter()
            .map(|s| Value::str(s.as_str()))
            .collect(),
    ))
}

fn is_abs(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "is_abs", &[])?;
    count_args(&args, "is_abs", 1, 1)?;
    Ok(Value::Bool(path_at(&args, 0, "is_abs")?.is_abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ok-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn replacement_dropped_keeps_the_old_file() {
        let dir = scratch("drop");
        let target = dir.join("keep.txt");
        std::fs::write(&target, "old").unwrap();
        let (pending, mut file) = Replacement::create(&target).unwrap();
        file.write_all(b"half of the new").unwrap();
        drop(file);
        drop(pending);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("keep.txt")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replacement_commit_replaces_and_keeps_permissions() {
        let dir = scratch("commit");
        let target = dir.join("tool.sh");
        std::fs::write(&target, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o750)).unwrap();
        }
        let (pending, mut file) = Replacement::create(&target).unwrap();
        file.write_all(b"new").unwrap();
        drop(file);
        pending.commit().unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o750);
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn replacement_is_private_until_committed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("private");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        // A new file: private while written, then the usual permissions.
        let target = dir.join("new.txt");
        let (pending, file) = Replacement::create(&target).unwrap();
        assert_eq!(mode(pending.tmp.as_ref().unwrap()), 0o600);
        drop(file);
        pending.commit().unwrap();
        assert_eq!(mode(&target), 0o666 & !umask());
        // An existing setuid file keeps its permission bits only.
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o4755)).unwrap();
        let (pending, file) = Replacement::create(&target).unwrap();
        assert_eq!(mode(pending.tmp.as_ref().unwrap()), 0o600);
        drop(file);
        pending.commit().unwrap();
        assert_eq!(mode(&target), 0o755);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_taken_temporary_name_is_skipped() {
        let dir = scratch("taken");
        let planted = dir.join("planted");
        std::fs::write(&planted, "someone else's").unwrap();
        let mut names = vec![dir.join("free"), planted.clone()];
        let (p, _) = create_unique(|| names.pop().unwrap()).unwrap();
        assert_eq!(p, dir.join("free"));
        assert_eq!(std::fs::read_to_string(&planted).unwrap(), "someone else's");
        assert_ne!(unique_suffix(), unique_suffix());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Links inside a copied tree stay links, and links inside an existing
    /// destination are never written through (SPEC.md 12.1).
    #[cfg(unix)]
    #[test]
    fn copy_keeps_links_and_never_writes_through_them() {
        use std::os::unix::fs::symlink;
        let dir = scratch("copy-links");
        let (outside, src, dst) = (dir.join("outside"), dir.join("src"), dir.join("dst"));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("key"), "secret").unwrap();
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("sub/f2"), "new").unwrap();
        symlink("../outside/key", src.join("k")).unwrap();
        symlink("../outside", src.join("d")).unwrap();
        assert!(copy_tree(&src, &dst, true).is_ok());
        assert_eq!(
            std::fs::read_link(dst.join("k")).unwrap(),
            Path::new("../outside/key")
        );
        assert!(
            std::fs::symlink_metadata(dst.join("d"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        // A link in the destination where the source has a directory.
        let dst2 = dir.join("dst2");
        std::fs::create_dir_all(&dst2).unwrap();
        symlink("../outside", dst2.join("sub")).unwrap();
        assert!(matches!(
            copy_tree(&src, &dst2, true),
            Err(CopyFail::Refused(m)) if m.contains("is a symbolic link")
        ));
        assert!(!outside.join("f2").exists());
        // A link in the destination where the source has a file is replaced.
        let dst3 = dir.join("dst3");
        std::fs::create_dir_all(dst3.join("sub")).unwrap();
        symlink("../../outside/key", dst3.join("sub/f2")).unwrap();
        assert!(copy_tree(&src.join("sub"), &dst3.join("sub"), true).is_ok());
        assert_eq!(
            std::fs::read_to_string(outside.join("key")).unwrap(),
            "secret"
        );
        assert_eq!(std::fs::read_to_string(dst3.join("sub/f2")).unwrap(), "new");
        // A link given as the source is followed by copy, kept by move.
        let (a, b) = (dir.join("a"), dir.join("b"));
        assert!(copy_tree(&src.join("k"), &a, true).is_ok());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "secret");
        assert!(
            !std::fs::symlink_metadata(&a)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(copy_tree(&src.join("k"), &b, false).is_ok());
        assert!(
            std::fs::symlink_metadata(&b)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A name that is not UTF-8 is never turned into the name of another
    /// file. macOS and Windows file systems cannot even store one.
    #[cfg(target_os = "linux")]
    #[test]
    fn names_that_are_not_utf8_are_refused_not_converted() {
        use std::os::unix::ffi::OsStrExt;
        let dir = scratch("not-utf8");
        std::fs::write(dir.join(std::ffi::OsStr::from_bytes(b"a\xff")), "").unwrap();
        std::fs::write(dir.join("a\u{FFFD}"), "").unwrap();
        std::fs::write(dir.join("b.txt"), "").unwrap();
        let run = |src: String| {
            let mut interp = Interp::new(Vec::new());
            match interp.run_source("<test>", &src, dir.clone()) {
                Ok(v) => v.display(),
                Err(crate::interp::Flow::Err(e)) => e.message,
                Err(_) => "other".into(),
            }
        };
        let d = Value::str(dir.to_str().unwrap()).repr();
        assert_eq!(
            run(format!("list_dir({d})")),
            "cannot list {}: the name \"a\\xFF\" is not valid UTF-8"
                .replace("{}", &OkPath::from_native(&dir).display())
        );
        assert!(run(format!("glob(\"*\", root: {d})")).contains("is not valid UTF-8"));
        assert_eq!(
            run(format!("glob(\"*.txt\", root: {d}).map(.name)")),
            "[\"b.txt\"]"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn replacement_refuses_a_directory() {
        let dir = scratch("dir");
        let err = Replacement::create(&dir).err().unwrap();
        assert_eq!(err.kind(), std::io::ErrorKind::IsADirectory);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
