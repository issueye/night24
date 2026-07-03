//! Type annotation parsing.

use super::*;
use crate::ast::*;
use crate::lexer::TokenKind;

impl Parser {
    /// Parse a type annotation. Expects cur on the first type token.
    pub fn parse_type(&mut self) -> Option<TypeAnnotation> {
        let mut t = self.parse_primary_type()?;
        if self.cur_is(TokenKind::Pipe) {
            let mut union = vec![t];
            while self.cur_is(TokenKind::Pipe) {
                self.next_token();
                if let Some(u) = self.parse_primary_type() {
                    union.push(u);
                }
            }
            t = TypeAnnotation {
                kind: TypeKind::Union,
                name: String::new(),
                array_of: None,
                union,
                optional: false,
            };
        }
        if self.cur_is(TokenKind::Question) {
            t.optional = true;
            self.next_token();
        }
        Some(t)
    }

    fn parse_primary_type(&mut self) -> Option<TypeAnnotation> {
        // Function type: (params) => ret
        if self.cur_is(TokenKind::LParen) {
            return self.parse_function_type();
        }
        // Object/structural type: { k: T, ... }
        if self.cur_is(TokenKind::LBrace) {
            self.skip_object_type();
            return Some(TypeAnnotation {
                kind: TypeKind::Object,
                name: String::new(),
                array_of: None,
                union: Vec::new(),
                optional: false,
            });
        }
        let name = self.cur.literal.clone();
        let t = TypeAnnotation {
            kind: TypeKind::Primitive,
            name,
            array_of: None,
            union: Vec::new(),
            optional: false,
        };
        self.next_token();
        // Array<T> generic form
        if self.cur_is(TokenKind::Lt) {
            // Skip a generic argument list like <number> (best-effort).
            self.skip_generic_args();
        }
        // T[] suffix
        if self.cur_is(TokenKind::LBrack) && self.peek_is(TokenKind::RBrack) {
            let inner = t;
            self.next_token();
            self.next_token();
            return Some(TypeAnnotation {
                kind: TypeKind::Array,
                name: String::new(),
                array_of: Some(Box::new(inner)),
                union: Vec::new(),
                optional: false,
            });
        }
        if self.cur_is(TokenKind::Question) {
            let mut opt = t;
            opt.optional = true;
            self.next_token();
            return Some(opt);
        }
        Some(t)
    }

    /// Parse (and discard) a function type `(a: T, b: U) => V`.
    fn parse_function_type(&mut self) -> Option<TypeAnnotation> {
        // Consume the balanced parameter list.
        let mut depth = 0i32;
        while !self.cur_is(TokenKind::Eof) {
            match self.cur.kind {
                TokenKind::LParen | TokenKind::LBrack | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBrack | TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        self.next_token();
                        break;
                    }
                }
                _ => {}
            }
            self.next_token();
        }
        // Optional `=> ReturnType`.
        if self.cur_is(TokenKind::Arrow) {
            self.next_token();
            let _ = self.parse_type();
        }
        Some(TypeAnnotation {
            kind: TypeKind::Function,
            name: String::new(),
            array_of: None,
            union: Vec::new(),
            optional: false,
        })
    }

    /// Skip over a structural object type `{ k: T, k2?: U, ... }`, balancing braces.
    fn skip_object_type(&mut self) {
        let mut depth = 0i32;
        while !self.cur_is(TokenKind::Eof) {
            match self.cur.kind {
                TokenKind::LBrace => depth += 1,
                TokenKind::RBrace => {
                    depth -= 1;
                    self.next_token();
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            self.next_token();
        }
    }

    /// Skip a generic argument list `<T, U>` (best-effort, balancing angles).
    fn skip_generic_args(&mut self) {
        // We saw `<`. Consume until the matching `>`.
        let mut depth = 0i32;
        while !self.cur_is(TokenKind::Eof) {
            match self.cur.kind {
                TokenKind::Lt => depth += 1,
                TokenKind::Gt => {
                    depth -= 1;
                    self.next_token();
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                TokenKind::RShift => {
                    // Treat >> as two closing angles.
                    depth -= 2;
                    self.next_token();
                    if depth <= 0 {
                        break;
                    }
                    continue;
                }
                _ => {}
            }
            self.next_token();
        }
    }
}
