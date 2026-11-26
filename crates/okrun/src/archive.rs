//! Archives: `zip`, `unzip`, `tar`, `untar` (SPEC.md 12.9).
//!
//! Members are stored with `/` paths relative to each source's parent, so
//! archiving the directory `dist` gives members `dist/...`. Extraction
//! refuses members that are absolute or escape the destination ("zip slip"),
//! and links that point outside it.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::builtins::{eager, type_err};
use crate::error::{ErrKind, OkError};
use crate::files::{ensure_parent, path_at};
use crate::interp::{Args, Builtin, Interp, R, io_reason};
use crate::list::{count_args, named};
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
            let child = format!("{name}/{}", e.file_name().to_string_lossy());
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

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode & 0o7777));
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) {}

fn zip(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "zip", &[])?;
    count_args(&args, "zip", 2, 2)?;
    let srcs = sources(&args, "zip")?;
    let dst = path_at(&args, 1, "zip")?;
    if !interp.mutation(&format!(
        "zip {} {}",
        srcs.iter()
            .map(OkPath::display)
            .collect::<Vec<_>>()
            .join(" "),
        dst.display()
    )) {
        return Ok(Value::Null);
    }
    let native = dst.native();
    ensure_parent(&native).map_err(|e| {
        fail(
            &args,
            format!("cannot write {}: {}", dst.display(), io_reason(&e)),
        )
    })?;
    let file = File::create(&native).map_err(|e| {
        fail(
            &args,
            format!("cannot write {}: {}", dst.display(), io_reason(&e)),
        )
    })?;
    let mut w = zip::ZipWriter::new(file);
    for src in &srcs {
        let list = members(src).map_err(|e| {
            fail(
                &args,
                format!("cannot read {}: {}", src.display(), io_reason(&e)),
            )
        })?;
        for (path, name, is_dir) in list {
            let mut opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            if let Some(m) = mode_of(&path) {
                opts = opts.unix_permissions(m);
            }
            let zerr = |e: zip::result::ZipError| {
                fail(&args, format!("cannot write {}: {e}", dst.display()))
            };
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
    }
    w.finish()
        .map_err(|e| fail(&args, format!("cannot write {}: {e}", dst.display())))?;
    Ok(Value::Null)
}

/// A member path that stays inside the destination, or `None`.
fn safe_join(dst: &Path, member: &str) -> Option<PathBuf> {
    let rel = Path::new(member);
    let mut out = dst.to_path_buf();
    let mut depth = 0i32;
    for c in rel.components() {
        match c {
            Component::Normal(p) => {
                out.push(p);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
                out.pop();
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

fn unzip(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "unzip", &[])?;
    count_args(&args, "unzip", 2, 2)?;
    let archive = path_at(&args, 0, "unzip")?;
    let dst = path_at(&args, 1, "unzip")?;
    if !interp.mutation(&format!("unzip {} {}", archive.display(), dst.display())) {
        return Ok(Value::Null);
    }
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
    let root = dst.native();
    for i in 0..z.len() {
        let mut entry = z
            .by_index(i)
            .map_err(|e| fail(&args, format!("cannot read {}: {e}", archive.display())))?;
        let name = entry
            .name()
            .map(|n| n.into_owned())
            .map_err(|e| fail(&args, format!("cannot read {}: {e}", archive.display())))?;
        let Some(out) = safe_join(&root, &name) else {
            return Err(fail(
                &args,
                format!(
                    "refusing to extract {name:?} from {}: it would land outside {}",
                    archive.display(),
                    dst.display()
                ),
            ));
        };
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| {
                fail(
                    &args,
                    format!(
                        "cannot create {}: {}",
                        OkPath::from_native(&out).display(),
                        io_reason(&e)
                    ),
                )
            })?;
            continue;
        }
        if entry.is_symlink() {
            return Err(fail(
                &args,
                format!(
                    "refusing to extract the symbolic link {name:?} from {}",
                    archive.display()
                ),
            ));
        }
        ensure_parent(&out).map_err(|e| {
            fail(
                &args,
                format!(
                    "cannot create {}: {}",
                    OkPath::from_native(&out).display(),
                    io_reason(&e)
                ),
            )
        })?;
        let mut f = File::create(&out).map_err(|e| {
            fail(
                &args,
                format!(
                    "cannot write {}: {}",
                    OkPath::from_native(&out).display(),
                    io_reason(&e)
                ),
            )
        })?;
        std::io::copy(&mut entry, &mut f)
            .map_err(|e| fail(&args, format!("cannot extract {name}: {e}")))?;
        if let Some(mode) = entry.unix_mode() {
            set_mode(&out, mode);
        }
    }
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
            .map(OkPath::display)
            .collect::<Vec<_>>()
            .join(" "),
        dst.display()
    )) {
        return Ok(Value::Null);
    }
    let native = dst.native();
    ensure_parent(&native).map_err(|e| {
        fail(
            &args,
            format!("cannot write {}: {}", dst.display(), io_reason(&e)),
        )
    })?;
    let file = File::create(&native).map_err(|e| {
        fail(
            &args,
            format!("cannot write {}: {}", dst.display(), io_reason(&e)),
        )
    })?;
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
    for src in &srcs {
        let list = members(src).map_err(|e| {
            fail(
                &args,
                format!("cannot read {}: {}", src.display(), io_reason(&e)),
            )
        })?;
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
    }
    let inner = b.into_inner().map_err(|e| {
        fail(
            &args,
            format!("cannot write {}: {}", dst.display(), io_reason(&e)),
        )
    })?;
    drop(inner);
    Ok(Value::Null)
}

