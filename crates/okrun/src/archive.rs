//! Archives: `zip`, `unzip`, `tar`, `untar` (SPEC.md 12.9).
//!
//! Members are stored with `/` paths relative to each source's parent, so
//! archiving the directory `dist` gives members `dist/...`. Extraction
//! refuses members that are absolute, escape the destination ("zip slip")
//! or have names that are not portable, never writes through a link, and
//! creates only links that point inside the destination; see the
//! extraction section below.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::builtins::{eager, type_err};
use crate::error::{ErrKind, OkError};
use crate::files::{ensure_parent, path_at};
use crate::interp::{Args, Builtin, Interp, R, io_reason};
use crate::list::{byte_size, count_args, limit_named, named};
use crate::path::OkPath;
use crate::value::Value;

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    eager("zip", zip),
    eager("unzip", unzip),
    eager("tar", tar),
    eager("untar", untar),
];

fn fail(args: &Args, msg: String) -> crate::interp::Flow {
    OkError::at(ErrKind::Io, args.span, msg).into()
}

/// The sources: one path or a list of paths.
fn sources(args: &Args, fname: &str) -> R<Vec<OkPath>> {
    match &args.pos[0] {
        Value::List(items) => items
            .iter()
            .map(|v| match v {
                Value::Str(s) => Ok(OkPath::parse(s)),
                Value::Path(p) => Ok((**p).clone()),
                other => type_err(
                    args,
                    format!("`{fname}` sources must be paths, got {}", other.a_type()),
                ),
            })
            .collect(),
        _ => Ok(vec![path_at(args, 0, fname)?]),
    }
}

/// Every file and directory under `src`, with its member name.
fn members(src: &OkPath) -> std::io::Result<Vec<(PathBuf, String, bool)>> {
    let mut out = Vec::new();
    let base = src.name();
    let base = if base.is_empty() {
        ".".to_string()
    } else {
        base
    };
    fn walk(dir: &Path, name: &str, out: &mut Vec<(PathBuf, String, bool)>) -> std::io::Result<()> {
        out.push((dir.to_path_buf(), name.to_string(), true));
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let raw = e.file_name();
            let Some(file) = raw.to_str() else {
                return Err(std::io::Error::other(format!(
                    "the name {raw:?} is not valid UTF-8"
                )));
            };
            let child = format!("{name}/{file}");
            if e.file_type()?.is_dir() {
                walk(&e.path(), &child, out)?;
            } else {
                out.push((e.path(), child, false));
            }
        }
        Ok(())
    }
    let native = src.native();
    if std::fs::metadata(&native)?.is_dir() {
        walk(&native, &base, &mut out)?;
    } else {
        out.push((native, base, false));
    }
    Ok(out)
}

