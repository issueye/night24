//! Statement parsing (recursive descent).

use super::*;
use crate::ast::*;
use crate::lexer::TokenKind;

impl Parser {
    /// Dispatch on the current token to parse a single statement.
    pub fn parse_statement(&mut self) -> Option<Stmt> {
        match self.cur.kind {
            TokenKind::Let => Some(self.parse_var_decl(DeclKind::Let)),
            TokenKind::Const => Some(self.parse_var_decl(DeclKind::Const)),
            TokenKind::Var => Some(self.parse_var_decl(DeclKind::Var)),
            TokenKind::If => self.parse_if().map(Stmt::If),
            TokenKind::While => self.parse_while().map(Stmt::While),
            TokenKind::For => self.parse_for(),
            TokenKind::Return => Some(Stmt::Return(self.parse_return())),
            TokenKind::Break => Some(Stmt::Break(self.parse_break())),
            TokenKind::Continue => Some(Stmt::Continue(self.parse_continue())),
            TokenKind::Throw => Some(Stmt::Throw(self.parse_throw())),
            TokenKind::Try => self.parse_try().map(|t| Stmt::Try(Box::new(t))),
            TokenKind::LBrace => self.parse_block().map(Stmt::Block),
            TokenKind::Function => self.parse_func_decl().map(Stmt::FuncDecl),
            TokenKind::Class => self.parse_class_decl().map(Stmt::ClassDecl),
            TokenKind::Async => self.parse_async_func_decl(),
            // Dynamic `import(specifier)` as a statement expression: fall
            // through to expression-statement parsing (so `.then(...)` chains
            // work). The expression parser produces a DynamicImport node.
            TokenKind::Import if self.peek_is(TokenKind::LParen) => {
                let pos = self.pos();
                if let Some(expr) = self.parse_expression(Prec::Comma) {
                    self.skip_semicolon();
                    Some(Stmt::Expr(ExprStmt { pos, expr }))
                } else {
                    None
                }
            }
            TokenKind::Import => self.parse_import().map(Stmt::Import),
            TokenKind::Export => self.parse_export().map(Stmt::Export),
            TokenKind::Match => {
                let me = self.parse_match_expr();
                self.skip_semicolon();
                Some(Stmt::Expr(ExprStmt {
                    pos: self.pos(),
                    expr: me,
                }))
            }
            TokenKind::RBrace => {
                self.add_error("unexpected }");
                self.next_token();
                None
            }
            TokenKind::Ident => {
                if self.peek_is(TokenKind::Colon) {
                    let name = self.cur.literal.clone();
                    self.next_token(); // ident
                    self.next_token(); // colon
                    let stmt = self.parse_statement()?;
                    return Some(Stmt::Labeled(Box::new(LabeledStmt {
                        pos: self.pos(),
                        label: name,
                        stmt: Box::new(stmt),
                    })));
                }
                self.parse_expr_statement()
            }
            _ => self.parse_expr_statement(),
        }
    }

    fn parse_expr_statement(&mut self) -> Option<Stmt> {
        let pos = self.pos();
        match self.parse_expression(Prec::Comma) {
            Some(expr) => {
                self.skip_semicolon();
                Some(Stmt::Expr(ExprStmt { pos, expr }))
            }
            None => {
                // Recovery: skip to a statement boundary so a malformed fragment
                // does not trap the parser in an infinite loop.
                self.sync();
                None
            }
        }
    }

    // ========================================================================
    // Variable declarations
    // ========================================================================

