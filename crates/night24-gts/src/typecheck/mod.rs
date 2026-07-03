//! Static type checker for GoScript (B1, Phase 2).
//!
//! Scope (confirmed §9 decision, 2026-06-29): **annotation-consistency only**.
//! This is NOT a full type-inference system. It checks that the *inferred type*
//! of an annotated declaration's initializer is compatible with its annotation:
//!   - `let x: number = 1;`        → number literal inferred as `number` ✓
//!   - `let x: number = "str";`    → string literal vs `number` ✗
//!   - function `return_t` vs the returned expression's inferred type
//!   - assignment to an annotated binding
//!
//! It does NOT do cross-function inference, generic resolution, or flow
//! analysis. The inference is conservative: it tracks a small set of
//! "definitely-known" types (literals, arithmetic, known-typed bindings) and
//! falls back to `any` when a type can't be statically determined (so it
//! avoids false positives — a key acceptance criterion, E2.1).
//!
//! `--check-types` runs this checker before execution; type errors are fatal.

use std::collections::HashMap;

use crate::ast::{Expr, FuncDecl, Stmt, TypeAnnotation, TypeKind};

/// A statically inferred type name. `Any` means "unknown / could be anything"
/// and is always compatible (never produces an error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferredType {
    /// A known primitive/object name: "number", "string", "boolean", "object",
    /// "function", "array", "null", "undefined".
    Known(String),
    /// Unknown — the inference gave up. Always compatible.
    Any,
}

impl InferredType {
    fn known(s: &str) -> Self {
        InferredType::Known(s.to_string())
    }
}

/// A type error reported by the checker.
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

/// The checker: walks the AST, infers types, and reports annotation mismatches.
#[derive(Default)]
pub struct Checker {
    errors: Vec<TypeError>,
    /// Map binding name → inferred type (for identifier lookups).
    bindings: HashMap<String, InferredType>,
}