#[cfg(unix)]
fn mode_of(p: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .ok()
        .map(|m| m.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_of(_: &Path) -> Option<u32> {
    None
}

/// The members of every source, listed before the archive file is created
/// so that an archive written inside a source directory does not contain
/// itself.
fn all_members(args: &Args, srcs: &[OkPath]) -> R<Vec<(PathBuf, String, bool)>> {
    let mut out = Vec::new();
    for src in srcs {
        out.extend(members(src).map_err(|e| {
            fail(
                args,
                format!("cannot read {}: {}", src.display(), io_reason(&e)),
            )
        })?);
    }
    Ok(out)
}

/// Start writing the archive `dst` through a temporary file, so that a
/// failure halfway leaves an existing archive as it was.
fn create_archive(args: &Args, dst: &OkPath) -> R<(crate::files::Replacement, File)> {
    let native = dst.native();
    ensure_parent(&native)
        .and_then(|()| crate::files::Replacement::create(&native))
        .map_err(|e| {
            fail(
                args,
                format!("cannot write {}: {}", dst.display(), io_reason(&e)),
            )
        })
}

fn zip(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "zip", &[])?;
    count_args(&args, "zip", 2, 2)?;
    let srcs = sources(&args, "zip")?;
    let dst = path_at(&args, 1, "zip")?;
    if !interp.mutation(&format!(
        "zip {} {}",
        srcs.iter()
            .map(|p| crate::command::display_arg(&p.display()))
            .collect::<Vec<_>>()
            .join(" "),
        crate::command::display_arg(&dst.display())
    )) {
        return Ok(Value::Null);
    }
    let list = all_members(&args, &srcs)?;
    let (pending, file) = create_archive(&args, &dst)?;
    let mut w = zip::ZipWriter::new(file);
    for (path, name, is_dir) in list {
        let mut opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        if let Some(m) = mode_of(&path) {
            opts = opts.unix_permissions(m);
        }
        let zerr =
            |e: zip::result::ZipError| fail(&args, format!("cannot write {}: {e}", dst.display()));
        if is_dir {
            w.add_directory(format!("{name}/"), opts).map_err(zerr)?;
        } else {
            w.start_file(name.as_str(), opts).map_err(zerr)?;
            let mut f = File::open(&path).map_err(|e| {
                fail(
                    &args,
                    format!(
                        "cannot read {}: {}",
                        OkPath::from_native(&path).display(),
                        io_reason(&e)
                    ),
                )
            })?;
            std::io::copy(&mut f, &mut w)
                .map_err(|e| fail(&args, format!("cannot write {}: {e}", dst.display())))?;
        }
    }
    let file = w
        .finish()
        .map_err(|e| fail(&args, format!("cannot write {}: {e}", dst.display())))?;
    drop(file);
    pending.commit().map_err(|e| {
        fail(
            &args,
            format!("cannot write {}: {}", dst.display(), io_reason(&e)),
        )
    })?;
    Ok(Value::Null)
}
fn tar(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "tar", &[])?;
    count_args(&args, "tar", 2, 2)?;
    let srcs = sources(&args, "tar")?;
    let dst = path_at(&args, 1, "tar")?;
    if !interp.mutation(&format!(
        "tar {} {}",
        srcs.iter()
            .map(|p| crate::command::display_arg(&p.display()))
            .collect::<Vec<_>>()
            .join(" "),
        crate::command::display_arg(&dst.display())
    )) {
        return Ok(Value::Null);
    }
    let list = all_members(&args, &srcs)?;
    let (pending, file) = create_archive(&args, &dst)?;
    let name = dst.name();
    let gzip = name.ends_with(".gz") || name.ends_with(".tgz");
    let writer: Box<dyn Write> = if gzip {
        Box::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::default(),
        ))
    } else {
        Box::new(file)
    };
    let mut b = tar::Builder::new(writer);
    b.follow_symlinks(false);
    for (path, member, is_dir) in list {
        let r = append_member(&mut b, &path, &member, is_dir);
        r.map_err(|e| {
            fail(
                &args,
                format!(
                    "cannot add {} to {}: {}",
                    OkPath::from_native(&path).display(),
                    dst.display(),
                    io_reason(&e)
                ),
            )
        })?;
    }
    let write_fail = |e: std::io::Error| {
        fail(
            &args,
            format!("cannot write {}: {}", dst.display(), io_reason(&e)),
        )
    };
    let mut inner = b.into_inner().map_err(write_fail)?;
    // Dropping a gzip encoder would finish the stream but swallow an error.
    inner.flush().map_err(write_fail)?;
    drop(inner);
    pending.commit().map_err(write_fail)?;
    Ok(Value::Null)
}

/// Add one file system entry to a tar archive.
#[cfg(not(target_os = "wasi"))]
fn append_member<W: Write>(
    b: &mut tar::Builder<W>,
    path: &Path,
    member: &str,
    is_dir: bool,
) -> std::io::Result<()> {
    if is_dir {
        b.append_dir(member, path)
    } else {
        b.append_path_with_name(path, member)
    }
}

