//! Extraction of untrusted archives (SPEC.md 12.9): crafted tar and zip
//! files, built here with the same crates ok uses, are unpacked by the real
//! `ok` binary into a fresh directory, and nothing may appear outside it.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A fresh directory `box` for one test, holding `out` (the destination)
/// and `secret` (a file outside it that must stay untouched).
fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ok-archive-tests-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::write(dir.join("secret"), "secret").unwrap();
    dir
}

/// Run `code` with ok in `dir`; returns (success, stderr).
fn run_ok(dir: &Path, code: &str) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_ok"))
        .args(["-e", code])
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("run ok");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
    )
}

enum M<'a> {
    File(&'a str, &'a str, u32),
    /// Only the Unix-only test of modes uses directory members.
    #[cfg_attr(not(unix), allow(dead_code))]
    Dir(&'a str),
    Sym(&'a str, &'a str),
    Hard(&'a str, &'a str),
}

/// A tar archive of `members`, written with raw names so that unsafe ones
/// can be expressed.
fn tar(dir: &Path, name: &str, members: &[M]) {
    let mut b = tar::Builder::new(Vec::new());
    for m in members {
        let mut h = tar::Header::new_gnu();
        let (path, data, kind, mode, link) = match m {
            M::File(p, d, mode) => (*p, d.as_bytes(), tar::EntryType::Regular, *mode, None),
            M::Dir(p) => (*p, &b""[..], tar::EntryType::Directory, 0o755, None),
            M::Sym(p, t) => (*p, &b""[..], tar::EntryType::Symlink, 0o777, Some(*t)),
            M::Hard(p, t) => (*p, &b""[..], tar::EntryType::Link, 0o644, Some(*t)),
        };
        h.set_entry_type(kind);
        h.set_mode(mode);
        h.set_size(data.len() as u64);
        {
            let gnu = h.as_gnu_mut().unwrap();
            gnu.name[..path.len()].copy_from_slice(path.as_bytes());
            if let Some(t) = link {
                gnu.linkname[..t.len()].copy_from_slice(t.as_bytes());
            }
        }
        h.set_cksum();
        b.append(&h, data).unwrap();
    }
    std::fs::write(dir.join(name), b.into_inner().unwrap()).unwrap();
}

