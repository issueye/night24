//! Expression parsing (Pratt-style).

use super::{Parser, Prec};
use crate::ast::*;
use crate::lexer::{is_keyword, TokenKind};

impl Parser {
    /// Parse an expression with at least the given precedence.
    pub fn parse_expression(&mut self, prec: Prec) -> Option<Expr> {
        let start_offset = self.cur.offset;
        let start_line = self.cur.line;
        let start_col = self.cur.column;
        let mut left = self.parse_prefix()?;

        // If the prefix parser did not advance, advance here.
        if self.cur.offset == start_offset
            && self.cur.line == start_line
            && self.cur.column == start_col
        {
            self.next_token();
        }

        while prec < self.cur_precedence() && !self.cur_is(TokenKind::Semi) {
            // parse_infix takes `left` by value; clone so a None result can still
            // return the original expression unchanged.
            left = match self.parse_infix(left.clone()) {
                Some(e) => e,
                None => break,
            };
        }
        Some(left)
    }

    fn parse_prefix(&mut self) -> Option<Expr> {
        let pos = self.pos();
        match self.cur.kind {
            // Dynamic `import(specifier)` — parsed as an expression so it can
            // be chained (`.then(...)`) or assigned. Requires `import` `(`.
            TokenKind::Import if self.peek_is(TokenKind::LParen) => {
                self.next_token(); // import
                self.next_token(); // (
                                   // The specifier is normally a string literal; parse it directly
                                   // to avoid the prefix-loop auto-advance subtleties.
                let source = if self.cur_is(TokenKind::String) {
                    let s = Expr::String(StringLit {
                        pos: self.pos(),
                        literal: self.cur.literal.clone(),
                    });
                    self.next_token(); // past the string
                    s
                } else {
                    // Fallback: parse a single prefix atom and advance.
                    let s = self
                        .parse_prefix()
                        .unwrap_or(Expr::Undefined(UndefinedLit { pos: pos.clone() }));
                    self.next_token();
                    s
                };
                // Consume the closing `)`. (cur is `)` here; peek is whatever
                // follows, so we advance past cur rather than using expect_peek.)
                if self.cur_is(TokenKind::RParen) {
                    self.next_token();
                } else {
                    self.add_error("expected ) after import()");
                }
                Some(Expr::DynamicImport(Box::new(DynamicImportExpr {
                    pos,
                    source,
                })))
            }
            TokenKind::Ident => Some(Expr::Ident(Ident {
                pos,
                name: self.cur.literal.clone(),
            })),
            TokenKind::Number => {
                let lit = self.cur.literal.clone();
                let value = parse_number_literal_value(&lit);
                let is_int = !lit.chars().any(|c| c == '.' || c == 'e' || c == 'E');
                Some(Expr::Number(NumberLit {
                    pos,
                    literal: lit,
                    value,
                    is_int,
                }))
            }
            TokenKind::String => Some(Expr::String(StringLit {
                pos,
                literal: self.cur.literal.clone(),
            })),
            TokenKind::Template => Some(Expr::Template(TemplateLit {
                pos,
                literal: self.cur.literal.clone(),
            })),
            TokenKind::Regexp => Some(Expr::Regexp(RegExpLit {
                pos,
                literal: self.cur.literal.clone(),
            })),
            TokenKind::True => Some(Expr::Bool(BoolLit { pos, value: true })),
            TokenKind::False => Some(Expr::Bool(BoolLit { pos, value: false })),
            TokenKind::Null => Some(Expr::Null(NullLit { pos })),
            TokenKind::Undefined => Some(Expr::Undefined(UndefinedLit { pos })),
            TokenKind::This => Some(Expr::This(ThisExpr { pos })),
            TokenKind::Super => Some(Expr::Super(SuperExpr {
                pos,
                method: String::new(),
            })),
            TokenKind::Bang
            | TokenKind::Minus
            | TokenKind::Plus
            | TokenKind::Tilde
            | TokenKind::Typeof
            | TokenKind::Void
            | TokenKind::Delete
            | TokenKind::PlusPlus
            | TokenKind::MinusMinus => self.parse_prefix_op(),
            TokenKind::Await => self.parse_await(),
            TokenKind::LParen => self.parse_paren_or_arrow(),
            TokenKind::LBrack => self.parse_array(),
            TokenKind::LBrace => self.parse_object(),
            TokenKind::New => self.parse_new(),
            TokenKind::Match => Some(self.parse_match_expr()),
            TokenKind::Function => self.parse_function_expr(),
            TokenKind::Async => self.parse_async_func_expr(),
            TokenKind::Class => self.parse_class_expr(),
            _ => {
                self.add_error(format!(
                    "no prefix parser for {:?} ({:?})",
                    self.cur.kind, self.cur.literal
                ));
                None
            }
        }
    }