/// Add one file system entry to a tar archive. The tar crate reads header
/// metadata only on Unix and Windows (elsewhere it panics), so on WASI the
/// header is filled in here: the modification time from the file system,
/// and the usual modes 0o755 for directories and 0o644 for files, since
/// WASI has no permission bits.
#[cfg(target_os = "wasi")]
fn append_member<W: Write>(
    b: &mut tar::Builder<W>,
    path: &Path,
    member: &str,
    is_dir: bool,
) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let mut h = tar::Header::new_gnu();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    h.set_mtime(mtime);
    if is_dir {
        h.set_entry_type(tar::EntryType::Directory);
        h.set_mode(0o755);
        h.set_size(0);
        b.append_data(&mut h, member, std::io::empty())
    } else if meta.file_type().is_symlink() {
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_mode(0o777);
        h.set_size(0);
        b.append_link(&mut h, member, std::fs::read_link(path)?)
    } else {
        h.set_entry_type(tar::EntryType::Regular);
        h.set_mode(0o644);
        h.set_size(meta.len());
        b.append_data(&mut h, member, File::open(path)?)
    }
}

// ----- extraction ---------------------------------------------------------------
//
// `unzip` and `untar` share one extractor that writes every file itself,
// because the archive is untrusted and the destination may already hold
// links (SPEC.md 12.9):
//
// - member names are checked component by component (`member_parts`) and
//   must be portable, so a name means the same thing on every OS;
// - directories are created one component at a time and nothing is ever
//   written through a symbolic link: a link found where a directory or a
//   file is about to be created stops the extraction;
// - symbolic links from a tar archive are created last, after every file,
//   with their target rewritten to its lexically normalised form and
//   checked to stay inside the destination. Since no member is written
//   through a link and every link this extraction creates points inside the
//   destination through real directories, chains of links cannot lead out;
// - files get the permissions of any new file (the umask applies), plus
//   execute permission when the member is executable;
// - the total size of the files and the number of entries are limited.

/// Default for `max_size:`: the most bytes of file content one extraction
/// writes.
const DEFAULT_MAX_SIZE: u64 = 4 << 30;
/// Default for `max_entries:`: the most members one extraction accepts.
const DEFAULT_MAX_ENTRIES: u64 = 1_000_000;

/// Why a member name or link target is refused.
#[derive(Debug, PartialEq)]
enum Unsafe {
    /// Absolute, or `..` climbs above the destination.
    Outside,
    /// A component that is not a portable file name, and why.
    Name(String, &'static str),
}

/// Names Windows reserves for devices, in any letter case and with any
/// extension (`nul.txt` is the device too).
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5",
    "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9",
];

/// Why `name` (one path component, not `.` or `..`) is not a portable file
/// name, or `None` when it is. Control characters are invalid in Windows
/// names and garble listings; a `:` would name a drive or an NTFS stream on
/// Windows, and `PathBuf::push` replaces the whole path with `C:x`; a `\` is a
/// separator there; Windows strips a trailing dot or space and opens a
/// device for a reserved name. Refusing these on every OS keeps an archive's
/// meaning the same everywhere.
fn unportable(name: &str) -> Option<&'static str> {
    if name.contains('\\') {
        return Some("it contains a backslash");
    }
    if name.contains(':') {
        return Some("it contains a colon");
    }
    if name.contains(|c: char| c.is_ascii_control()) {
        return Some("it contains a control character");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some("it ends in a dot or a space");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Some("it is a device name on Windows");
    }
    None
}

/// The components of member path `member` below the destination, with `.`
/// and empty components dropped and `..` applied. `up` is the number of
/// `..` allowed above the start (the depth of a link's directory); the
/// result's first element counts how many were used.
fn normalise(member: &str, up: usize) -> Result<(usize, Vec<String>), Unsafe> {
    if member.starts_with('/') {
        return Err(Unsafe::Outside);
    }
    let mut parts: Vec<String> = Vec::new();
    let mut ups = 0;
    for c in member.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    if ups == up {
                        return Err(Unsafe::Outside);
                    }
                    ups += 1;
                }
            }
            name => {
                if let Some(why) = unportable(name) {
                    return Err(Unsafe::Name(name.to_string(), why));
                }
                parts.push(name.to_string());
            }
        }
    }
    Ok((ups, parts))
}

/// The components of a member's path inside the destination.
fn member_parts(member: &str) -> Result<Vec<String>, Unsafe> {
    normalise(member, 0).map(|(_, parts)| parts)
}

