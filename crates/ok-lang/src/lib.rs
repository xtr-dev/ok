//! The `ok` language: lexer, parser, evaluator and standard library.
//!
//! Milestone M0 contains only the specification (`SPEC.md` at the
//! repository root). The interpreter arrives in M1; until then this crate
//! exposes the version string so the binary and CI have something real to
//! build and test.

/// The version of `ok`, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Text printed by the placeholder binary for anything other than a version
/// request. Removed when the M1 interpreter lands.
pub const NOT_IMPLEMENTED: &str =
    "ok: the interpreter is not implemented yet (milestone M0 is the specification; see SPEC.md)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver_like() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "version {VERSION} should have three parts");
        assert!(parts.iter().all(|p| p.parse::<u64>().is_ok()));
    }
}
