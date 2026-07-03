//! The parser: a Pratt-style expression parser combined with recursive
//! descent for statements.
//!
//! Mirrors the structure of the original Go implementation: a token stream
//! with `cur`/`peek` lookahead, prefix/infix parse function tables keyed by
//! token kind, precedence levels, and a statement dispatcher.
//!
//! To support speculative parsing (arrow-function parameter lists vs.
//! parenthesized expressions), the parser keeps a replay buffer: tokens read
//! during a speculative parse are captured and can be re-injected.

// Re-export the building blocks so submodules can pull them in with `use super::*`.
pub use crate::ast::*;
pub use crate::lexer::{Lexer, Token, TokenKind};

/// Precedence levels, from lowest to highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Prec {
    Lowest = 0,
    Comma = 1,
    Assign = 2,
    Ternary = 3,
    OrOr = 4,
    AndAnd = 5,
    BitOr = 6,
    BitXor = 7,
    BitAnd = 8,
    Equals = 9,
    Compare = 10,
    Shift = 11,
    Sum = 12,
    Product = 13,
    Exponent = 14,
    Prefix = 15,
    Postfix = 16,
    Call = 17,
}

const MAX_PARSE_ERRORS: usize = 100;

/// A captured parser state used to rewind a speculative parse.
pub(crate) struct Mark {
    pub(crate) cur: Token,
    pub(crate) peek: Token,
    pub(crate) buf: Vec<Token>,
    pub(crate) captured: Vec<Token>,
}

/// The recursive-descent / Pratt parser.
pub struct Parser {
    pub(crate) lexer: Lexer,
    pub(crate) cur: Token,
    pub(crate) peek: Token,
    pub(crate) buf: Vec<Token>,
    pub(crate) marks: Vec<Mark>,
    pub(crate) file: String,
    pub(crate) errors: Vec<String>,
}

impl Parser {
    /// Construct a new parser over the given lexer.
    pub fn new(mut lexer: Lexer, file: impl Into<String>) -> Parser {
        let cur = lexer.next_token();
        let peek = lexer.next_token();
        Parser {
            lexer,
            cur,
            peek,
            buf: Vec::new(),
            marks: Vec::new(),
            file: file.into(),
            errors: Vec::new(),
        }
    }