/// Extract one checked entry to `out`, its place inside `root`. Returns
/// false when the tar crate finds it would land outside `root`.
#[cfg(not(target_os = "wasi"))]
fn unpack_entry<R: Read>(
    entry: &mut tar::Entry<R>,
    root: &Path,
    _out: &Path,
) -> std::io::Result<bool> {
    entry.unpack_in(root)
}

/// Extract one checked entry to `out`, its place inside `root`.
///
/// The tar crate's unpacking canonicalizes paths and sets file times and
/// permissions, none of which browser WASI hosts support (on WASI setting
/// permissions always fails), so the WebAssembly build writes directories
/// and regular files itself, to the path `safe_join` already checked; the
/// host's preopened directories confine them. Links cannot be created there
/// and are refused; other special entries are skipped.
#[cfg(target_os = "wasi")]
fn unpack_entry<R: Read>(
    entry: &mut tar::Entry<R>,
    _root: &Path,
    out: &Path,
) -> std::io::Result<bool> {
    use tar::EntryType;
    match entry.header().entry_type() {
        EntryType::Directory => std::fs::create_dir_all(out)?,
        EntryType::Regular | EntryType::Continuous => {
            ensure_parent(out)?;
            let mut f = File::create(out)?;
            std::io::copy(entry, &mut f)?;
        }
        EntryType::Symlink | EntryType::Link => {
            return Err(std::io::Error::other(
                "the WebAssembly build of ok cannot create links",
            ));
        }
        _ => {}
    }
    Ok(true)
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

fn untar(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "untar", &[])?;
    count_args(&args, "untar", 2, 2)?;
    let archive = path_at(&args, 0, "untar")?;
    let dst = path_at(&args, 1, "untar")?;
    if !interp.mutation(&format!("untar {} {}", archive.display(), dst.display())) {
        return Ok(Value::Null);
    }
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
    a.set_preserve_permissions(cfg!(unix));
    let root = dst.native();
    std::fs::create_dir_all(&root).map_err(|e| {
        fail(
            &args,
            format!("cannot create {}: {}", dst.display(), io_reason(&e)),
        )
    })?;
    let entries = a.entries().map_err(|e| {
        fail(
            &args,
            format!("{} is not a valid tar archive: {e}", archive.display()),
        )
    })?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| fail(&args, format!("cannot read {}: {e}", archive.display())))?;
        let name = entry
            .path()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let Some(out) = safe_join(&root, &name) else {
            return Err(fail(
                &args,
                format!(
                    "refusing to extract {name:?} from {}: it would land outside {}",
                    archive.display(),
                    dst.display()
                ),
            ));
        };
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            let target = entry
                .link_name()
                .ok()
                .flatten()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let base = Path::new(&name)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let joined = if base.is_empty() {
                target.clone()
            } else {
                format!("{base}/{target}")
            };
            if target.starts_with('/') || safe_join(&root, &joined).is_none() {
                return Err(fail(
                    &args,
                    format!(
                        "refusing to extract the link {name:?} -> {target:?} from {}: it points outside {}",
                        archive.display(),
                        dst.display()
                    ),
                ));
            }
        }
        let ok = unpack_entry(&mut entry, &root, &out)
            .map_err(|e| fail(&args, format!("cannot extract {name}: {e}")))?;
        if !ok {
            return Err(fail(
                &args,
                format!(
                    "refusing to extract {name:?} from {}: it would land outside {}",
                    archive.display(),
                    dst.display()
                ),
            ));
        }
    }
    Ok(Value::Null)
}