    fn parse_prefix_op(&mut self) -> Option<Expr> {
        let pos = self.pos();
        let op = self.cur.literal.clone();
        self.next_token();
        let right = self.parse_expression(Prec::Prefix)?;
        Some(Expr::Prefix(Box::new(PrefixExpr { pos, op, right })))
    }

    fn parse_await(&mut self) -> Option<Expr> {
        let pos = self.pos();
        self.next_token();
        let value = self.parse_expression(Prec::Prefix)?;
        Some(Expr::Await(Box::new(AwaitExpr { pos, value })))
    }

    fn parse_paren_or_arrow(&mut self) -> Option<Expr> {
        self.next_token(); // consume (
        if self.cur_is(TokenKind::RParen) {
            self.next_token(); // )
            if self.cur_is(TokenKind::Arrow) {
                return self.parse_arrow_lambda(Vec::new());
            }
            return None;
        }
        // Try arrow parameter list; rewind if it does not lead to `=>`.
        let mark = self.mark();
        if let Some(params) = self.try_parse_param_list() {
            if self.cur_is(TokenKind::Arrow) {
                self.commit(mark);
                return self.parse_arrow_lambda(params);
            }
        }
        // Not an arrow: rewind to just after '(' and parse a parenthesized expression.
        self.rewind(mark);
        let expr = self.parse_expression(Prec::Comma)?;
        if self.cur_is(TokenKind::RParen) {
            self.next_token();
        }
        if self.cur_is(TokenKind::Arrow) {
            // (expr) => body — treat single-ident param
            return self.parse_arrow_lambda(vec![Param {
                pos: Position::default(),
                name: "_".into(),
                type_anno: None,
                default: None,
                spread: false,
                optional: false,
            }]);
        }
        Some(expr)
    }

    fn parse_arrow_lambda(&mut self, params: Vec<Param>) -> Option<Expr> {
        let pos = self.pos();
        self.next_token(); // arrow
        let body = if self.cur_is(TokenKind::LBrace) {
            ArrowBody::Block(self.parse_block()?)
        } else {
            ArrowBody::Expr(self.parse_expression(Prec::Comma)?)
        };
        Some(Expr::Arrow(Box::new(ArrowFuncExpr {
            pos,
            params,
            return_t: None,
            body,
            is_async: false,
        })))
    }

    fn parse_array(&mut self) -> Option<Expr> {
        let pos = self.pos();
        self.next_token(); // [
        let mut elements = Vec::new();
        if self.cur_is(TokenKind::RBrack) {
            self.next_token();
            return Some(Expr::Array(ArrayLit { pos, elements }));
        }
        loop {
            if self.cur_is(TokenKind::Ellipsis) {
                self.next_token();
                let v = self.parse_expression(Prec::Comma)?;
                elements.push(Expr::Spread(Box::new(SpreadExpr {
                    pos: self.pos(),
                    value: v,
                })));
            } else {
                elements.push(self.parse_expression(Prec::Comma)?);
            }
            if self.cur_is(TokenKind::Comma) {
                self.next_token();
                if self.cur_is(TokenKind::RBrack) {
                    break;
                }
                continue;
            }
            break;
        }
        if self.cur_is(TokenKind::RBrack) {
            self.next_token();
        } else {
            self.add_error("expected ]");
        }
        Some(Expr::Array(ArrayLit { pos, elements }))
    }

    fn parse_object(&mut self) -> Option<Expr> {
        let pos = self.pos();
        self.next_token(); // {
        let mut properties = Vec::new();
        if self.cur_is(TokenKind::RBrace) {
            self.next_token(); // }
            return Some(Expr::Object(ObjectLit { pos, properties }));
        }
        while !self.cur_is(TokenKind::RBrace) && !self.cur_is(TokenKind::Eof) {
            if let Some(prop) = self.parse_property() {
                properties.push(prop);
            }
            if self.cur_is(TokenKind::Comma) {
                self.next_token();
            }
        }
        if self.cur_is(TokenKind::RBrace) {
            self.next_token();
        } else {
            self.add_error("expected }");
        }
        Some(Expr::Object(ObjectLit { pos, properties }))
    }

