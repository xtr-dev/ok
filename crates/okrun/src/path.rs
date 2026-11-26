//! First-class path values (SPEC.md 5.8).
//!
//! A path is stored as a prefix (empty for relative paths, `/`, `C:/`,
//! `//server/share/`) plus normalised components. It always displays with
//! `/` so output is identical on every OS, and converts to the native form
//! only when it is handed to the operating system.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OkPath {
    prefix: String,
    parts: Vec<String>,
}

impl OkPath {
    /// Parse with the rules of this OS: `/` separates components everywhere,
    /// `\` only on Windows, so Unix file names containing `\` stay
    /// representable.
    pub fn parse(s: &str) -> OkPath {
        Self::parse_with(s, cfg!(windows))
    }

    /// Parse with Windows rules (`\` separators, drive and UNC prefixes) on
    /// or off; split out so both rule sets can be unit-tested on any host.
    pub fn parse_with(s: &str, windows: bool) -> OkPath {
        let sep = |c: char| c == '/' || (windows && c == '\\');
        let chars: Vec<char> = s.chars().collect();
        let mut prefix = String::new();
        let mut rest_start = 0;
        if windows && chars.len() >= 2 && chars[0].is_ascii_alphabetic() && chars[1] == ':' {
            prefix.push(chars[0].to_ascii_uppercase());
            prefix.push(':');
            rest_start = 2;
            if chars.len() > 2 && sep(chars[2]) {
                prefix.push('/');
                rest_start = 3;
            }
        } else if windows && chars.len() >= 2 && sep(chars[0]) && sep(chars[1]) {
            // UNC: //server/share/
            let body: String = chars[2..].iter().collect();
            let mut it = body.split(sep).filter(|p| !p.is_empty());
            let server = it.next().unwrap_or("");
            let share = it.next().unwrap_or("");
            prefix = format!("//{server}/{share}/");
            let consumed = 2 + server.chars().count() + 1 + share.chars().count();
            rest_start = consumed.min(chars.len());
        } else if !chars.is_empty() && sep(chars[0]) {
            prefix.push('/');
            rest_start = 1;
        }
        let rest: String = chars[rest_start.min(chars.len())..].iter().collect();
        let mut parts: Vec<String> = Vec::new();
        for comp in rest.split(sep) {
            match comp {
                "" | "." => {}
                ".." => {
                    if parts.last().is_some_and(|p| p != "..") {
                        parts.pop();
                    } else if prefix.is_empty() || !prefix.ends_with('/') {
                        parts.push("..".into());
                    }
                    // `..` directly at a root stays at the root.
                }
                other => parts.push(other.to_string()),
            }
        }
        OkPath { prefix, parts }
    }

    pub fn is_abs(&self) -> bool {
        self.prefix.ends_with('/')
    }

    /// The `/`-separated display form.
    pub fn display(&self) -> String {
        let body = self.parts.join("/");
        if self.prefix.is_empty() && body.is_empty() {
            return ".".into();
        }
        format!("{}{}", self.prefix, body)
    }

    /// The form handed to the operating system.
    pub fn native(&self) -> PathBuf {
        let d = self.display();
        if cfg!(windows) {
            PathBuf::from(d.replace('/', "\\"))
        } else {
            PathBuf::from(d)
        }
    }

    pub fn from_native(p: &std::path::Path) -> OkPath {
        OkPath::parse(&p.to_string_lossy())
    }

    /// `self / other`. An absolute `other` replaces `self` (SPEC.md 5.8).
    pub fn join(&self, other: &OkPath) -> OkPath {
        if other.is_abs() || !other.prefix.is_empty() {
            return other.clone();
        }
        let mut out = self.clone();
        for p in &other.parts {
            if p == ".." {
                if out.parts.last().is_some_and(|l| l != "..") {
                    out.parts.pop();
                } else if !out.is_abs() {
                    out.parts.push("..".into());
                }
            } else {
                out.parts.push(p.clone());
            }
        }
        out
    }

    pub fn join_str(&self, other: &str) -> OkPath {
        self.join(&OkPath::parse(other))
    }

    pub fn parent(&self) -> OkPath {
        let mut out = self.clone();
        match out.parts.last().map(String::as_str) {
            None if out.prefix.is_empty() => out.parts.push("..".into()),
            None => {}
            Some("..") => out.parts.push("..".into()),
            Some(_) => {
                out.parts.pop();
            }
        }
        out
    }

    pub fn name(&self) -> String {
        match self.parts.last() {
            Some(p) if p != ".." => p.clone(),
            _ => String::new(),
        }
    }

    fn split_ext(&self) -> (String, String) {
        let name = self.name();
        match name.rfind('.') {
            Some(i) if i > 0 => (name[..i].to_string(), name[i + 1..].to_string()),
            _ => (name, String::new()),
        }
    }

    pub fn stem(&self) -> String {
        self.split_ext().0
    }

