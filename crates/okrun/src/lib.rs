//! The `ok` language: lexer, parser, evaluator and standard library.
//!
//! The binary in `main.rs` only calls [`cli::main`]. Everything else lives
//! here so that it can be unit-tested. A program passes through three
//! stages: [`lexer`] turns text into tokens, [`parser`] builds the [`ast`],
//! and [`interp`] evaluates it using [`value`]s and the [`builtins`]. The
//! standard library is spread over [`list`], [`text`], [`formats`],
//! [`files`], [`system`], [`net`] and [`archive`], and `run` lives in
//! [`command`]. Project mode ([`project`], [`jobs`], [`trust`]) and the
//! `ok self` tools ([`tools`], [`fmt`], [`check`]) use the same
//! interpreter. All failures are [`error::OkError`]s, rendered with source
//! snippets from the [`source`] map.

// An `OkError` is large because it carries its context (span, notes, help,
// trace), and errors are rare. Boxing every `Result` to shrink them would
// add noise to the whole interpreter for no measurable gain.
#![allow(clippy::result_large_err)]

pub mod archive;
pub mod ast;
pub mod builtins;
pub mod check;
pub mod cli;
pub mod command;
pub mod error;
pub mod files;
pub mod fmt;
pub mod format;
pub mod formats;
pub mod interp;
pub mod jobs;
pub mod lazy;
pub mod lexer;
pub mod list;
pub mod net;
pub mod parser;
pub mod path;
pub mod procs;
pub mod project;
pub mod sig;
pub mod source;
pub mod system;
pub mod text;
pub mod tools;
pub mod trust;
pub mod value;

/// The version of `ok`, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