    fn parse_property(&mut self) -> Option<Property> {
        let pos = self.pos();
        if self.cur_is(TokenKind::Ellipsis) {
            self.next_token();
            let value = self.parse_expression(Prec::Comma)?;
            return Some(Property {
                pos: pos.clone(),
                key: value.clone(),
                value,
                computed: false,
                shorthand: false,
                spread: true,
                is_accessor: false,
            });
        }
        if self.cur_is(TokenKind::LBrack) {
            self.next_token();
            let key = self.parse_expression(Prec::Comma)?;
            if !self.cur_is(TokenKind::RBrack) {
                self.add_error("expected ]");
            }
            self.next_token(); // skip ]
            if !self.cur_is(TokenKind::Colon) {
                self.add_error("expected : after computed key");
            }
            self.next_token(); // skip :
            let value = self.parse_expression(Prec::Comma)?;
            return Some(Property {
                pos: pos.clone(),
                key,
                value,
                computed: true,
                shorthand: false,
                spread: false,
                is_accessor: false,
            });
        }

        // Parse key as a simple primary (identifier / string / number / keyword).
        let key = match self.cur.kind {
            TokenKind::Ident => Expr::Ident(Ident {
                pos: pos.clone(),
                name: self.cur.literal.clone(),
            }),
            TokenKind::String => Expr::String(StringLit {
                pos: pos.clone(),
                literal: self.cur.literal.clone(),
            }),
            TokenKind::Number => {
                let lit = self.cur.literal.clone();
                let value = parse_number_literal_value(&lit);
                let is_int = !lit.chars().any(|c| c == '.' || c == 'e' || c == 'E');
                Expr::Number(NumberLit {
                    pos: pos.clone(),
                    literal: lit,
                    value,
                    is_int,
                })
            }
            _ => {
                if is_keyword(self.cur.kind) {
                    Expr::Ident(Ident {
                        pos: pos.clone(),
                        name: self.cur.literal.clone(),
                    })
                } else {
                    self.add_error("expected property key");
                    return None;
                }
            }
        };
        self.next_token(); // advance past key

        // Method shorthand: key(args) { body }
        if self.cur_is(TokenKind::LParen) {
            if let Expr::Ident(ident) = &key {
                let name = ident.name.clone();
                let (params, _ret) = self.parse_func_params();
                let body = self.parse_block()?;
                return Some(Property {
                    pos: pos.clone(),
                    key: key.clone(),
                    value: Expr::Func(Box::new(FuncExpr {
                        pos,
                        name,
                        params,
                        return_t: None,
                        body,
                        is_async: false,
                    })),
                    computed: false,
                    shorthand: false,
                    spread: false,
                    is_accessor: false,
                });
            }
        }

        // Key-value: key: value
        if self.cur_is(TokenKind::Colon) {
            self.next_token();
            let value = self.parse_expression(Prec::Comma)?;
            return Some(Property {
                pos,
                key,
                value,
                computed: false,
                shorthand: false,
                spread: false,
                is_accessor: false,
            });
        }

        // Shorthand: { key }
        Some(Property {
            pos,
            key: key.clone(),
            value: key,
            computed: false,
            shorthand: true,
            spread: false,
            is_accessor: false,
        })
    }

    fn parse_new(&mut self) -> Option<Expr> {
        let pos = self.pos();
        self.next_token();
        let callee = self.parse_expression(Prec::Call)?;
        let mut args = Vec::new();
        if self.cur_is(TokenKind::LParen) {
            self.next_token();
            args = self.parse_call_args();
            if !self.cur_is(TokenKind::RParen) {
                self.add_error("expected ) after new args");
            } else {
                self.next_token();
            }
        }
        Some(Expr::New(Box::new(NewExpr { pos, callee, args })))
    }

    fn parse_function_expr(&mut self) -> Option<Expr> {
        let pos = self.pos();
        let mut name = String::new();
        if self.peek_is(TokenKind::Ident) {
            self.next_token();
            name = self.cur.literal.clone();
        }
        self.next_token();
        let (params, return_t) = self.parse_func_params();
        let body = self.parse_block()?;
        Some(Expr::Func(Box::new(FuncExpr {
            pos,
            name,
            params,
            return_t,
            body,
            is_async: false,
        })))
    }