    pub fn ext(&self) -> String {
        self.split_ext().1
    }

    pub fn parts(&self) -> &[String] {
        &self.parts
    }

    /// The parts as `p.parts()` returns them (SPEC.md 12.2): the prefix
    /// (`/`, `C:/`, `C:`, `//server/share/`) first when there is one, so
    /// that joining the parts in order rebuilds the path.
    pub fn parts_with_root(&self) -> Vec<String> {
        let root = (!self.prefix.is_empty()).then(|| self.prefix.clone());
        root.into_iter().chain(self.parts.iter().cloned()).collect()
    }
}

/// `p` made absolute against the working directory, with `.` and `..`
/// resolved by their names alone (symlinks are not followed).
pub fn lexical_absolute(p: &std::path::Path) -> PathBuf {
    let ok = OkPath::from_native(p);
    if ok.is_abs() {
        return ok.native();
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    OkPath::from_native(&cwd).join(&ok).native()
}

/// The absolute form of an existing path, used to compare paths (import
/// keys, project confinement, `remove` protections).
///
/// Native builds use `fs::canonicalize`, which also resolves symlinks. The
/// WebAssembly build resolves the path lexically instead: browser WASI hosts
/// implement no realpath (canonicalize fails with "not supported"), and the
/// host's preopened directories already confine every file the script can
/// reach. Both report a missing path as `NotFound`.
pub fn canonical(p: &std::path::Path) -> std::io::Result<PathBuf> {
    if cfg!(target_os = "wasi") {
        std::fs::metadata(p)?;
        Ok(lexical_absolute(p))
    } else {
        std::fs::canonicalize(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_start_with_the_root_and_rebuild_the_path() {
        let rebuild = |p: &OkPath| {
            p.parts_with_root()
                .iter()
                .fold(OkPath::parse_with(".", false), |acc, x| acc.join(&OkPath::parse_with(x, true)))
        };
        for (s, parts) in [
            ("/a/b", vec!["/", "a", "b"]),
            ("a/b", vec!["a", "b"]),
            ("/", vec!["/"]),
            ("C:/x", vec!["C:/", "x"]),
            ("//srv/share/x", vec!["//srv/share/", "x"]),
        ] {
            let p = OkPath::parse_with(s, true);
            assert_eq!(p.parts_with_root(), parts, "{s}");
            assert_eq!(rebuild(&p), p, "{s}");
        }
    }

    fn unix(s: &str) -> String {
        OkPath::parse_with(s, false).display()
    }

    fn win(s: &str) -> String {
        OkPath::parse_with(s, true).display()
    }

    #[test]
    fn normalisation() {
        assert_eq!(unix("a//b/./c/"), "a/b/c");
        assert_eq!(unix("a/b/../c"), "a/c");
        assert_eq!(unix("../a/../../b"), "../../b");
        assert_eq!(unix("/../x"), "/x");
        assert_eq!(unix(""), ".");
        assert_eq!(unix("./"), ".");
        assert_eq!(unix("/"), "/");
        assert_eq!(unix(r"a\b"), r"a\b");
    }

    #[test]
    fn windows_rules() {
        assert_eq!(win(r"C:\Users\bas\..\x"), "C:/Users/x");
        assert_eq!(win(r"c:/x"), "C:/x");
        assert_eq!(win(r"\\server\share\dir\f.txt"), "//server/share/dir/f.txt");
        assert_eq!(win(r"a\b/c"), "a/b/c");
        assert_eq!(win("D:rel"), "D:rel");
        assert!(OkPath::parse_with(r"C:\x", true).is_abs());
        assert!(!OkPath::parse_with("D:rel", true).is_abs());
    }

    #[test]
    fn parts_of_a_name() {
        let p = OkPath::parse_with("dir/archive.tar.gz", false);
        assert_eq!(p.name(), "archive.tar.gz");
        assert_eq!(p.stem(), "archive.tar");
        assert_eq!(p.ext(), "gz");
        assert_eq!(p.parent().display(), "dir");
        let dot = OkPath::parse_with(".bashrc", false);
        assert_eq!(dot.stem(), ".bashrc");
        assert_eq!(dot.ext(), "");
        assert_eq!(OkPath::parse_with("/", false).parent().display(), "/");
        assert_eq!(OkPath::parse_with("a", false).parent().display(), ".");
        assert_eq!(OkPath::parse_with(".", false).parent().display(), "..");
    }

    #[test]
    fn joining() {
        let base = OkPath::parse_with("/srv/app", false);
        assert_eq!(
            base.join(&OkPath::parse_with("releases/v1", false))
                .display(),
            "/srv/app/releases/v1"
        );
        assert_eq!(
            base.join(&OkPath::parse_with("/etc", false)).display(),
            "/etc"
        );
        assert_eq!(
            base.join(&OkPath::parse_with("../x", false)).display(),
            "/srv/x"
        );
    }
}
