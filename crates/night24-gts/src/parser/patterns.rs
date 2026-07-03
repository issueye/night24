//! Pattern parsing for `match` arms.

use super::*;
use crate::ast::*;
use crate::lexer::TokenKind;
use crate::parser::parse_number_literal_value;

impl Parser {
    /// Parse a `match expr { arms }` expression. Expects cur on `match`.
    pub fn parse_match_expr(&mut self) -> Expr {
        let pos = self.pos();
        self.next_token(); // skip match
        let expr = match self.parse_expression(Prec::Comma) {
            Some(e) => e,
            None => return Expr::Null(NullLit { pos }),
        };
        if !self.cur_is(TokenKind::LBrace) {
            self.add_error("expected { after match subject");
            return Expr::Null(NullLit { pos });
        }
        self.next_token(); // {
        let mut arms = Vec::new();
        while !self.cur_is(TokenKind::RBrace) && !self.cur_is(TokenKind::Eof) {
            if let Some(arm) = self.parse_match_arm() {
                arms.push(arm);
            }
            if self.cur_is(TokenKind::Comma) {
                self.next_token();
                continue;
            }
            if self.cur_is(TokenKind::RBrace) {
                break;
            }
            // Recovery: skip to next arm boundary
            self.next_token();
        }
        if self.cur_is(TokenKind::RBrace) {
            self.next_token(); // }
        }
        Expr::Match(Box::new(MatchExpr { pos, expr, arms }))
    }

    fn parse_match_arm(&mut self) -> Option<MatchArm> {
        let pos = self.pos();
        let pattern = self.parse_pattern()?;
        let mut binding_name = String::new();
        let mut binding_pos = Position::default();
        if self.cur_is(TokenKind::LParen) {
            self.next_token();
            if !self.cur_is(TokenKind::Ident) {
                self.add_error("expected identifier in match arm binding");
                return None;
            }
            binding_name = self.cur.literal.clone();
            binding_pos = self.pos();
            self.next_token();
            if !self.cur_is(TokenKind::RParen) {
                self.add_error("expected ) after match arm binding");
                return None;
            }
            self.next_token();
        }
        let guard = if self.cur_is(TokenKind::If) {
            self.next_token();
            self.parse_expression(Prec::Assign)
        } else {
            None
        };
        if !self.cur_is(TokenKind::Arrow) {
            self.add_error("expected => in match arm");
            return None;
        }
        self.next_token(); // =>
        let body = if self.cur_is(TokenKind::LBrace) {
            MatchBody::Block(self.parse_block()?)
        } else {
            MatchBody::Expr(self.parse_expression(Prec::Comma)?)
        };
        Some(MatchArm {
            pos,
            pattern,
            binding_name,
            binding_pos,
            guard,
            body,
        })
    }

    fn parse_pattern(&mut self) -> Option<Pattern> {
        let primary = self.parse_primary_pattern()?;
        // OR pattern: primary | primary | ...
        if self.cur_is(TokenKind::Pipe) {
            let pos = self.pos();
            let mut alts = vec![primary];
            while self.cur_is(TokenKind::Pipe) {
                self.next_token();
                if let Some(alt) = self.parse_primary_pattern() {
                    alts.push(alt);
                }
            }
            return Some(Pattern::Or(OrPattern {
                pos,
                alternatives: alts,
            }));
        }
        Some(primary)
    }