    fn parse_async_func_expr(&mut self) -> Option<Expr> {
        self.next_token(); // skip async
        if self.cur_is(TokenKind::Function) {
            let mut f = self.parse_function_expr()?;
            if let Expr::Func(fe) = &mut f {
                fe.is_async = true;
            }
            return Some(f);
        }
        if self.cur_is(TokenKind::LParen) {
            let mut expr = self.parse_paren_or_arrow()?;
            if let Expr::Arrow(a) = &mut expr {
                a.is_async = true;
            }
            return Some(expr);
        }
        // async ident => ... (single-param arrow)
        if self.cur_is(TokenKind::Ident) {
            let pos = self.pos();
            let name = self.cur.literal.clone();
            let param = Param {
                pos,
                name,
                type_anno: None,
                default: None,
                spread: false,
                optional: false,
            };
            self.next_token();
            if self.cur_is(TokenKind::Arrow) {
                let mut arrow = self.parse_arrow_lambda(vec![param])?;
                if let Expr::Arrow(a) = &mut arrow {
                    a.is_async = true;
                }
                return Some(arrow);
            }
        }
        Some(Expr::Ident(Ident {
            pos: self.pos(),
            name: self.cur.literal.clone(),
        }))
    }

    fn parse_class_expr(&mut self) -> Option<Expr> {
        // A class expression shares parsing with a class declaration; both build a
        // ClassDecl AST node. The statement form also binds the name in scope, but
        // for the expression form we return it directly.
        let decl = self.parse_class_decl()?;
        Some(Expr::to_class_node(decl))
    }

