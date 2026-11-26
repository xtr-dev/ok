//! The `ok` language: lexer, parser, evaluator and standard library.
//!
//! The binary in `main.rs` only calls [`cli::main`]; everything else lives
//! here so it can be unit-tested. Pipeline: [`lexer`] turns text into
//! tokens, [`parser`] builds the [`ast`], [`interp`] evaluates it using
//! [`value`]s and the [`builtins`]. All failures are [`error::OkError`]s,
//! rendered with source snippets from the [`source`] map.

// Errors travel on the cold path and carry rich context (span, notes, help,
// trace); boxing every `Result` to shrink them would add noise to the whole
// interpreter for no measurable gain.
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