    fn parse_var_decl(&mut self, kind: DeclKind) -> Stmt {
        let pos = self.pos();
        self.next_token(); // skip let/const/var

        // Destructuring binding: let [a,b] = … / let {x, y: z = d} = … (B3.2).
        if self.cur_is(TokenKind::LBrack) || self.cur_is(TokenKind::LBrace) {
            let binding = self.parse_binding_pattern();
            // A destructuring declaration must have an initializer.
            let value = if self.cur_is(TokenKind::Eq) {
                self.next_token();
                self.parse_expression(Prec::Comma)
            } else {
                None
            };
            self.skip_semicolon();
            return match kind {
                DeclKind::Let => Stmt::Let(LetStmt {
                    pos,
                    name: String::new(),
                    binding: Some(binding),
                    type_anno: None,
                    value,
                }),
                DeclKind::Const => Stmt::Const(ConstStmt {
                    pos,
                    name: String::new(),
                    binding: Some(binding),
                    type_anno: None,
                    value,
                }),
                DeclKind::Var => Stmt::Var(VarStmt {
                    pos,
                    name: String::new(),
                    binding: Some(binding),
                    type_anno: None,
                    value,
                }),
            };
        }

        let name = self.cur.literal.clone();
        self.next_token(); // advance past identifier

        let type_anno = if self.cur_is(TokenKind::Colon) {
            self.next_token();
            self.parse_type()
        } else {
            None
        };

        let value = if self.cur_is(TokenKind::Eq) {
            self.next_token();
            self.parse_expression(Prec::Comma)
        } else {
            None
        };

        self.skip_semicolon();

        match kind {
            DeclKind::Let => Stmt::Let(LetStmt {
                pos,
                name,
                binding: None,
                type_anno,
                value,
            }),
            DeclKind::Const => Stmt::Const(ConstStmt {
                pos,
                name,
                binding: None,
                type_anno,
                value,
            }),
            DeclKind::Var => Stmt::Var(VarStmt {
                pos,
                name,
                binding: None,
                type_anno,
                value,
            }),
        }
    }

    /// Parse a destructuring binding pattern (B3.2): `[a, , b = d, ...rest]`
    /// or `{x, y: z = d}`. The current token is the opening `[` or `{`.
    fn parse_binding_pattern(&mut self) -> BindingPattern {
        if self.cur_is(TokenKind::LBrack) {
            self.next_token(); // consume `[`
            let mut elems: Vec<ArrayBindingElem> = Vec::new();
            while !self.cur_is(TokenKind::RBrack) {
                // Rest element `...rest` (only valid as the last element).
                if self.cur_is(TokenKind::Ellipsis) {
                    self.next_token();
                    let name = self.cur.literal.clone();
                    self.next_token();
                    elems.push(ArrayBindingElem {
                        name,
                        default: None,
                        is_rest: true,
                    });
                    break;
                }
                // Hole `[, b]` — no binding name.
                if self.cur_is(TokenKind::Comma) {
                    elems.push(ArrayBindingElem {
                        name: String::new(),
                        default: None,
                        is_rest: false,
                    });
                    self.next_token();
                    continue;
                }
                let name = self.cur.literal.clone();
                self.next_token();
                let default = if self.cur_is(TokenKind::Eq) {
                    self.next_token();
                    self.parse_expression(Prec::Comma)
                } else {
                    None
                };
                elems.push(ArrayBindingElem {
                    name,
                    default,
                    is_rest: false,
                });
                if self.cur_is(TokenKind::Comma) {
                    self.next_token();
                }
            }
            self.next_token(); // consume `]`
            BindingPattern::Array(elems)
        } else {
            // Object pattern `{x, y: z = d}`.
            self.next_token(); // consume `{`
            let mut elems: Vec<ObjectBindingElem> = Vec::new();
            while !self.cur_is(TokenKind::RBrace) {
                let key = self.cur.literal.clone();
                self.next_token();
                // `key: target` rename; otherwise target == key.
                let target = if self.cur_is(TokenKind::Colon) {
                    self.next_token();
                    let t = self.cur.literal.clone();
                    self.next_token();
                    t
                } else {
                    key.clone()
                };
                let default = if self.cur_is(TokenKind::Eq) {
                    self.next_token();
                    self.parse_expression(Prec::Comma)
                } else {
                    None
                };
                elems.push(ObjectBindingElem {
                    key,
                    target,
                    default,
                });
                if self.cur_is(TokenKind::Comma) {
                    self.next_token();
                }
            }
            self.next_token(); // consume `}`
            BindingPattern::Object(elems)
        }
    }

    // ========================================================================
    // Block
    // ========================================================================

    pub fn parse_block(&mut self) -> Option<BlockStmt> {
        let pos = self.pos();
        self.next_token(); // {
        let mut statements = Vec::new();
        while !self.cur_is(TokenKind::RBrace) && !self.cur_is(TokenKind::Eof) {
            if let Some(stmt) = self.parse_statement() {
                statements.push(stmt);
            }
        }
        if self.cur_is(TokenKind::RBrace) {
            self.next_token(); // }
        } else {
            self.add_error("expected }");
        }
        Some(BlockStmt { pos, statements })
    }

    // ========================================================================
    // If / While / For
    // ========================================================================