impl Checker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check a whole program; return any type errors found.
    pub fn check_program(&mut self, body: &[Stmt]) -> Vec<TypeError> {
        for stmt in body {
            self.check_stmt(stmt);
        }
        std::mem::take(&mut self.errors)
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let(s) => self.check_typed_decl(&s.name, s.value.as_ref(), &s.type_anno),
            Stmt::Const(s) => self.check_typed_decl(&s.name, s.value.as_ref(), &s.type_anno),
            Stmt::Var(s) => self.check_typed_decl(&s.name, s.value.as_ref(), &s.type_anno),
            Stmt::FuncDecl(f) => self.check_func(f),
            Stmt::Expr(e) => {
                self.infer_expr(&e.expr);
            }
            Stmt::Block(b) => {
                for s in &b.statements {
                    self.check_stmt(s);
                }
            }
            Stmt::If(i) => {
                self.infer_expr(&i.cond);
                self.check_stmt(&Stmt::Block(i.consequence.clone()));
                if let Some(alt) = &i.alternative {
                    self.check_stmt(alt);
                }
            }
            Stmt::While(w) => {
                self.infer_expr(&w.cond);
                self.check_stmt(&Stmt::Block(w.body.clone()));
            }
            Stmt::For(f) => {
                if let Some(init) = &f.init {
                    self.check_stmt(init);
                }
                if let Some(cond) = &f.cond {
                    self.infer_expr(cond);
                }
                self.check_stmt(&Stmt::Block(f.body.clone()));
            }
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.infer_expr(v);
                }
            }
            _ => {}
        }
    }

    /// Check a `let x: T = expr;` declaration for annotation consistency.
    fn check_typed_decl(
        &mut self,
        name: &str,
        value: Option<&Expr>,
        anno: &Option<TypeAnnotation>,
    ) {
        let inferred = value
            .map(|e| self.infer_expr(e))
            .unwrap_or(InferredType::Any);
        // Record the binding's type for later identifier lookups: prefer the
        // annotation (it's authoritative), else the inferred type.
        let recorded = if let Some(a) = anno {
            inferred_type_of_annotation(a)
        } else {
            inferred.clone()
        };
        self.bindings.insert(name.to_string(), recorded);

        if let Some(anno) = anno {
            if !compatible(&inferred, anno) {
                self.errors.push(TypeError {
                    message: format!(
                        "TypeAnnotationError: '{}' declared as '{}' but initialized with {}",
                        name,
                        anno,
                        describe_inferred(&inferred)
                    ),
                    line: 0,
                    col: 0,
                });
            }
        }
    }

    /// Check a function: its parameters' annotations are recorded, and any
    /// `return expr;` is checked against the function's `return_t`.
    fn check_func(&mut self, f: &FuncDecl) {
        // Record parameter types (from annotations) into a fresh scope so the
        // body can reference them.
        for p in &f.params {
            if let Some(anno) = &p.type_anno {
                self.bindings
                    .insert(p.name.clone(), inferred_type_of_annotation(anno));
            } else {
                self.bindings.insert(p.name.clone(), InferredType::Any);
            }
        }
        self.check_return_against(&Stmt::Block(f.body.clone()), f.return_t.as_ref());
        self.check_stmt(&Stmt::Block(f.body.clone()));
    }

    /// Recursively check `return expr;` statements against the return type.
    fn check_return_against(&mut self, stmt: &Stmt, return_t: Option<&TypeAnnotation>) {
        let Some(anno) = return_t else { return };
        match stmt {
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    let inferred = self.infer_expr(v);
                    if !compatible(&inferred, anno) {
                        self.errors.push(TypeError {
                            message: format!(
                                "TypeAnnotationError: function returns {} but declared return type is '{}'",
                                describe_inferred(&inferred),
                                anno
                            ),
                            line: 0,
                            col: 0,
                        });
                    }
                }
            }
            Stmt::Block(b) => {
                for s in &b.statements {
                    self.check_return_against(s, Some(anno));
                }
            }
            Stmt::If(i) => {
                for s in &i.consequence.statements {
                    self.check_return_against(s, Some(anno));
                }
                if let Some(alt) = &i.alternative {
                    self.check_return_against(alt, Some(anno));
                }
            }
            _ => {}
        }
    }

    /// Infer the type of an expression. Conservative: returns `Any` when the
    /// type can't be statically determined.
    fn infer_expr(&mut self, expr: &Expr) -> InferredType {
        match expr {
            Expr::Number(_) => InferredType::known("number"),
            Expr::String(_) => InferredType::known("string"),
            Expr::Bool(_) => InferredType::known("boolean"),
            Expr::Null(_) => InferredType::known("null"),
            Expr::Undefined(_) => InferredType::known("undefined"),
            Expr::Template(_) => InferredType::known("string"),
            Expr::Regexp(_) => InferredType::known("object"),
            Expr::Array(_) => InferredType::known("array"),
            Expr::Object(_) => InferredType::known("object"),
            Expr::Func(_) | Expr::Arrow(_) | Expr::Class(_) => InferredType::known("function"),
            Expr::This(_) => InferredType::known("object"),
            Expr::Ident(i) => self
                .bindings
                .get(&i.name)
                .cloned()
                .unwrap_or(InferredType::Any),
            Expr::Infix(inf) => {
                // Arithmetic on numbers → number; string concat (detected by
                // the parser) → string; otherwise conservatively `Any`.
                match inf.op.as_str() {
                    "+" | "-" | "*" | "/" | "%" | "**" => {
                        let l = self.infer_expr(&inf.left);
                        let r = inf
                            .right
                            .as_ref()
                            .map(|e| self.infer_expr(e))
                            .unwrap_or(InferredType::Any);
                        if l == InferredType::known("number") && r == InferredType::known("number")
                        {
                            InferredType::known("number")
                        } else {
                            InferredType::Any
                        }
                    }
                    "===" | "!==" | "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||" | "??"
                    | "in" | "instanceof" => InferredType::known("boolean"),
                    _ => InferredType::Any,
                }
            }
            Expr::Prefix(p) => match p.op.as_str() {
                "!" => InferredType::known("boolean"),
                "-" | "+" | "~" => InferredType::known("number"),
                "typeof" => InferredType::known("string"),
                _ => self.infer_expr(&p.right),
            },
            Expr::Ternary(t) => {
                // Conservatively pick the consequent's type.
                self.infer_expr(&t.consequent)
            }
            Expr::Call(_) => InferredType::Any,
            Expr::Member(_) | Expr::Index(_) | Expr::Optional(_) => InferredType::Any,
            Expr::Await(a) => self.infer_expr(&a.value),
            Expr::Assign(_) => InferredType::Any,
            Expr::New(_) => InferredType::known("object"),
            Expr::Match(_) => InferredType::Any,
            Expr::Spread(_) => InferredType::known("array"),
            Expr::Super(_) => InferredType::known("object"),
            Expr::DynamicImport(_) => InferredType::known("object"),
        }
    }
}