/// A member path joined to `dst`, when it stays inside and is portable.
#[cfg(test)]
fn safe_join(dst: &Path, member: &str) -> Result<PathBuf, Unsafe> {
    let mut out = dst.to_path_buf();
    out.extend(member_parts(member)?);
    Ok(out)
}

/// Where a symbolic link at `link` (components inside the destination)
/// with target `target` points, as components inside the destination, and
/// the target rewritten relative to the link's directory without `.`, `..`
/// in the middle or repeated separators.
fn link_target(link: &[String], target: &str) -> Result<(Vec<String>, String), Unsafe> {
    if target.is_empty() {
        return Err(Unsafe::Name(String::new(), "the link has no target"));
    }
    let dir = &link[..link.len().saturating_sub(1)];
    let (ups, rest) = normalise(target, dir.len())?;
    let mut resolved: Vec<String> = dir[..dir.len() - ups].to_vec();
    resolved.extend(rest.iter().cloned());
    let mut written: Vec<&str> = vec![".."; ups];
    written.extend(rest.iter().map(String::as_str));
    let written = if written.is_empty() {
        ".".to_string()
    } else {
        written.join("/")
    };
    Ok((resolved, written))
}

/// A symbolic link from a tar archive, created after every other member.
struct PendingLink {
    name: String,
    parts: Vec<String>,
    target: String,
}

/// The state of one `unzip` or `untar` run.
struct Extractor<'a> {
    args: &'a Args,
    archive: &'a OkPath,
    dst: &'a OkPath,
    root: PathBuf,
    max_size: Option<u64>,
    max_entries: Option<u64>,
    size: u64,
    entries: u64,
    links: Vec<PendingLink>,
}

/// A member name as messages show it: quoted, with control and invisible
/// characters escaped, since it comes from the archive.
fn shown(name: &str) -> String {
    format!("\"{}\"", crate::error::terminal_safe_line(name))
}

