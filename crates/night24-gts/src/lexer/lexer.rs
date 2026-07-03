//! The lexical analyzer for GoScript source code.
//!
//! Produces a stream of [`Token`]s with position information. The lexer rejects
//! `==` / `!=` (GoScript only allows `===` / `!==`), supports nested block
//! comments, template strings with embedded `${...}` expressions, and regexp
//! literals (disambiguated by the previous significant token, as in JS).

use crate::lexer::token::{lookup_ident, Token, TokenKind};

/// The lexer holds the input source and scanning cursor.
pub struct Lexer {
    input: Vec<char>,
    /// Byte length of the original source (used to bound indexing).
    pos: usize,
    line: usize,
    col: usize,
    errors: Vec<String>,
    prev_token: TokenKind,
}

fn is_letter(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

fn is_digit(ch: char) -> bool {
    ch.is_ascii_digit()
}

fn is_hex_digit(ch: char) -> bool {
    is_digit(ch) || ('a'..='f').contains(&ch) || ('A'..='F').contains(&ch)
}

impl Lexer {
    /// Construct a new lexer over the given source text.
    pub fn new(input: &str) -> Lexer {
        Lexer {
            input: input.chars().collect(),
            pos: 0,
            line: 1,
            col: 0,
            errors: Vec::new(),
            prev_token: TokenKind::Eof,
        }
    }

    /// All errors accumulated during scanning.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    fn add_error(&mut self, msg: impl Into<String>) {
        self.errors.push(format!(
            "Lexer error at line {} col {}: {}",
            self.line,
            self.col,
            msg.into()
        ));
    }

    fn peek(&self) -> char {
        if self.pos < self.input.len() {
            self.input[self.pos]
        } else {
            '\0'
        }
    }

    fn peek_at(&self, offset: usize) -> char {
        if self.pos + offset < self.input.len() {
            self.input[self.pos + offset]
        } else {
            '\0'
        }
    }

    fn advance(&mut self) -> char {
        let ch = self.peek();
        if ch != '\0' {
            self.pos += 1;
            if ch == '\n' {
                self.line += 1;
                self.col = 0;
            } else {
                self.col += 1;
            }
        }
        ch
    }

    /// Produce the next token from the source.
    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace_and_comments();
        let start_line = self.line;
        let start_col = self.col;
        let start_offset = self.pos;
        let ch = self.peek();

        let kind = match ch {
            '(' => {
                self.advance();
                TokenKind::LParen
            }
            ')' => {
                self.advance();
                TokenKind::RParen
            }
            '{' => {
                self.advance();
                TokenKind::LBrace
            }
            '}' => {
                self.advance();
                TokenKind::RBrace
            }
            '[' => {
                self.advance();
                TokenKind::LBrack
            }
            ']' => {
                self.advance();
                TokenKind::RBrack
            }
            ',' => {
                self.advance();
                TokenKind::Comma
            }
            ';' => {
                self.advance();
                TokenKind::Semi
            }
            ':' => {
                self.advance();
                TokenKind::Colon
            }
            '~' => {
                self.advance();
                TokenKind::Tilde
            }
            '^' => {
                self.advance();
                if self.peek() == '=' {
                    self.advance();
                    TokenKind::CaretEq
                } else {
                    TokenKind::Caret
                }
            }
            '?' => match self.peek_at(1) {
                '?' => {
                    self.advance();
                    self.advance();
                    TokenKind::QmQm
                }
                '.' => {
                    self.advance();
                    self.advance();
                    TokenKind::QmDot
                }
                _ => {
                    self.advance();
                    TokenKind::Question
                }
            },
            '+' => match self.peek_at(1) {
                '+' => {
                    self.advance();
                    self.advance();
                    TokenKind::PlusPlus
                }
                '=' => {
                    self.advance();
                    self.advance();
                    TokenKind::PlusEq
                }
                _ => {
                    self.advance();
                    TokenKind::Plus
                }
            },
            '-' => match self.peek_at(1) {
                '-' => {
                    self.advance();
                    self.advance();
                    TokenKind::MinusMinus
                }
                '=' => {
                    self.advance();
                    self.advance();
                    TokenKind::MinusEq
                }
                _ => {
                    self.advance();
                    TokenKind::Minus
                }
            },
            '*' => match self.peek_at(1) {
                '*' => {
                    self.advance();
                    if self.peek() == '=' {
                        self.advance();
                        TokenKind::PowEq
                    } else {
                        self.advance();
                        TokenKind::Pow
                    }
                }
                '=' => {
                    self.advance();
                    self.advance();
                    TokenKind::StarEq
                }
                _ => {
                    self.advance();
                    TokenKind::Star
                }
            },
            '/' => {
                if self.peek_at(1) == '/' {
                    self.skip_line_comment();
                    return self.next_token();
                } else if self.peek_at(1) == '*' {
                    self.skip_block_comment();
                    return self.next_token();
                } else if self.peek_at(1) == '=' {
                    self.advance();
                    self.advance();
                    TokenKind::SlashEq
                } else if self.can_start_regexp() && self.has_regexp_terminator() {
                    return self.read_regexp(start_line, start_col, start_offset);
                } else {
                    self.advance();
                    TokenKind::Slash
                }
            }
            '%' => {
                self.advance();
                if self.peek() == '=' {
                    self.advance();
                    TokenKind::PercentEq
                } else {
                    TokenKind::Percent
                }
            }
            '=' => match self.peek_at(1) {
                '=' => {
                    self.advance();
                    if self.peek_at(1) == '=' {
                        self.advance();
                        self.advance();
                        TokenKind::EqEqEq
                    } else {
                        self.advance();
                        self.add_error(
                            "'==' is not allowed in GoScript; use '===' for strict equality",
                        );
                        TokenKind::Illegal
                    }
                }
                '>' => {
                    self.advance();
                    self.advance();
                    TokenKind::Arrow
                }
                _ => {
                    self.advance();
                    TokenKind::Eq
                }
            },
            '!' => {
                self.advance();
                if self.peek() == '=' {
                    self.advance();
                    if self.peek() == '=' {
                        self.advance();
                        TokenKind::NeqEq
                    } else {
                        self.add_error(
                            "'!=' is not allowed in GoScript; use '!==' for strict inequality",
                        );
                        TokenKind::Illegal
                    }
                } else {
                    TokenKind::Bang
                }
            }
            '<' => match self.peek_at(1) {
                '<' => {
                    self.advance();
                    if self.peek() == '=' {
                        self.advance();
                        TokenKind::LShiftEq
                    } else {
                        self.advance();
                        TokenKind::LShift
                    }
                }
                '=' => {
                    self.advance();
                    self.advance();
                    TokenKind::LtEq
                }
                _ => {
                    self.advance();
                    TokenKind::Lt
                }
            },
            '>' => match self.peek_at(1) {
                '>' => {
                    self.advance();
                    match self.peek_at(1) {
                        '>' => {
                            self.advance();
                            if self.peek_at(1) == '=' {
                                self.advance();
                                self.advance();
                                TokenKind::UrShiftEq
                            } else {
                                self.advance();
                                TokenKind::UrShift
                            }
                        }
                        '=' => {
                            self.advance();
                            self.advance();
                            TokenKind::RShiftEq
                        }
                        _ => {
                            self.advance();
                            TokenKind::RShift
                        }
                    }
                }
                '=' => {
                    self.advance();
                    self.advance();
                    TokenKind::GtEq
                }
                _ => {
                    self.advance();
                    TokenKind::Gt
                }
            },
            '&' => {
                self.advance();
                match self.peek() {
                    '&' => {
                        self.advance();
                        TokenKind::AndAnd
                    }
                    '=' => {
                        self.advance();
                        TokenKind::AmpEq
                    }
                    _ => TokenKind::Amp,
                }
            }
            '|' => {
                self.advance();
                match self.peek() {
                    '|' => {
                        self.advance();
                        TokenKind::OrOr
                    }
                    '=' => {
                        self.advance();
                        TokenKind::PipeEq
                    }
                    _ => TokenKind::Pipe,
                }
            }
            '.' => match self.peek_at(1) {
                '.' => {
                    self.advance(); // consume first '.', pos now on second '.'
                                    // Look at the character after the second dot to decide the kind.
                    if self.peek_at(1) == '=' {
                        self.advance(); // consume second '.'
                        self.advance(); // consume '='
                        TokenKind::DotDotEq
                    } else if self.peek_at(1) == '.' {
                        self.advance(); // consume second '.'
                        self.advance(); // consume third '.'
                        TokenKind::Ellipsis
                    } else {
                        self.advance(); // consume second '.'
                        TokenKind::DotDot
                    }
                }
                _ => {
                    self.advance();
                    TokenKind::Dot
                }
            },
            '"' | '\'' => return self.read_string(ch, start_line, start_col, start_offset),
            '`' => return self.read_template(start_line, start_col, start_offset),
            '\0' => TokenKind::Eof,
            _ => {
                if is_letter(ch) {
                    let ident = self.read_identifier();
                    let kind = lookup_ident(&ident);
                    let tok = Token::new(kind, ident, start_line, start_col, start_offset);
                    self.record_token(tok.kind);
                    return tok;
                } else if is_digit(ch) {
                    let num = self.read_number();
                    let tok =
                        Token::new(TokenKind::Number, num, start_line, start_col, start_offset);
                    self.record_token(tok.kind);
                    return tok;
                } else {
                    self.advance();
                    self.add_error(format!(
                        "unexpected character: {:?} (U+{:04X})",
                        ch, ch as u32
                    ));
                    TokenKind::Illegal
                }
            }
        };

        // Collect the literal span for simple operator/delimiter tokens.
        let literal: String = self.input[start_offset..self.pos].iter().collect();
        let tok = Token::new(kind, literal, start_line, start_col, start_offset);
        self.record_token(tok.kind);
        tok
    }

    fn record_token(&mut self, kind: TokenKind) {
        if kind != TokenKind::Illegal && kind != TokenKind::Eof {
            self.prev_token = kind;
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            let ch = self.peek();
            if ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' {
                self.advance();
                continue;
            }
            if ch == '/' && self.peek_at(1) == '/' {
                self.skip_line_comment();
                continue;
            }
            if ch == '/' && self.peek_at(1) == '*' {
                self.skip_block_comment();
                continue;
            }
            break;
        }
    }

    fn skip_line_comment(&mut self) {
        while self.peek() != '\n' && self.peek() != '\0' {
            self.advance();
        }
    }

    fn skip_block_comment(&mut self) {
        // consume the leading /*
        self.advance();
        self.advance();
        let mut depth = 1;
        while depth > 0 && self.peek() != '\0' {
            if self.peek() == '/' && self.peek_at(1) == '*' {
                self.advance();
                self.advance();
                depth += 1;
                continue;
            }
            if self.peek() == '*' && self.peek_at(1) == '/' {
                self.advance();
                self.advance();
                depth -= 1;
                continue;
            }
            self.advance();
        }
        if depth > 0 {
            self.add_error("unterminated block comment");
        }
    }

    fn read_identifier(&mut self) -> String {
        let mut out = String::new();
        while is_letter(self.peek()) || is_digit(self.peek()) {
            out.push(self.advance());
        }
        out
    }

    fn read_number(&mut self) -> String {
        let mut out = String::new();
        if self.peek() == '0' {
            match self.peek_at(1) {
                'x' | 'X' => {
                    out.push(self.advance());
                    out.push(self.advance());
                    while is_hex_digit(self.peek()) {
                        out.push(self.advance());
                    }
                    return out;
                }
                'b' | 'B' => {
                    out.push(self.advance());
                    out.push(self.advance());
                    while self.peek() == '0' || self.peek() == '1' {
                        out.push(self.advance());
                    }
                    return out;
                }
                'o' | 'O' => {
                    out.push(self.advance());
                    out.push(self.advance());
                    while ('0'..='7').contains(&self.peek()) {
                        out.push(self.advance());
                    }
                    return out;
                }
                _ => {}
            }
        }
        while is_digit(self.peek()) {
            out.push(self.advance());
        }
        if self.peek() == '.' && is_digit(self.peek_at(1)) {
            out.push(self.advance());
            while is_digit(self.peek()) {
                out.push(self.advance());
            }
        }
        if self.peek() == 'e' || self.peek() == 'E' {
            out.push(self.advance());
            if self.peek() == '+' || self.peek() == '-' {
                out.push(self.advance());
            }
            while is_digit(self.peek()) {
                out.push(self.advance());
            }
        }
        out
    }

    fn read_string(&mut self, quote: char, line: usize, col: usize, offset: usize) -> Token {
        let mut lit = String::new();
        lit.push(self.advance()); // opening quote
        while self.peek() != quote && self.peek() != '\0' && self.peek() != '\n' {
            if self.peek() == '\\' {
                lit.push(self.advance()); // backslash
                if self.peek() != '\0' {
                    lit.push(self.advance()); // escaped char
                }
            } else {
                lit.push(self.advance());
            }
        }
        if self.peek() != quote {
            self.add_error("unterminated string literal");
        } else {
            lit.push(self.advance()); // closing quote
        }
        Token::new(TokenKind::String, lit, line, col, offset)
    }

    fn read_template(&mut self, line: usize, col: usize, offset: usize) -> Token {
        let mut lit = String::new();
        lit.push(self.advance()); // opening backtick
        let mut expr_depth = 0usize;
        let mut quote: char = '\0';
        let mut escape = false;
        while self.peek() != '\0' {
            let ch = self.peek();
            if quote != '\0' {
                if escape {
                    escape = false;
                } else if ch == '\\' {
                    escape = true;
                } else if ch == quote {
                    quote = '\0';
                }
                lit.push(self.advance());
                continue;
            }
            if expr_depth == 0 {
                if ch == '`' {
                    lit.push(self.advance());
                    break;
                }
                if ch == '$' && self.peek_at(1) == '{' {
                    lit.push(self.advance());
                    lit.push(self.advance());
                    expr_depth = 1;
                    continue;
                }
                lit.push(self.advance());
                continue;
            }
            match ch {
                '\'' | '"' => quote = ch,
                '{' => expr_depth += 1,
                '}' => {
                    expr_depth -= 1;
                }
                _ => {}
            }
            lit.push(self.advance());
        }
        if expr_depth > 0 || !lit.ends_with('`') {
            self.add_error("unterminated template literal");
        }
        Token::new(TokenKind::Template, lit, line, col, offset)
    }

    fn read_regexp(&mut self, line: usize, col: usize, offset: usize) -> Token {
        let mut lit = String::new();
        lit.push(self.advance()); // opening slash
        let mut in_class = false;
        let mut escape = false;
        while self.peek() != '\0' && self.peek() != '\n' {
            let ch = self.peek();
            if escape {
                escape = false;
                lit.push(self.advance());
                continue;
            }
            if ch == '\\' {
                escape = true;
                lit.push(self.advance());
                continue;
            }
            if in_class {
                if ch == ']' {
                    in_class = false;
                }
                lit.push(self.advance());
                continue;
            }
            if ch == '[' {
                in_class = true;
                lit.push(self.advance());
                continue;
            }
            if ch == '/' {
                lit.push(self.advance());
                while is_letter(self.peek()) {
                    lit.push(self.advance());
                }
                let tok = Token::new(TokenKind::Regexp, lit, line, col, offset);
                self.record_token(tok.kind);
                return tok;
            }
            lit.push(self.advance());
        }
        self.add_error("unterminated regexp literal");
        Token::new(TokenKind::Illegal, lit, line, col, offset)
    }

    fn can_start_regexp(&self) -> bool {
        matches!(
            self.prev_token,
            TokenKind::Eof
                | TokenKind::LParen
                | TokenKind::LBrace
                | TokenKind::LBrack
                | TokenKind::Comma
                | TokenKind::Semi
                | TokenKind::Colon
                | TokenKind::Eq
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Pow
                | TokenKind::Bang
                | TokenKind::Amp
                | TokenKind::Pipe
                | TokenKind::Caret
                | TokenKind::Tilde
                | TokenKind::Question
                | TokenKind::Arrow
                | TokenKind::EqEqEq
                | TokenKind::NeqEq
                | TokenKind::Lt
                | TokenKind::LtEq
                | TokenKind::Gt
                | TokenKind::GtEq
                | TokenKind::AndAnd
                | TokenKind::OrOr
                | TokenKind::QmQm
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
                | TokenKind::Return
                | TokenKind::Throw
                | TokenKind::Delete
                | TokenKind::Typeof
                | TokenKind::Void
                | TokenKind::Await
        )
    }

    fn has_regexp_terminator(&self) -> bool {
        let mut in_class = false;
        let mut escape = false;
        let mut i = self.pos + 1; // skip opening slash
        while i < self.input.len() {
            let r = self.input[i];
            if r == '\0' || r == '\n' {
                return false;
            }
            i += 1;
            if escape {
                escape = false;
                continue;
            }
            if r == '\\' {
                escape = true;
                continue;
            }
            if in_class {
                if r == ']' {
                    in_class = false;
                }
                continue;
            }
            if r == '[' {
                in_class = true;
                continue;
            }
            if r == '/' {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_optional_chain_dot_separately_from_ellipsis() {
        let mut lexer = Lexer::new("obj?.name ...rest");

        assert_eq!(lexer.next_token().kind, TokenKind::Ident);
        assert_eq!(lexer.next_token().kind, TokenKind::QmDot);
        assert_eq!(lexer.next_token().kind, TokenKind::Ident);
        assert_eq!(lexer.next_token().kind, TokenKind::Ellipsis);
        assert_eq!(lexer.next_token().kind, TokenKind::Ident);
    }
}
