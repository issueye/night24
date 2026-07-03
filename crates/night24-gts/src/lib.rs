//! GoScript — a JavaScript-style dynamic scripting language interpreter
//! implemented in Rust.
//!
//! This is a Rust port of the Go-based GoScript interpreter. It implements the
//! full front-end pipeline (lexer → parser → AST), a tree-walking evaluator, a
//! runtime object system, built-in functions, an async model (Promise /
//! async/await / timers), a module system, and a CLI.

pub mod apidoc;
pub mod ast;
pub mod async_runtime;
pub mod bundler;
pub mod bytecode;
pub mod evaluator;
pub mod gtp;
pub mod lexer;
pub mod lsp;
pub mod module;
pub mod object;
pub mod packagefile;
pub mod parser;
pub mod runtime;
pub mod stdlib;
pub mod typecheck;

/// The interpreter version string.
pub const VERSION: &str = "0.2.0";