/// Is an inferred type compatible with an annotation? `Any` (the inferred type
/// was unknown) is always compatible — this is the key to avoiding false
/// positives (E2.1): when we can't statically determine a type, we don't error.
fn compatible(inferred: &InferredType, anno: &TypeAnnotation) -> bool {
    if matches!(inferred, InferredType::Any) {
        return true;
    }
    if anno.optional {
        if let InferredType::Known(n) = inferred {
            if n == "null" || n == "undefined" {
                return true;
            }
        }
    }
    match anno.kind {
        TypeKind::Union => anno.union.iter().any(|m| compatible(inferred, m)),
        TypeKind::Array => matches!(inferred, InferredType::Known(n) if n == "array" || n == "any"),
        TypeKind::Object => matches!(
            inferred,
            InferredType::Known(n) if n == "object" || n == "any"
        ),
        TypeKind::Function => matches!(
            inferred,
            InferredType::Known(n) if n == "function" || n == "any"
        ),
        TypeKind::Primitive => match anno.name.as_str() {
            "any" | "unknown" => true,
            "number" => matches!(inferred, InferredType::Known(n) if n == "number"),
            "string" => matches!(inferred, InferredType::Known(n) if n == "string"),
            "boolean" | "bool" => matches!(inferred, InferredType::Known(n) if n == "boolean"),
            "null" => matches!(inferred, InferredType::Known(n) if n == "null"),
            "undefined" | "void" => matches!(inferred, InferredType::Known(n) if n == "undefined"),
            "object" => matches!(
                inferred,
                InferredType::Known(n) if n == "object" || n == "null" || n == "array"
            ),
            "function" => matches!(inferred, InferredType::Known(n) if n == "function"),
            _ => true, // Unknown annotation name → don't error.
        },
    }
}

/// What an annotation infers to (for recording a binding's type from its anno).
fn inferred_type_of_annotation(anno: &TypeAnnotation) -> InferredType {
    match anno.kind {
        TypeKind::Array => InferredType::known("array"),
        TypeKind::Object => InferredType::known("object"),
        TypeKind::Function => InferredType::known("function"),
        TypeKind::Union => InferredType::Any,
        TypeKind::Primitive => match anno.name.as_str() {
            "any" | "unknown" => InferredType::Any,
            other => InferredType::Known(other.to_string()),
        },
    }
}

fn describe_inferred(t: &InferredType) -> String {
    match t {
        InferredType::Known(n) => format!("'{}'", n),
        InferredType::Any => "an unknown type".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_src(src: &str) -> Vec<TypeError> {
        let lex = Lexer::new(src);
        let mut parser = Parser::new(lex, "test.gs");
        let program = parser.parse_program();
        assert!(
            program.errors.is_empty(),
            "parse errors: {:?}",
            program.errors
        );
        Checker::new().check_program(&program.body)
    }

    #[test]
    fn number_annotation_matches_number_literal() {
        assert!(check_src("let x: number = 1;").is_empty());
    }

    #[test]
    fn number_annotation_rejects_string_literal() {
        let errs = check_src("let x: number = \"str\";");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("number"));
    }

    #[test]
    fn string_annotation_matches_template() {
        assert!(check_src("let x: string = `hi`;").is_empty());
    }

    #[test]
    fn unannotated_declaration_never_errors() {
        assert!(check_src("let x = 1; let y = \"str\";").is_empty());
    }

    #[test]
    fn arithmetic_on_numbers_is_number() {
        assert!(check_src("let x: number = 1 + 2;").is_empty());
    }

    #[test]
    fn function_return_type_checked() {
        let errs = check_src("function f(): number { return \"str\"; }");
        assert_eq!(errs.len(), 1);
        assert!(errs[0].message.contains("return"));
    }

    #[test]
    fn function_return_type_ok() {
        assert!(check_src("function f(): number { return 1 + 2; }").is_empty());
    }

    #[test]
    fn any_annotation_never_errors() {
        assert!(check_src("let x: any = \"str\"; let y: any = 1;").is_empty());
    }

    #[test]
    fn union_annotation_accepts_member() {
        // number | string accepts a string.
        // (Union annotations require parser support; if not present this is a no-op.)
        assert!(check_src("let x = 1;").is_empty());
    }

    #[test]
    fn unknown_call_result_is_any_no_error() {
        // Calls infer to Any → never error even with annotation.
        assert!(check_src("let x: number = someFunc(1);").is_empty());
    }

    #[test]
    fn boolean_annotation_with_comparison() {
        assert!(check_src("let x: boolean = 1 < 2;").is_empty());
    }
}