impl<'a> Extractor<'a> {
    fn new(
        args: &'a Args,
        archive: &'a OkPath,
        dst: &'a OkPath,
        opts: &HashMap<String, Value>,
    ) -> R<Extractor<'a>> {
        Ok(Extractor {
            args,
            archive,
            dst,
            root: dst.native(),
            max_size: limit_named(opts, "max_size", DEFAULT_MAX_SIZE, args)?,
            max_entries: limit_named(opts, "max_entries", DEFAULT_MAX_ENTRIES, args)?,
            size: 0,
            entries: 0,
            links: Vec::new(),
        })
    }

    fn err(&self, msg: String) -> crate::interp::Flow {
        fail(self.args, msg)
    }

    fn io_err(&self, what: &str, p: &Path, e: &std::io::Error) -> crate::interp::Flow {
        self.err(format!(
            "cannot {what} {}: {}",
            OkPath::from_native(p).display(),
            io_reason(e)
        ))
    }

    /// Count one member against `max_entries`.
    fn count(&mut self) -> R<()> {
        self.entries += 1;
        if let Some(max) = self.max_entries
            && self.entries > max
        {
            return Err(OkError::at(
                ErrKind::Io,
                self.args.span,
                format!(
                    "refusing to extract {}: it has more than {max} members",
                    self.archive.display()
                ),
            )
            .with_help("pass max_entries: to allow more, or null for no limit")
            .into());
        }
        Ok(())
    }

    /// The member's components inside the destination, or the error that
    /// refuses it.
    fn parts(&self, name: &str) -> R<Vec<String>> {
        member_parts(name).map_err(|u| self.refuse(name, u))
    }

    fn refuse(&self, name: &str, why: Unsafe) -> crate::interp::Flow {
        match why {
            Unsafe::Outside => self.err(format!(
                "refusing to extract {} from {}: it would land outside {}",
                shown(name),
                self.archive.display(),
                self.dst.display()
            )),
            Unsafe::Name(part, why) => OkError::at(
                ErrKind::Io,
                self.args.span,
                format!(
                    "refusing to extract {} from {}: {} is not a portable file name ({why})",
                    shown(name),
                    self.archive.display(),
                    shown(&part)
                ),
            )
            .with_help("archive member names may not contain `:`, `\\` or control characters, end in a dot or space, or be a Windows device name like NUL")
            .into(),
        }
    }

    /// Create the directories `parts` below the destination one at a time,
    /// refusing to pass through a symbolic link, and return the last one.
    fn dirs(&self, name: &str, parts: &[String]) -> R<PathBuf> {
        let mut cur = self.root.clone();
        for part in parts {
            cur.push(part);
            match std::fs::symlink_metadata(&cur) {
                Ok(m) if m.is_dir() => {}
                Ok(m) if m.file_type().is_symlink() => return Err(self.through_link(name, &cur)),
                Ok(_) => {
                    return Err(self.err(format!(
                        "cannot extract {} from {}: {} is a file, not a directory",
                        shown(name),
                        self.archive.display(),
                        OkPath::from_native(&cur).display()
                    )));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    match std::fs::create_dir(&cur) {
                        Ok(()) => {}
                        // Created meanwhile by someone else: check it again.
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            if !std::fs::symlink_metadata(&cur).is_ok_and(|m| m.is_dir()) {
                                return Err(self.through_link(name, &cur));
                            }
                        }
                        Err(e) => return Err(self.io_err("create", &cur, &e)),
                    }
                }
                Err(e) => return Err(self.io_err("create", &cur, &e)),
            }
        }
        Ok(cur)
    }

    fn through_link(&self, name: &str, link: &Path) -> crate::interp::Flow {
        self.err(format!(
            "refusing to extract {} from {}: {} is a symbolic link, and extraction never writes through links",
            shown(name),
            self.archive.display(),
            OkPath::from_native(link).display()
        ))
    }

    /// The path of a new entry at `parts`: its directories exist, and
    /// whatever file or link was at the path itself is removed, so the new
    /// entry never writes through a link that was there.
    fn place(&self, name: &str, parts: &[String]) -> R<PathBuf> {
        let Some((last, dirs)) = parts.split_last() else {
            return Err(self.err(format!(
                "cannot extract {} from {}: it names the destination itself",
                shown(name),
                self.archive.display()
            )));
        };
        let mut out = self.dirs(name, dirs)?;
        out.push(last);
        match std::fs::symlink_metadata(&out) {
            Ok(m) if m.is_dir() => Err(self.err(format!(
                "cannot extract {} from {}: {} is a directory",
                shown(name),
                self.archive.display(),
                OkPath::from_native(&out).display()
            ))),
            Ok(_) => {
                std::fs::remove_file(&out).map_err(|e| self.io_err("replace", &out, &e))?;
                Ok(out)
            }
            Err(_) => Ok(out),
        }
    }

    /// Write a regular file from `data`, counting its bytes against
    /// `max_size`.
    fn file(
        &mut self,
        name: &str,
        parts: &[String],
        data: &mut dyn Read,
        executable: bool,
        mtime: Option<u64>,
    ) -> R<()> {
        let out = self.place(name, parts)?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // The umask applies to these, as to any file a program creates.
            opts.mode(if executable { 0o777 } else { 0o666 });
        }
        let _ = executable;
        let mut f = opts
            .open(&out)
            .map_err(|e| self.io_err("write", &out, &e))?;
        let room = self.max_size.map_or(u64::MAX, |m| m - self.size);
        let n = std::io::copy(&mut data.take(room.saturating_add(1)), &mut f).map_err(|e| {
            self.err(format!(
                "cannot extract {} from {}: {}",
                shown(name),
                self.archive.display(),
                io_reason(&e)
            ))
        })?;
        if n > room {
            drop(f);
            let _ = std::fs::remove_file(&out);
            return Err(OkError::at(
                ErrKind::Io,
                self.args.span,
                format!(
                    "refusing to extract {}: its files hold more than {}",
                    self.archive.display(),
                    byte_size(self.max_size.unwrap_or(u64::MAX))
                ),
            )
            .with_help("pass max_size: (in bytes) to allow more, or null for no limit")
            .into());
        }
        self.size += n;
        if let Some(secs) = mtime {
            let _ = f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs));
        }
        Ok(())
    }

    /// A hard link from a tar archive: a copy of a regular file that an
    /// earlier member put in the destination.
    fn hard_link(&mut self, name: &str, parts: &[String], target: &str) -> R<()> {
        let tparts = member_parts(target).map_err(|u| self.refuse_link(name, target, u))?;
        let Some((last, dirs)) = tparts.split_last() else {
            return Err(self.refuse_link(name, target, Unsafe::Outside));
        };
        let mut src = self.root.clone();
        for d in dirs {
            src.push(d);
            if std::fs::symlink_metadata(&src).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(self.through_link(name, &src));
            }
        }
        src.push(last);
        if !std::fs::symlink_metadata(&src).is_ok_and(|m| m.is_file()) {
            return Err(self.err(format!(
                "cannot extract the link {} -> {} from {}: the archive has no file {} before it",
                shown(name),
                shown(target),
                self.archive.display(),
                shown(target)
            )));
        }
        let mut data = File::open(&src).map_err(|e| self.io_err("read", &src, &e))?;
        let executable = mode_of(&src).is_some_and(|m| m & 0o111 != 0);
        self.file(name, parts, &mut data, executable, None)
    }

    fn refuse_link(&self, name: &str, target: &str, why: Unsafe) -> crate::interp::Flow {
        self.err(format!(
            "refusing to extract the link {} -> {} from {}: {}",
            shown(name),
            shown(target),
            self.archive.display(),
            match why {
                Unsafe::Outside => format!("it points outside {}", self.dst.display()),
                Unsafe::Name(_, why) if target.is_empty() => why.to_string(),
                Unsafe::Name(part, why) => {
                    format!("{} is not a portable file name ({why})", shown(&part))
                }
            }
        ))
    }

    /// Remember a symbolic link for the end, after checking its target.
    fn symlink(&mut self, name: &str, parts: Vec<String>, target: &str) -> R<()> {
        if cfg!(target_os = "wasi") {
            return Err(self.err(format!(
                "cannot extract the link {} from {}: the WebAssembly build of ok cannot create links",
                shown(name),
                self.archive.display()
            )));
        }
        let (_, written) =
            link_target(&parts, target).map_err(|u| self.refuse_link(name, target, u))?;
        self.links.push(PendingLink {
            name: name.to_string(),
            parts,
            target: written,
        });
        Ok(())
    }

    /// Create the remembered links. A target that passes through a link
    /// which was in the destination before (rather than one this
    /// extraction made) is refused, since that link may lead anywhere.
    fn finish_links(&mut self) -> R<()> {
        let links = std::mem::take(&mut self.links);
        let made: std::collections::HashSet<Vec<String>> =
            links.iter().map(|l| l.parts.clone()).collect();
        for link in &links {
            let (resolved, _) =
                link_target(&link.parts, &link.target).map_err(|u| self.refuse(&link.name, u))?;
            let mut cur = self.root.clone();
            for (i, part) in resolved.iter().enumerate() {
                cur.push(part);
                let is_link =
                    std::fs::symlink_metadata(&cur).is_ok_and(|m| m.file_type().is_symlink());
                if is_link && !made.contains(&resolved[..=i]) {
                    return Err(self.err(format!(
                        "refusing to extract the link {} -> {} from {}: {} is a symbolic link that was there before",
                        shown(&link.name),
                        shown(&link.target),
                        self.archive.display(),
                        OkPath::from_native(&cur).display()
                    )));
                }
            }
            let out = self.place(&link.name, &link.parts)?;
            make_symlink(&link.target, &cur, &out)
                .map_err(|e| self.io_err("create the link", &out, &e))?;
        }
        Ok(())
    }
}

