// The `lexer` submodule mirrors the file name (lexer/lexer.rs) by design so
// that the parser refers to `lexer::Lexer`; clippy flags the name overlap.
#![allow(clippy::module_inception)]

//! Lexical analysis for GoScript.

mod lexer;
mod token;

pub use lexer::Lexer;
pub use token::{is_keyword, lookup_ident, Token, TokenKind};