fn zip(dir: &Path, name: &str, members: &[(&str, &str, Option<u32>)]) {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (n, data, mode) in members {
        let mut o = zip::write::SimpleFileOptions::default();
        if let Some(m) = mode {
            o = o.unix_permissions(*m);
        }
        w.start_file(*n, o).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    std::fs::write(dir.join(name), w.finish().unwrap().into_inner()).unwrap();
}

/// Every path under `dir`, relative and with `/`, sorted.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            out.push(
                p.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            if e.file_type().unwrap().is_dir() {
                walk(base, &p, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn assert_secret_kept(dir: &Path) {
    assert_eq!(
        std::fs::read_to_string(dir.join("secret")).unwrap(),
        "secret"
    );
}

#[test]
fn members_outside_or_with_unportable_names_are_refused() {
    let dir = sandbox("names");
    let cases: &[(&str, &str)] = &[
        ("../secret", "would land outside"),
        ("a/../../secret", "would land outside"),
        ("/tmp/ok-abs", "would land outside"),
        (
            "a/C:evil.txt",
            "\"C:evil.txt\" is not a portable file name (it contains a colon)",
        ),
        ("..\\secret", "(it contains a backslash)"),
        ("x/NUL.txt", "(it is a device name on Windows)"),
        ("trail.", "(it ends in a dot or a space)"),
        (
            "esc\u{1b}[2J",
            "\"esc\\u{1b}[2J\" is not a portable file name (it contains a control character)",
        ),
    ];
    for (i, (member, want)) in cases.iter().enumerate() {
        tar(
            &dir,
            &format!("t{i}.tar"),
            &[M::File(member, "pwned", 0o644)],
        );
        zip(&dir, &format!("z{i}.zip"), &[(member, "pwned", None)]);
        for code in [
            format!("untar(\"t{i}.tar\", \"out\")"),
            format!("unzip(\"z{i}.zip\", \"out\")"),
        ] {
            let (ok, err) = run_ok(&dir, &code);
            assert!(!ok, "{code} with {member:?} succeeded");
            assert!(err.contains(want), "{code} with {member:?}: {err}");
            assert!(!err.contains('\u{1b}'), "{err}");
        }
    }
    assert!(listing(&dir.join("out")).is_empty());
    assert_secret_kept(&dir);
}

#[test]
fn links_must_stay_inside_and_are_created_last() {
    let dir = sandbox("links");
    for (i, (link, target)) in [("s", "/etc"), ("a/s", "../../secret"), ("e", "d/../../x")]
        .iter()
        .enumerate()
    {
        tar(&dir, &format!("l{i}.tar"), &[M::Sym(link, target)]);
        let (ok, err) = run_ok(&dir, &format!("untar(\"l{i}.tar\", \"out\")"));
        assert!(!ok && err.contains("it points outside"), "{err}");
    }
    tar(&dir, "h.tar", &[M::Hard("h", "../secret")]);
    let (ok, err) = run_ok(&dir, "untar(\"h.tar\", \"out\")");
    assert!(
        !ok && err.contains("the link \"h\" -> \"../secret\""),
        "{err}"
    );
    // A hard link to a file the archive did not put there is refused.
    tar(&dir, "h2.tar", &[M::Hard("h", "missing")]);
    let (ok, err) = run_ok(&dir, "untar(\"h2.tar\", \"out\")");
    assert!(!ok && err.contains("has no file \"missing\""), "{err}");
    assert!(listing(&dir.join("out")).is_empty());
    assert_secret_kept(&dir);
}

#[cfg(unix)]
#[test]
fn link_chains_cannot_lead_outside() {
    let dir = sandbox("chains");
    // `e -> d/d/../..` reads as `.` by its text, but through `d -> .` the
    // system would resolve it to the parent of `out`.
    tar(
        &dir,
        "chain.tar",
        &[M::Sym("d", "."), M::Sym("e", "d/d/../..")],
    );
    let (ok, err) = run_ok(&dir, "untar(\"chain.tar\", \"out\")");
    assert!(ok, "{err}");
    assert_eq!(
        std::fs::read_link(dir.join("out/e")).unwrap(),
        Path::new(".")
    );
    // Writing through the extracted link stays inside.
    let (ok, err) = run_ok(&dir, "write(\"x\", \"out/e/new.txt\")");
    assert!(ok, "{err}");
    assert!(dir.join("out/new.txt").exists());
    // A member below a link of the same archive is refused, since links are
    // created after every file.
    tar(
        &dir,
        "chain2.tar",
        &[M::Sym("d2", "."), M::File("d2/x", "x", 0o644)],
    );
    let (ok, _) = run_ok(&dir, "untar(\"chain2.tar\", \"out2\")");
    assert!(!ok);
    // Legitimate links work, also through each other.
    tar(
        &dir,
        "pkg.tar",
        &[
            M::File("pkg/lib/libfoo.so.1", "elf", 0o755),
            M::Sym("pkg/lib/libfoo.so", "libfoo.so.1"),
            M::Sym("pkg/bin/foo", "../lib/libfoo.so"),
            M::Sym("pkg/current", "lib/"),
            M::Hard("pkg/copy", "pkg/lib/libfoo.so.1"),
        ],
    );
    let (ok, err) = run_ok(
        &dir,
        "untar(\"pkg.tar\", \"pkg\")\nuntar(\"pkg.tar\", \"pkg\")\nprint(read(\"pkg/pkg/bin/foo\"), read(\"pkg/pkg/current/libfoo.so\"), read(\"pkg/pkg/copy\"))",
    );
    assert!(ok, "{err}");
    assert_eq!(
        std::fs::read_link(dir.join("pkg/pkg/current")).unwrap(),
        Path::new("lib")
    );
    assert_secret_kept(&dir);
}

#[cfg(unix)]
#[test]
fn extraction_never_writes_through_links_in_the_destination() {
    use std::os::unix::fs::symlink;
    let dir = sandbox("through");
    std::fs::create_dir_all(dir.join("outside")).unwrap();
    symlink("../outside", dir.join("out/pre")).unwrap();
    symlink("../secret", dir.join("out/a")).unwrap();
    zip(&dir, "pre.zip", &[("pre/x.txt", "pwned", None)]);
    tar(&dir, "pre.tar", &[M::File("pre/x.txt", "pwned", 0o644)]);
    for code in ["unzip(\"pre.zip\", \"out\")", "untar(\"pre.tar\", \"out\")"] {
        let (ok, err) = run_ok(&dir, code);
        assert!(!ok && err.contains("is a symbolic link"), "{err}");
    }
    assert!(listing(&dir.join("outside")).is_empty());
    // A member at the place of a link replaces the link, not its target.
    zip(&dir, "a.zip", &[("a", "replaced", None)]);
    let (ok, err) = run_ok(&dir, "unzip(\"a.zip\", \"out\")");
    assert!(ok, "{err}");
    assert_eq!(
        std::fs::read_to_string(dir.join("out/a")).unwrap(),
        "replaced"
    );
    assert_secret_kept(&dir);
    // A link of the archive may not pass through a link that was there.
    tar(&dir, "via.tar", &[M::Sym("via", "pre/x")]);
    let (ok, err) = run_ok(&dir, "untar(\"via.tar\", \"out\")");
    assert!(!ok && err.contains("was there before"), "{err}");
}

#[cfg(unix)]
#[test]
fn modes_follow_the_umask_and_keep_only_the_execute_bit() {
    use std::os::unix::fs::PermissionsExt;
    let dir = sandbox("modes");
    tar(
        &dir,
        "m.tar",
        &[
            M::File("suid", "x", 0o6777),
            M::File("plain", "x", 0o666),
            M::Dir("open"),
        ],
    );
    zip(
        &dir,
        "m.zip",
        &[
            ("zsuid", "x", Some(0o106777)),
            ("zplain", "x", Some(0o100666)),
        ],
    );
    let (ok, err) = run_ok(&dir, "untar(\"m.tar\", \"out\")\nunzip(\"m.zip\", \"out\")");
    assert!(ok, "{err}");
    // What the umask leaves of a new file's and directory's permissions,
    // measured on files this test creates the ordinary way.
    std::fs::write(dir.join("probe"), "").unwrap();
    std::fs::create_dir(dir.join("probe-dir")).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    let file = mode(&dir.join("probe"));
    let exec = (file | ((file & 0o444) >> 2)) & mode(&dir.join("probe-dir"));
    let out = |p: &str| mode(&dir.join("out").join(p));
    assert_eq!(out("suid"), exec);
    assert_eq!(out("zsuid"), exec);
    assert_eq!(out("plain"), file);
    assert_eq!(out("zplain"), file);
    assert_eq!(out("open"), mode(&dir.join("probe-dir")));
}