    /// All accumulated parse errors.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub(crate) fn add_error(&mut self, msg: impl Into<String>) {
        if self.errors.len() >= MAX_PARSE_ERRORS {
            if self.errors.len() == MAX_PARSE_ERRORS {
                self.errors.push(format!(
                    "{}: too many parse errors (limit: {})",
                    self.pos(),
                    MAX_PARSE_ERRORS
                ));
            }
            return;
        }
        self.errors.push(format!("{}: {}", self.pos(), msg.into()));
    }

    pub(crate) fn pos(&self) -> Position {
        Position::new(
            self.file.clone(),
            self.cur.line,
            self.cur.column,
            self.cur.offset,
        )
    }

    pub(crate) fn read_token(&mut self) -> Token {
        if let Some(tok) = self.buf.first().cloned() {
            self.buf.remove(0);
            // Tokens read from the buffer are not re-captured (they were already
            // captured when first produced from the lexer).
            return tok;
        }
        let tok = self.lexer.next_token();
        for mark in self.marks.iter_mut() {
            mark.captured.push(tok.clone());
        }
        tok
    }

    pub(crate) fn next_token(&mut self) {
        let next = self.read_token();
        self.cur = std::mem::replace(&mut self.peek, next);
    }

    pub(crate) fn cur_is(&self, kind: TokenKind) -> bool {
        self.cur.kind == kind
    }

    pub(crate) fn peek_is(&self, kind: TokenKind) -> bool {
        self.peek.kind == kind
    }

    pub(crate) fn cur_precedence(&self) -> Prec {
        precedence_of(self.cur.kind)
    }

    /// Precedence of the lookahead token. Reserved for future Pratt-style
    /// expression parsing; the current recursive-descent path uses
    /// `cur_precedence` instead.
    #[allow(dead_code)]
    pub(crate) fn peek_precedence(&self) -> Prec {
        precedence_of(self.peek.kind)
    }

    pub(crate) fn expect_peek(&mut self, kind: TokenKind) -> bool {
        if self.peek_is(kind) {
            self.next_token();
            true
        } else {
            self.add_error(format!(
                "expected {:?}, got {:?} ({:?})",
                kind, self.peek.kind, self.peek.literal
            ));
            false
        }
    }

    pub(crate) fn skip_semicolon(&mut self) {
        if self.cur_is(TokenKind::Semi) {
            self.next_token();
        }
    }

    // ========================================================================
    // Speculative parsing (mark / rewind / commit)
    // ========================================================================

    pub(crate) fn mark(&mut self) -> usize {
        let m = Mark {
            cur: self.cur.clone(),
            peek: self.peek.clone(),
            buf: self.buf.clone(),
            captured: Vec::new(),
        };
        self.marks.push(m);
        self.marks.len() - 1
    }

    pub(crate) fn rewind(&mut self, idx: usize) {
        let m = self.marks.remove(idx);
        // Drop any later marks (they are no longer valid).
        self.marks.truncate(idx);
        self.cur = m.cur;
        self.peek = m.peek;
        // Re-inject captured tokens ahead of the saved buffer.
        let mut combined = m.captured;
        combined.extend(m.buf);
        self.buf = combined;
    }

    pub(crate) fn commit(&mut self, idx: usize) {
        self.marks.remove(idx);
    }

    // ========================================================================
    // Program & Top-Level
    // ========================================================================

    /// Parse a full program.
    pub fn parse_program(&mut self) -> Program {
        let pos = self.pos();
        let mut body = Vec::new();
        while !self.cur_is(TokenKind::Eof) {
            if let Some(stmt) = self.parse_statement() {
                body.push(stmt);
            }
        }
        let errors = self.errors.to_vec();
        Program { pos, body, errors }
    }

    pub(crate) fn sync(&mut self) {
        while !self.cur_is(TokenKind::Eof) {
            if self.cur_is(TokenKind::Semi) {
                self.next_token();
                return;
            }
            if self.cur_is(TokenKind::RBrace) || self.cur_is(TokenKind::LBrace) {
                return;
            }
            match self.cur.kind {
                TokenKind::Let
                | TokenKind::Const
                | TokenKind::Var
                | TokenKind::Function
                | TokenKind::Class
                | TokenKind::If
                | TokenKind::While
                | TokenKind::For
                | TokenKind::Return
                | TokenKind::Break
                | TokenKind::Continue
                | TokenKind::Try
                | TokenKind::Throw
                | TokenKind::Import
                | TokenKind::Export
                | TokenKind::Match => return,
                _ => self.next_token(),
            }
        }
    }
}

/// Look up the precedence for an infix token.
pub fn precedence_of(kind: TokenKind) -> Prec {
    match kind {
        TokenKind::Comma => Prec::Comma,
        TokenKind::Eq
        | TokenKind::PlusEq
        | TokenKind::MinusEq
        | TokenKind::StarEq
        | TokenKind::SlashEq
        | TokenKind::PercentEq
        | TokenKind::PowEq
        | TokenKind::LShiftEq
        | TokenKind::RShiftEq
        | TokenKind::UrShiftEq
        | TokenKind::AmpEq
        | TokenKind::PipeEq
        | TokenKind::CaretEq
        | TokenKind::Arrow => Prec::Assign,
        TokenKind::Question => Prec::Ternary,
        TokenKind::OrOr | TokenKind::QmQm => Prec::OrOr,
        TokenKind::AndAnd => Prec::AndAnd,
        TokenKind::Pipe => Prec::BitOr,
        TokenKind::Caret => Prec::BitXor,
        TokenKind::Amp => Prec::BitAnd,
        TokenKind::EqEqEq | TokenKind::NeqEq => Prec::Equals,
        TokenKind::Lt
        | TokenKind::LtEq
        | TokenKind::Gt
        | TokenKind::GtEq
        | TokenKind::In
        | TokenKind::Instanceof => Prec::Compare,
        TokenKind::LShift | TokenKind::RShift | TokenKind::UrShift => Prec::Shift,
        TokenKind::Plus | TokenKind::Minus => Prec::Sum,
        TokenKind::Star | TokenKind::Slash | TokenKind::Percent => Prec::Product,
        TokenKind::Pow => Prec::Exponent,
        TokenKind::LParen | TokenKind::Dot | TokenKind::LBrack | TokenKind::QmDot => Prec::Call,
        TokenKind::PlusPlus | TokenKind::MinusMinus => Prec::Postfix,
        _ => Prec::Lowest,
    }
}
