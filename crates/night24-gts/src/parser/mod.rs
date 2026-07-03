//! The parser: converts a token stream into an AST.
// The `parser` submodule mirrors the file name (parser/parser.rs) by design
// so callers refer to `parser::Parser`; clippy flags the name overlap.
#![allow(clippy::module_inception)]

mod expressions;
mod functions_classes;
mod modules;
mod parser;
mod patterns;
mod statements;
mod types;

pub use parser::{precedence_of, Parser, Prec};

// Re-export the number-literal helper used by patterns/expressions.
pub use expressions::parse_number_literal_value;