    fn parse_infix(&mut self, left: Expr) -> Option<Expr> {
        let pos = self.pos();
        match self.cur.kind {
            TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent
            | TokenKind::Pow
            | TokenKind::EqEqEq
            | TokenKind::NeqEq
            | TokenKind::Lt
            | TokenKind::LtEq
            | TokenKind::Gt
            | TokenKind::GtEq
            | TokenKind::AndAnd
            | TokenKind::OrOr
            | TokenKind::QmQm
            | TokenKind::Amp
            | TokenKind::Pipe
            | TokenKind::Caret
            | TokenKind::LShift
            | TokenKind::RShift
            | TokenKind::UrShift
            | TokenKind::In
            | TokenKind::Instanceof => {
                let op = self.cur.literal.clone();
                let prec = self.cur_precedence();
                self.next_token();
                let right = self.parse_expression(prec)?;
                Some(Expr::Infix(Box::new(InfixExpr {
                    pos,
                    op,
                    left,
                    right: Some(right),
                })))
            }
            TokenKind::LParen => {
                self.next_token(); // skip (
                let args = self.parse_call_args();
                if !self.cur_is(TokenKind::RParen) {
                    self.add_error("expected )");
                } else {
                    self.next_token(); // skip )
                }
                Some(Expr::Call(Box::new(CallExpr {
                    pos,
                    callee: left,
                    args,
                })))
            }
            TokenKind::Dot => {
                self.next_token();
                let name = self.cur.literal.clone();
                self.next_token();
                Some(Expr::Member(Box::new(MemberExpr {
                    pos: pos.clone(),
                    object: left,
                    property: Expr::Ident(Ident { pos, name }),
                    computed: false,
                })))
            }
            TokenKind::LBrack => {
                self.next_token();
                let index = self.parse_expression(Prec::Comma)?;
                if !self.cur_is(TokenKind::RBrack) {
                    self.add_error("expected ]");
                } else {
                    self.next_token();
                }
                Some(Expr::Index(Box::new(IndexExpr { pos, left, index })))
            }
            TokenKind::QmDot => {
                self.next_token(); // skip ?.
                if self.cur_is(TokenKind::LBrack) {
                    self.next_token();
                    let index = self.parse_expression(Prec::Comma)?;
                    if self.cur_is(TokenKind::RBrack) {
                        self.next_token();
                    }
                    return Some(Expr::Optional(Box::new(OptionalExpr {
                        pos: pos.clone(),
                        object: left,
                        property: index,
                        computed: true,
                        is_call: false,
                        args: Vec::new(),
                    })));
                }
                if self.cur_is(TokenKind::LParen) {
                    self.next_token();
                    let args = self.parse_call_args();
                    if self.cur_is(TokenKind::RParen) {
                        self.next_token();
                    }
                    return Some(Expr::Optional(Box::new(OptionalExpr {
                        pos: pos.clone(),
                        object: left,
                        property: Expr::Null(NullLit { pos }),
                        computed: false,
                        is_call: true,
                        args,
                    })));
                }
                let name = self.cur.literal.clone();
                self.next_token();
                Some(Expr::Optional(Box::new(OptionalExpr {
                    pos: pos.clone(),
                    object: left,
                    property: Expr::Ident(Ident { pos, name }),
                    computed: false,
                    is_call: false,
                    args: Vec::new(),
                })))
            }
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                let op = self.cur.literal.clone();
                self.next_token();
                Some(Expr::Infix(Box::new(InfixExpr {
                    pos,
                    op,
                    left,
                    right: None,
                })))
            }
            TokenKind::Question => {
                self.next_token(); // skip ?
                let consequent = self.parse_expression(Prec::Comma)?;
                if !self.cur_is(TokenKind::Colon) {
                    self.add_error("expected : in ternary");
                    return Some(left);
                }
                self.next_token(); // skip :
                let alternate = self.parse_expression(Prec::Comma)?;
                Some(Expr::Ternary(Box::new(TernaryExpr {
                    pos,
                    cond: left,
                    consequent,
                    alternate,
                })))
            }
            TokenKind::Arrow => {
                // ident => body
                let (param_pos, name) = match &left {
                    Expr::Ident(i) => (i.pos.clone(), i.name.clone()),
                    _ => {
                        self.next_token(); // skip stray =>
                        return Some(left);
                    }
                };
                self.next_token(); // skip =>
                let body = if self.cur_is(TokenKind::LBrace) {
                    ArrowBody::Block(self.parse_block()?)
                } else {
                    ArrowBody::Expr(self.parse_expression(Prec::Comma)?)
                };
                Some(Expr::Arrow(Box::new(ArrowFuncExpr {
                    pos,
                    params: vec![Param {
                        pos: param_pos,
                        name,
                        type_anno: None,
                        default: None,
                        spread: false,
                        optional: false,
                    }],
                    return_t: None,
                    body,
                    is_async: false,
                })))
            }
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
            | TokenKind::CaretEq => {
                let op = self.cur.literal.clone();
                self.next_token();
                let right = self.parse_expression(Prec::Assign)?;
                Some(Expr::Assign(Box::new(AssignExpr {
                    pos,
                    op,
                    left,
                    right,
                })))
            }
            _ => None,
        }
    }

    fn parse_call_args(&mut self) -> Vec<Expr> {
        let mut args = Vec::new();
        if self.cur_is(TokenKind::RParen) {
            return args;
        }
        loop {
            if self.cur_is(TokenKind::Ellipsis) {
                self.next_token();
                let v = self
                    .parse_expression(Prec::Comma)
                    .unwrap_or(Expr::Null(NullLit { pos: self.pos() }));
                args.push(Expr::Spread(Box::new(SpreadExpr {
                    pos: self.pos(),
                    value: v,
                })));
            } else {
                if let Some(e) = self.parse_expression(Prec::Comma) {
                    args.push(e);
                }
            }
            if self.cur_is(TokenKind::Comma) {
                self.next_token();
                continue;
            }
            break;
        }
        args
    }
}

/// Parse a number literal token (handles hex/binary/octal/float).
pub fn parse_number_literal_value(lit: &str) -> f64 {
    let bytes = lit.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'0' {
        match bytes[1] {
            b'x' | b'X' => {
                return u64::from_str_radix(&lit[2..], 16)
                    .map(|v| v as f64)
                    .unwrap_or(0.0)
            }
            b'b' | b'B' => {
                return u64::from_str_radix(&lit[2..], 2)
                    .map(|v| v as f64)
                    .unwrap_or(0.0)
            }
            b'o' | b'O' => {
                return u64::from_str_radix(&lit[2..], 8)
                    .map(|v| v as f64)
                    .unwrap_or(0.0)
            }
            _ => {}
        }
    }
    lit.parse::<f64>().unwrap_or(0.0)
}