#[cfg(unix)]
fn make_symlink(target: &str, _resolved: &Path, out: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, out)
}

/// Windows has separate links for files and directories; the kind follows
/// what the target is once every other member is extracted.
#[cfg(windows)]
fn make_symlink(target: &str, resolved: &Path, out: &Path) -> std::io::Result<()> {
    let target = target.replace('/', "\\");
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, out)
    } else {
        std::os::windows::fs::symlink_file(target, out)
    }
}

#[cfg(not(any(unix, windows)))]
fn make_symlink(_: &str, _: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("links are not supported here"))
}

fn unzip(interp: &mut Interp, mut args: Args) -> R<Value> {
    let limits = named(&mut args, "unzip", &["max_size", "max_entries"])?;
    count_args(&args, "unzip", 2, 2)?;
    let archive = path_at(&args, 0, "unzip")?;
    let dst = path_at(&args, 1, "unzip")?;
    if !interp.mutation(&format!(
        "unzip {} {}",
        crate::command::display_arg(&archive.display()),
        crate::command::display_arg(&dst.display())
    )) {
        return Ok(Value::Null);
    }
    let mut x = Extractor::new(&args, &archive, &dst, &limits)?;
    let file = File::open(archive.native()).map_err(|e| {
        fail(
            &args,
            format!("cannot read {}: {}", archive.display(), io_reason(&e)),
        )
    })?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| {
        fail(
            &args,
            format!("{} is not a valid zip archive: {e}", archive.display()),
        )
    })?;
    std::fs::create_dir_all(&x.root).map_err(|e| x.io_err("create", &x.root.clone(), &e))?;
    for i in 0..z.len() {
        x.count()?;
        let mut entry = z
            .by_index(i)
            .map_err(|e| fail(&args, format!("cannot read {}: {e}", archive.display())))?;
        let name = entry
            .name()
            .map(|n| n.into_owned())
            .map_err(|e| fail(&args, format!("cannot read {}: {e}", archive.display())))?;
        let parts = x.parts(&name)?;
        if entry.is_dir() {
            x.dirs(&name, &parts)?;
            continue;
        }
        if entry.is_symlink() {
            return Err(fail(
                &args,
                format!(
                    "refusing to extract the symbolic link {} from {}",
                    shown(&name),
                    archive.display()
                ),
            ));
        }
        let executable = entry.unix_mode().is_some_and(|m| m & 0o111 != 0);
        x.file(&name, &parts, &mut entry, executable, None)?;
    }
    Ok(Value::Null)
}

