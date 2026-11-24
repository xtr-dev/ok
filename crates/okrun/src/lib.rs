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

pub mod ast;
pub mod builtins;
pub mod cli;
pub mod error;
pub mod format;
pub mod interp;
pub mod lexer;
pub mod parser;
pub mod path;
pub mod source;
pub mod value;

/// The version of `ok`, taken from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