    fn parse_if(&mut self) -> Option<IfStmt> {
        let pos = self.pos();
        if !self.peek_is(TokenKind::LParen) {
            self.add_error("expected ( after if");
            return None;
        }
        self.next_token(); // skip if, cur = (
        self.next_token(); // skip (, cur = condition
        let cond = self.parse_expression(Prec::Comma)?;
        if !self.cur_is(TokenKind::RParen) {
            self.add_error("expected ) after if condition");
        }
        self.next_token(); // skip ), cur = {
        let consequence = self.parse_block()?;
        let alternative = if self.cur_is(TokenKind::Else) {
            self.next_token(); // skip else
            if self.cur_is(TokenKind::If) {
                self.parse_if().map(|s| Box::new(Stmt::If(s)))
            } else {
                self.parse_block().map(|b| Box::new(Stmt::Block(b)))
            }
        } else {
            None
        };
        Some(IfStmt {
            pos,
            cond,
            consequence,
            alternative,
        })
    }

    fn parse_while(&mut self) -> Option<WhileStmt> {
        let pos = self.pos();
        if !self.peek_is(TokenKind::LParen) {
            self.add_error("expected ( after while");
            return None;
        }
        self.next_token(); // while, cur = (
        self.next_token(); // (, cur = condition
        let cond = self.parse_expression(Prec::Comma)?;
        if !self.cur_is(TokenKind::RParen) {
            self.add_error("expected ) after while condition");
        }
        self.next_token(); // ), cur = {
        let body = self.parse_block()?;
        Some(WhileStmt { pos, cond, body })
    }

    fn parse_for(&mut self) -> Option<Stmt> {
        let pos = self.pos();
        if !self.peek_is(TokenKind::LParen) {
            self.add_error("expected ( after for");
            return None;
        }
        self.next_token(); // for
        self.next_token(); // (, cur = first token in for header

        // for-in / for-of detection: speculative parse of `let/const/var? ident
        // (in|of)`. Rewind if it does not match.
        let maybe_for_in = matches!(
            self.cur.kind,
            TokenKind::Let | TokenKind::Const | TokenKind::Var | TokenKind::Ident
        );
        if maybe_for_in {
            let mark = self.mark();
            // Advance past the optional let/const/var. The declaration kind is
            // intentionally discarded here: ForIn/ForOf AST nodes do not carry
            // a decl-kind field, so capturing it would be a dead store.
            if matches!(
                self.cur.kind,
                TokenKind::Let | TokenKind::Const | TokenKind::Var
            ) {
                self.next_token();
            }
            if self.cur_is(TokenKind::Ident) {
                let name = self.cur.literal.clone();
                self.next_token(); // skip ident
                if self.cur_is(TokenKind::In) {
                    self.next_token(); // skip in
                    let iterable = self.parse_expression(Prec::Comma)?;
                    if !self.cur_is(TokenKind::RParen) {
                        self.add_error("expected )");
                    }
                    self.next_token(); // skip )
                    let body = self.parse_block()?;
                    return Some(Stmt::ForIn(Box::new(ForInStmt {
                        pos,
                        name,
                        iterable,
                        body,
                    })));
                }
                if self.cur_is(TokenKind::Of) {
                    self.next_token(); // skip of
                    let iterable = self.parse_expression(Prec::Comma)?;
                    if !self.cur_is(TokenKind::RParen) {
                        self.add_error("expected )");
                    }
                    self.next_token(); // skip )
                    let body = self.parse_block()?;
                    return Some(Stmt::ForOf(Box::new(ForOfStmt {
                        pos,
                        name,
                        iterable,
                        body,
                    })));
                }
            }
            // Not for-in/for-of: rewind to the start of the header and fall
            // through to C-style for parsing.
            self.rewind(mark);
        }

        // C-style for: for (init; cond; post) body
        // Note: parse_var_decl consumes its trailing semicolon; the expression
        // path consumes it explicitly below. After init, cur is at the cond.
        let init = if !self.cur_is(TokenKind::Semi) {
            if matches!(
                self.cur.kind,
                TokenKind::Let | TokenKind::Const | TokenKind::Var
            ) {
                let kind = match self.cur.kind {
                    TokenKind::Const => DeclKind::Const,
                    TokenKind::Var => DeclKind::Var,
                    _ => DeclKind::Let,
                };
                // parse_var_decl consumes the trailing ';', leaving cur on cond.
                Some(Box::new(self.parse_var_decl(kind)))
            } else {
                let e = self.parse_expression(Prec::Comma)?;
                // consume the ';' separating init from cond
                if self.cur_is(TokenKind::Semi) {
                    self.next_token();
                }
                Some(Box::new(Stmt::Expr(ExprStmt {
                    pos: self.pos(),
                    expr: e,
                })))
            }
        } else {
            self.next_token(); // skip leading ';'
            None
        };

        let cond = if !self.cur_is(TokenKind::Semi) && !self.cur_is(TokenKind::RParen) {
            self.parse_expression(Prec::Comma)
        } else {
            None
        };
        if self.cur_is(TokenKind::Semi) {
            self.next_token(); // skip ;
        } else if !self.cur_is(TokenKind::RParen) {
            self.add_error("expected ; or ) in for");
        }
        let post = if !self.cur_is(TokenKind::RParen) {
            self.parse_expression(Prec::Comma)
        } else {
            None
        };
        if self.cur_is(TokenKind::RParen) {
            self.next_token(); // skip )
        } else {
            self.add_error("expected ) after for");
        }
        let body = self.parse_block()?;
        Some(Stmt::For(Box::new(ForStmt {
            pos,
            init,
            cond,
            post,
            body,
        })))
    }