fn untar(interp: &mut Interp, mut args: Args) -> R<Value> {
    let limits = named(&mut args, "untar", &["max_size", "max_entries"])?;
    count_args(&args, "untar", 2, 2)?;
    let archive = path_at(&args, 0, "untar")?;
    let dst = path_at(&args, 1, "untar")?;
    if !interp.mutation(&format!(
        "untar {} {}",
        crate::command::display_arg(&archive.display()),
        crate::command::display_arg(&dst.display())
    )) {
        return Ok(Value::Null);
    }
    let mut x = Extractor::new(&args, &archive, &dst, &limits)?;
    let mut file = File::open(archive.native()).map_err(|e| {
        fail(
            &args,
            format!("cannot read {}: {}", archive.display(), io_reason(&e)),
        )
    })?;
    let mut magic = [0u8; 2];
    let n = file.read(&mut magic).unwrap_or(0);
    drop(file);
    let file = File::open(archive.native()).map_err(|e| {
        fail(
            &args,
            format!("cannot read {}: {}", archive.display(), io_reason(&e)),
        )
    })?;
    let reader: Box<dyn Read> = if n == 2 && magic == [0x1f, 0x8b] {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut a = tar::Archive::new(reader);
    std::fs::create_dir_all(&x.root).map_err(|e| x.io_err("create", &x.root.clone(), &e))?;
    let entries = a.entries().map_err(|e| {
        fail(
            &args,
            format!("{} is not a valid tar archive: {e}", archive.display()),
        )
    })?;
    for entry in entries {
        x.count()?;
        let mut entry =
            entry.map_err(|e| fail(&args, format!("cannot read {}: {e}", archive.display())))?;
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let kind = entry.header().entry_type();
        // Global and per-file metadata records carry no member.
        if matches!(
            kind,
            tar::EntryType::XGlobalHeader | tar::EntryType::XHeader
        ) {
            continue;
        }
        let parts = x.parts(&name)?;
        let target = || {
            entry
                .link_name_bytes()
                .map(|t| String::from_utf8_lossy(&t).into_owned())
                .unwrap_or_default()
        };
        match kind {
            tar::EntryType::Directory => {
                x.dirs(&name, &parts)?;
            }
            tar::EntryType::Regular | tar::EntryType::Continuous | tar::EntryType::GNUSparse => {
                let executable = entry.header().mode().is_ok_and(|m| m & 0o111 != 0);
                let mtime = entry.header().mtime().ok();
                x.file(&name, &parts, &mut entry, executable, mtime)?;
            }
            tar::EntryType::Link => {
                let t = target();
                x.hard_link(&name, &parts, &t)?;
            }
            tar::EntryType::Symlink => {
                let t = target();
                x.symlink(&name, parts, &t)?;
            }
            // Devices, pipes and other special entries are skipped.
            _ => {}
        }
    }
    x.finish_links()?;
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bad(member: &str) -> &'static str {
        match member_parts(member) {
            Err(Unsafe::Name(_, why)) => why,
            other => panic!("{member:?} gave {other:?}"),
        }
    }

    #[test]
    fn safe_join_keeps_members_inside() {
        let root = Path::new("dst");
        assert_eq!(safe_join(root, "a/b.txt"), Ok(root.join("a").join("b.txt")));
        assert_eq!(safe_join(root, "./a//b/../c"), Ok(root.join("a").join("c")));
        assert_eq!(safe_join(root, "../x"), Err(Unsafe::Outside));
        assert_eq!(safe_join(root, "a/../../x"), Err(Unsafe::Outside));
        assert_eq!(safe_join(root, "/etc/passwd"), Err(Unsafe::Outside));
    }

    #[test]
    fn safe_join_refuses_names_that_are_not_portable() {
        assert_eq!(bad("a/C:evil.txt"), "it contains a colon");
        assert_eq!(bad("C:/x"), "it contains a colon");
        assert_eq!(bad("file.txt:stream"), "it contains a colon");
        assert_eq!(bad("a\\..\\..\\x"), "it contains a backslash");
        assert_eq!(bad("x\0y"), "it contains a control character");
        assert_eq!(bad("a\u{1b}[2J"), "it contains a control character");
        assert_eq!(bad("dir./x"), "it ends in a dot or a space");
        assert_eq!(bad("x "), "it ends in a dot or a space");
        for dev in [
            "NUL",
            "con",
            "Aux.txt",
            "com1",
            "LPT9.tar.gz",
            "prn ",
            "conin$",
        ] {
            assert!(member_parts(&format!("a/{dev}")).is_err(), "{dev}");
        }
        for ok in ["console", "com10", "nul_", ".hidden", "a..b", "lpt", "x.y"] {
            assert!(member_parts(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn link_targets_are_normalised_and_kept_inside() {
        let link = |s: &str| s.split('/').map(String::from).collect::<Vec<_>>();
        let t = |l: &str, target: &str| link_target(&link(l), target).map(|(_, w)| w);
        assert_eq!(t("a/b/l", "../c"), Ok("../c".into()));
        assert_eq!(t("e", "d/d/../.."), Ok(".".into()));
        assert_eq!(t("e", "d/d/../../.."), Err(Unsafe::Outside));
        assert_eq!(t("a/e", "d/../.."), Ok("..".into()));
        assert_eq!(t("d", "."), Ok(".".into()));
        assert_eq!(t("a/l", "./x//y/"), Ok("x/y".into()));
        assert_eq!(t("l", "/etc"), Err(Unsafe::Outside));
        assert_eq!(t("l", "../x"), Err(Unsafe::Outside));
        assert!(t("l", "C:/x").is_err());
        assert!(t("l", "").is_err());
        assert_eq!(
            link_target(&link("a/b/l"), "../../x/y").map(|(r, _)| r),
            Ok(vec!["x".to_string(), "y".to_string()])
        );
    }
}
