//! Import / export declaration parsing.

use super::*;
use crate::ast::*;
use crate::lexer::TokenKind;
use std::collections::HashMap;

impl Parser {
    /// Parse an `import` declaration. Expects cur on `import`.
    pub fn parse_import(&mut self) -> Option<ImportDecl> {
        let pos = self.pos();
        self.next_token(); // import
        let mut default = String::new();
        let mut namespace = String::new();
        let mut names: Vec<String> = Vec::new();
        let mut aliases: HashMap<String, String> = HashMap::new();

        if self.cur_is(TokenKind::Star) {
            self.next_token();
            if !self.cur_is(TokenKind::As) {
                self.add_error("expected as after * in namespace import");
                return Some(ImportDecl {
                    pos,
                    default,
                    namespace,
                    names,
                    aliases,
                    source: String::new(),
                });
            }
            self.next_token();
            namespace = self.cur.literal.clone();
            self.next_token();
        }
        if self.cur_is(TokenKind::Ident) && !self.peek_is(TokenKind::LBrace) {
            default = self.cur.literal.clone();
            if self.peek_is(TokenKind::Comma) {
                self.next_token();
                self.next_token();
            } else {
                self.next_token();
            }
        }
        if self.cur_is(TokenKind::LBrace) {
            self.next_token();
            while !self.cur_is(TokenKind::RBrace) && !self.cur_is(TokenKind::Eof) {
                let name = self.cur.literal.clone();
                self.next_token();
                if self.cur_is(TokenKind::As) {
                    self.next_token();
                    aliases.insert(name, self.cur.literal.clone());
                    self.next_token();
                } else {
                    names.push(name);
                }
                if self.cur_is(TokenKind::Comma) {
                    self.next_token();
                }
            }
            self.next_token(); // }
        }
        if !self.cur_is(TokenKind::From) {
            self.add_error("expected from in import");
            return Some(ImportDecl {
                pos,
                default,
                namespace,
                names,
                aliases,
                source: String::new(),
            });
        }
        self.next_token(); // from
        let source = self.cur.literal.clone();
        self.next_token(); // string
        self.skip_semicolon();
        Some(ImportDecl {
            pos,
            default,
            namespace,
            names,
            aliases,
            source,
        })
    }

    /// Parse an `export` declaration. Expects cur on `export`.
    pub fn parse_export(&mut self) -> Option<ExportDecl> {
        let pos = self.pos();
        self.next_token(); // export
        let is_default = if self.cur_is(TokenKind::Default) {
            self.next_token();
            true
        } else {
            false
        };
        let mut decl = None;
        let mut specifiers = Vec::new();
        let mut from = String::new();
        let mut is_star = false;
        if !is_default {
            // `export * from "..."` — aggregate re-export of all named exports.
            if self.cur_is(TokenKind::Star) {
                self.next_token(); // consume `*`
                if self.cur_is(TokenKind::As) {
                    // `export * as ns from "..."` — namespace re-export.
                    // (Not implemented at eval; record but fall through to error
                    // at the spec level. For MVP we only support bare `export *`.)
                    self.next_token();
                    let _ns = self.cur.literal.clone();
                    self.next_token();
                }
                is_star = true;
                if self.cur_is(TokenKind::From) {
                    self.next_token(); // from
                    from = self.cur.literal.clone();
                    self.next_token(); // string
                }
                self.skip_semicolon();
            } else if self.cur_is(TokenKind::LBrace) {
                self.next_token();
                while !self.cur_is(TokenKind::RBrace) && !self.cur_is(TokenKind::Eof) {
                    let name = self.cur.literal.clone();
                    let mut alias = name.clone();
                    self.next_token();
                    if self.cur_is(TokenKind::As) {
                        self.next_token();
                        alias = self.cur.literal.clone();
                        self.next_token();
                    }
                    specifiers.push(ExportSpec { name, alias });
                    if self.cur_is(TokenKind::Comma) {
                        self.next_token();
                    }
                }
                self.next_token(); // }
                                   // Optional `from "..."` for re-exporting from another module.
                if self.cur_is(TokenKind::From) {
                    self.next_token(); // from
                    from = self.cur.literal.clone();
                    self.next_token(); // string
                }
                self.skip_semicolon();
            } else {
                decl = self.parse_statement().map(Box::new);
            }
        } else {
            let expr = self.parse_expression(Prec::Comma)?;
            decl = Some(Box::new(Stmt::Expr(ExprStmt {
                pos: self.pos(),
                expr,
            })));
            self.skip_semicolon();
        }
        Some(ExportDecl {
            pos,
            is_default,
            is_star,
            decl,
            specifiers,
            from,
        })
    }
}
