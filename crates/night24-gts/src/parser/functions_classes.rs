//! Function and class declaration parsing.

use super::*;
use crate::ast::*;
use crate::lexer::{is_keyword, TokenKind};

impl Parser {
    pub fn parse_func_decl(&mut self) -> Option<FuncDecl> {
        let pos = self.pos();
        self.expect_peek(TokenKind::Ident);
        let name = self.cur.literal.clone();
        self.next_token();
        let (params, return_t) = self.parse_func_params();
        let body = self.parse_block()?;
        Some(FuncDecl {
            pos,
            name,
            params,
            return_t,
            body,
            is_async: false,
        })
    }

    pub fn parse_async_func_decl(&mut self) -> Option<Stmt> {
        self.next_token(); // skip async
        if self.cur_is(TokenKind::Function) {
            let mut fd = self.parse_func_decl()?;
            fd.is_async = true;
            return Some(Stmt::FuncDecl(fd));
        }
        None
    }

    /// Parse a function parameter list `(...)` plus optional return type. Expects
    /// cur to be on `(`.
    pub fn parse_func_params(&mut self) -> (Vec<Param>, Option<TypeAnnotation>) {
        if !self.cur_is(TokenKind::LParen) {
            self.add_error("expected (");
            return (Vec::new(), None);
        }
        self.next_token(); // (
        let mut params = Vec::new();
        if self.cur_is(TokenKind::RParen) {
            self.next_token(); // )
            let return_t = if self.cur_is(TokenKind::Colon) {
                self.next_token();
                self.parse_type()
            } else {
                None
            };
            return (params, return_t);
        }
        loop {
            let spread = if self.cur_is(TokenKind::Ellipsis) {
                self.next_token();
                true
            } else {
                false
            };
            let param = self.parse_param(spread);
            if self.cur_is(TokenKind::Eq) {
                self.next_token();
                let default = self.parse_expression(Prec::Comma);
                let mut p = param;
                p.default = default;
                params.push(p);
            } else {
                params.push(param);
            }
            if self.cur_is(TokenKind::Comma) {
                self.next_token();
                continue;
            }
            break;
        }
        if !self.cur_is(TokenKind::RParen) {
            self.add_error("expected )");
        }
        self.next_token(); // )
        let return_t = if self.cur_is(TokenKind::Colon) {
            self.next_token();
            self.parse_type()
        } else {
            None
        };
        (params, return_t)
    }

    /// Speculatively parse a parameter list. Returns None (without leaving the
    /// cursor in a useful state) if the tokens do not form a parameter list.
    /// The caller is responsible for rewinding via mark/rewind on failure.
    pub(crate) fn try_parse_param_list(&mut self) -> Option<Vec<Param>> {
        if self.cur_is(TokenKind::RParen) {
            self.next_token();
            return Some(Vec::new());
        }
        let mut params = Vec::new();
        loop {
            let spread = if self.cur_is(TokenKind::Ellipsis) {
                self.next_token();
                true
            } else {
                false
            };
            if !self.cur_is(TokenKind::Ident) {
                return None; // not a parameter list
            }
            let param = self.parse_param(spread);
            if self.cur_is(TokenKind::Eq) {
                self.next_token();
                let default = self.parse_expression(Prec::Comma);
                let mut p = param;
                p.default = default;
                params.push(p);
            } else {
                params.push(param);
            }
            if self.cur_is(TokenKind::Comma) {
                self.next_token();
                continue;
            }
            if self.cur_is(TokenKind::RParen) {
                self.next_token();
                return Some(params);
            }
            return None;
        }
    }

    fn parse_param(&mut self, spread: bool) -> Param {
        let pos = self.pos();
        let name = self.cur.literal.clone();
        self.next_token();
        let mut optional = false;
        if self.cur_is(TokenKind::Question) {
            optional = true;
            self.next_token();
        }
        let type_anno = if self.cur_is(TokenKind::Colon) {
            self.next_token();
            self.parse_type()
        } else if optional {
            Some(TypeAnnotation {
                kind: TypeKind::Primitive,
                name: "any".into(),
                array_of: None,
                union: Vec::new(),
                optional: true,
            })
        } else {
            None
        };
        let mut anno = type_anno;
        if optional {
            if let Some(a) = &mut anno {
                a.optional = true;
            }
        }
        Param {
            pos,
            name,
            type_anno: anno,
            default: None,
            spread,
            optional,
        }
    }

    // ========================================================================
    // Classes
    // ========================================================================

    pub fn parse_class_decl(&mut self) -> Option<ClassDecl> {
        let pos = self.pos();
        if !self.peek_is(TokenKind::Ident) {
            self.add_error("expected class name");
            return None;
        }
        self.next_token(); // class
        let name = self.cur.literal.clone();
        self.next_token(); // name

        let super_ = if self.cur_is(TokenKind::Extends) {
            self.next_token(); // extends
            self.parse_expression(Prec::Comma)
        } else {
            None
        };
        let body = self.parse_class_body()?;
        Some(ClassDecl {
            pos,
            name,
            super_,
            body,
        })
    }

    fn parse_class_body(&mut self) -> Option<ClassBody> {
        if !self.cur_is(TokenKind::LBrace) {
            self.add_error("expected {");
            return None;
        }
        let pos = self.pos();
        self.next_token(); // {
        let mut members = Vec::new();
        while !self.cur_is(TokenKind::RBrace) && !self.cur_is(TokenKind::Eof) {
            if let Some(m) = self.parse_class_member() {
                members.push(m);
            }
        }
        if self.cur_is(TokenKind::RBrace) {
            self.next_token(); // }
        }
        Some(ClassBody { pos, members })
    }

    fn parse_class_member(&mut self) -> Option<ClassMember> {
        if self.cur_is(TokenKind::Semi) || self.cur_is(TokenKind::RBrace) {
            return None;
        }
        let pos = self.pos();
        let mut is_static = false;
        let mut is_async = false;
        if self.cur_is(TokenKind::Static) {
            is_static = true;
            self.next_token();
        }
        if self.cur_is(TokenKind::Async) {
            is_async = true;
            self.next_token();
        }
        // Allow keywords as member names (e.g. `default`, `if`) like the Go impl
        // allows reserved words via the literal. We accept any token's literal.
        let name = self.cur.literal.clone();
        self.next_token(); // skip name

        let mut params = Vec::new();
        let mut body = None;
        let mut type_anno = None;
        let mut default_val = None;
        let mut kind = ClassMemberKind::Field;

        if self.cur_is(TokenKind::LParen) {
            // method or constructor
            kind = if name == "constructor" {
                ClassMemberKind::Constructor
            } else {
                ClassMemberKind::Method
            };
            let (p, _) = self.parse_func_params();
            params = p;
            body = self.parse_block();
        } else if self.cur_is(TokenKind::Colon) {
            // field with type
            self.next_token();
            type_anno = self.parse_type();
            if self.cur_is(TokenKind::Eq) {
                self.next_token();
                default_val = self.parse_expression(Prec::Comma);
            }
            self.skip_semicolon();
        } else if self.cur_is(TokenKind::Eq) {
            // field with default value
            self.next_token();
            default_val = self.parse_expression(Prec::Comma);
            self.skip_semicolon();
        } else {
            // field without type or value
            self.skip_semicolon();
        }

        Some(ClassMember {
            pos,
            is_static,
            is_async,
            name,
            params,
            body,
            type_anno,
            default_val,
            kind,
        })
    }
}

// Keep the keyword import referenced for future reserved-word-as-name handling.
#[allow(dead_code)]
fn _kw(k: crate::lexer::TokenKind) -> bool {
    is_keyword(k)
}