    // ========================================================================
    // Return / Break / Continue / Throw
    // ========================================================================

    fn parse_return(&mut self) -> ReturnStmt {
        let pos = self.pos();
        self.next_token();
        let value = if !self.cur_is(TokenKind::Semi)
            && !self.cur_is(TokenKind::RBrace)
            && !self.cur_is(TokenKind::Eof)
        {
            self.parse_expression(Prec::Comma)
        } else {
            None
        };
        self.skip_semicolon();
        ReturnStmt { pos, value }
    }

    fn parse_break(&mut self) -> BreakStmt {
        let pos = self.pos();
        let label = if self.peek_is(TokenKind::Ident) {
            self.next_token();
            self.cur.literal.clone()
        } else {
            String::new()
        };
        self.next_token();
        self.skip_semicolon();
        BreakStmt { pos, label }
    }

    fn parse_continue(&mut self) -> ContinueStmt {
        let pos = self.pos();
        let label = if self.peek_is(TokenKind::Ident) {
            self.next_token();
            self.cur.literal.clone()
        } else {
            String::new()
        };
        self.next_token();
        self.skip_semicolon();
        ContinueStmt { pos, label }
    }

    fn parse_throw(&mut self) -> ThrowStmt {
        let pos = self.pos();
        self.next_token();
        let value = self
            .parse_expression(Prec::Comma)
            .unwrap_or(Expr::Null(NullLit { pos: pos.clone() }));
        self.skip_semicolon();
        ThrowStmt { pos, value }
    }

    // ========================================================================
    // Try / Catch / Finally
    // ========================================================================

    fn parse_try(&mut self) -> Option<TryStmt> {
        let pos = self.pos();
        self.next_token(); // skip try, cur = {
        let block = self.parse_block()?;

        let catch = if self.cur_is(TokenKind::Catch) {
            self.next_token(); // catch, cur = (
            self.next_token(); // (, cur = ident
            let catch_pos = self.pos();
            let mut name = String::new();
            let mut type_anno = None;
            if self.cur_is(TokenKind::Ident) {
                name = self.cur.literal.clone();
                if self.peek_is(TokenKind::Colon) {
                    self.next_token(); // ident
                    self.next_token(); // colon
                    type_anno = self.parse_type();
                }
            }
            self.next_token(); // past ident or type, cur = )
            if !self.cur_is(TokenKind::RParen) {
                self.add_error("expected ) after catch");
            }
            self.next_token(); // skip ), cur = {
            let body = self.parse_block()?;
            Some(CatchClause {
                pos: catch_pos,
                name,
                type_anno,
                body,
            })
        } else {
            None
        };

        let finalizer = if self.cur_is(TokenKind::Finally) {
            self.next_token(); // skip finally, cur = {
            self.parse_block()
        } else {
            None
        };

        Some(TryStmt {
            pos,
            block,
            catch,
            finalizer,
        })
    }
}

#[derive(Clone, Copy)]
enum DeclKind {
    Let,
    Const,
    Var,
}