    fn parse_primary_pattern(&mut self) -> Option<Pattern> {
        let pos = self.pos();
        match self.cur.kind {
            TokenKind::Number => {
                let lit = self.cur.literal.clone();
                let value = parse_number_literal_value(&lit);
                let num = Expr::Number(NumberLit {
                    pos: pos.clone(),
                    literal: lit,
                    value,
                    is_int: true,
                });
                self.next_token();
                if self.cur_is(TokenKind::DotDot) || self.cur_is(TokenKind::DotDotEq) {
                    let inclusive = self.cur_is(TokenKind::DotDotEq);
                    self.next_token();
                    let end = self.parse_literal_expr();
                    return Some(Pattern::Range(Box::new(RangePattern {
                        pos,
                        start: num,
                        end,
                        inclusive,
                    })));
                }
                if self.cur_is(TokenKind::Pipe) {
                    return self.parse_or_pattern_continue(Pattern::Literal(Box::new(
                        LiteralPattern { pos, value: num },
                    )));
                }
                Some(Pattern::Literal(Box::new(LiteralPattern {
                    pos,
                    value: num,
                })))
            }
            TokenKind::String => {
                let lit = self.cur.literal.clone();
                let s = Expr::String(StringLit {
                    pos: pos.clone(),
                    literal: lit,
                });
                self.next_token();
                if self.cur_is(TokenKind::Pipe) {
                    return self.parse_or_pattern_continue(Pattern::Literal(Box::new(
                        LiteralPattern { pos, value: s },
                    )));
                }
                Some(Pattern::Literal(Box::new(LiteralPattern { pos, value: s })))
            }
            TokenKind::True => {
                let b = Expr::Bool(BoolLit {
                    pos: pos.clone(),
                    value: true,
                });
                self.next_token();
                if self.cur_is(TokenKind::Pipe) {
                    return self.parse_or_pattern_continue(Pattern::Literal(Box::new(
                        LiteralPattern { pos, value: b },
                    )));
                }
                Some(Pattern::Literal(Box::new(LiteralPattern { pos, value: b })))
            }
            TokenKind::False => {
                let b = Expr::Bool(BoolLit {
                    pos: pos.clone(),
                    value: false,
                });
                self.next_token();
                if self.cur_is(TokenKind::Pipe) {
                    return self.parse_or_pattern_continue(Pattern::Literal(Box::new(
                        LiteralPattern { pos, value: b },
                    )));
                }
                Some(Pattern::Literal(Box::new(LiteralPattern { pos, value: b })))
            }
            TokenKind::Null => {
                let n = Expr::Null(NullLit { pos: pos.clone() });
                self.next_token();
                if self.cur_is(TokenKind::Pipe) {
                    return self.parse_or_pattern_continue(Pattern::Literal(Box::new(
                        LiteralPattern { pos, value: n },
                    )));
                }
                Some(Pattern::Literal(Box::new(LiteralPattern { pos, value: n })))
            }
            TokenKind::Undefined => {
                let u = Expr::Undefined(UndefinedLit { pos: pos.clone() });
                self.next_token();
                if self.cur_is(TokenKind::Pipe) {
                    return self.parse_or_pattern_continue(Pattern::Literal(Box::new(
                        LiteralPattern { pos, value: u },
                    )));
                }
                Some(Pattern::Literal(Box::new(LiteralPattern { pos, value: u })))
            }
            TokenKind::Ident => {
                let name = self.cur.literal.clone();
                self.next_token();
                if name == "_" {
                    if self.cur_is(TokenKind::DotDot) || self.cur_is(TokenKind::DotDotEq) {
                        let inclusive = self.cur_is(TokenKind::DotDotEq);
                        self.next_token();
                        let end = self
                            .parse_expression(Prec::Comma)
                            .unwrap_or(Expr::Null(NullLit { pos: pos.clone() }));
                        return Some(Pattern::Range(Box::new(RangePattern {
                            pos: pos.clone(),
                            start: Expr::Ident(Ident {
                                pos,
                                name: "_".into(),
                            }),
                            end,
                            inclusive,
                        })));
                    }
                    return Some(Pattern::Wildcard(WildcardPattern { pos }));
                }
                if self.cur_is(TokenKind::DotDot) || self.cur_is(TokenKind::DotDotEq) {
                    let inclusive = self.cur_is(TokenKind::DotDotEq);
                    self.next_token();
                    let end = self
                        .parse_expression(Prec::Comma)
                        .unwrap_or(Expr::Null(NullLit { pos: pos.clone() }));
                    return Some(Pattern::Range(Box::new(RangePattern {
                        pos: pos.clone(),
                        start: Expr::Ident(Ident {
                            pos,
                            name: name.clone(),
                        }),
                        end,
                        inclusive,
                    })));
                }
                Some(Pattern::Ident(IdentPattern { pos, name }))
            }
            _ => {
                self.add_error(format!("unexpected token in pattern: {:?}", self.cur.kind));
                None
            }
        }
    }

    fn parse_or_pattern_continue(&mut self, first: Pattern) -> Option<Pattern> {
        let pos = self.pos();
        let mut alts = vec![first];
        while self.cur_is(TokenKind::Pipe) {
            self.next_token();
            if let Some(next) = self.parse_primary_pattern() {
                alts.push(next);
            }
        }
        if alts.len() == 1 {
            return Some(alts.remove(0));
        }
        Some(Pattern::Or(OrPattern {
            pos,
            alternatives: alts,
        }))
    }

    /// Parse a single literal token (number/string) without the full Pratt pipeline.
    fn parse_literal_expr(&mut self) -> Expr {
        let pos = self.pos();
        match self.cur.kind {
            TokenKind::Number => {
                let lit = self.cur.literal.clone();
                let value = parse_number_literal_value(&lit);
                self.next_token();
                Expr::Number(NumberLit {
                    pos,
                    literal: lit,
                    value,
                    is_int: true,
                })
            }
            TokenKind::String => {
                let lit = self.cur.literal.clone();
                self.next_token();
                Expr::String(StringLit { pos, literal: lit })
            }
            _ => self
                .parse_expression(Prec::Comma)
                .unwrap_or(Expr::Null(NullLit { pos })),
        }
    }
}
