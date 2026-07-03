//! Lexical variable resolver for bytecode functions.
//!
//! This pass records the shape the VM will later lower to slot opcodes:
//! function-local slots, direct upvalues, forwarded parent upvalues, and
//! globals. It deliberately lives outside `compiler.rs` so the emitter can stay
//! focused on bytecode layout.

use std::collections::HashMap;

use crate::ast::{
    ArrowBody, BlockStmt, Expr, MatchBody, MatchExpr, Param, Pattern, Position, Program, Stmt,
};

use super::closure::{UpvalueDesc, UpvalueSource};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionKey {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub offset: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalBinding {
    pub name: String,
    pub slot: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameResolution {
    pub name: String,
    pub binding: ResolvedBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedBinding {
    LocalSlot(u16),
    Upvalue { index: u16, source: UpvalueSource },
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionResolution {
    pub key: FunctionKey,
    pub locals: Vec<LocalBinding>,
    pub references: Vec<NameResolution>,
    pub upvalues: Vec<UpvalueDesc>,
}

#[derive(Debug, Default)]
pub struct ResolutionMap {
    functions: HashMap<FunctionKey, FunctionResolution>,
    /// Names declared at the module top level (`let`/`const`/`var`/`fn`/`class`
    /// and import bindings). The root scope is `can_capture = false`, so any
    /// reference to these from a nested function is classified `Global` by the
    /// resolver — yet at runtime they live in the root environment's `bindings`,
    /// **not** `vm.globals`. Callers that want to lower a `Global` reference to
    /// `LoadGlobal` must skip names in this set to stay semantically correct.
    top_level_names: Vec<String>,
}

impl ResolutionMap {
    pub fn function(&self, name: &str, pos: &Position) -> Option<&FunctionResolution> {
        self.functions.get(&FunctionKey::new(name, pos))
    }

    /// True if `name` is a top-level declaration (and therefore stored in the
    /// root environment at runtime, not `vm.globals`). Used to decide whether a
    /// `Global`-classified reference is safe to lower to `LoadGlobal`.
    pub fn is_top_level_binding(&self, name: &str) -> bool {
        self.top_level_names.iter().any(|n| n == name)
    }

    #[cfg(test)]
    fn functions_named(&self, name: &str) -> Vec<&FunctionResolution> {
        self.functions
            .values()
            .filter(|resolution| resolution.key.name == name)
            .collect()
    }
}

pub fn resolve_program(program: &Program) -> ResolutionMap {
    let mut resolver = Resolver::new();
    resolver.enter_root(&program.body);
    for stmt in &program.body {
        resolver.scan_stmt(stmt);
    }
    resolver.finish_root()
}

struct Resolver {
    scopes: Vec<FunctionScope>,
    resolutions: HashMap<FunctionKey, FunctionResolution>,
}

#[derive(Default)]
struct FunctionScope {
    key: Option<FunctionKey>,
    can_capture: bool,
    locals: Vec<LocalBinding>,
    local_by_name: HashMap<String, u16>,
    upvalues: Vec<UpvalueDesc>,
    upvalue_by_name: HashMap<String, u16>,
    references: Vec<NameResolution>,
}

impl Resolver {
    fn new() -> Self {
        Resolver {
            scopes: Vec::new(),
            resolutions: HashMap::new(),
        }
    }

    fn enter_root(&mut self, statements: &[Stmt]) {
        self.scopes.push(FunctionScope::root());
        self.collect_local_declarations(statements);
    }

    fn finish_root(mut self) -> ResolutionMap {
        // The root scope carries the top-level binding names. Preserve them so
        // callers can tell "real globals" (builtins, in `vm.globals`) apart
        // from top-level declarations (in the root environment's `bindings`).
        let top_level_names = if let Some(root) = self.scopes.pop() {
            root.locals.into_iter().map(|b| b.name).collect()
        } else {
            Vec::new()
        };
        ResolutionMap {
            functions: self.resolutions,
            top_level_names,
        }
    }

    fn enter_function(&mut self, name: &str, params: &[Param], body: &BlockStmt, pos: &Position) {
        let key = FunctionKey::new(name, pos);
        self.scopes.push(FunctionScope::function(key));
        if !name.is_empty() {
            self.declare_current(name);
        }
        for param in params {
            self.declare_current(&param.name);
        }
        self.collect_local_declarations(&body.statements);
        for param in params {
            if let Some(default) = &param.default {
                self.scan_expr(default);
            }
        }
        for stmt in &body.statements {
            self.scan_stmt(stmt);
        }
        let scope = self.scopes.pop().expect("function scope");
        let key = scope.key.clone().expect("function key");
        self.resolutions.insert(
            key.clone(),
            FunctionResolution {
                key,
                locals: scope.locals,
                references: scope.references,
                upvalues: scope.upvalues,
            },
        );
    }

    fn collect_local_declarations(&mut self, statements: &[Stmt]) {
        for stmt in statements {
            match stmt {
                Stmt::Let(s) => {
                    self.declare_current(&s.name);
                }
                Stmt::Const(s) => {
                    self.declare_current(&s.name);
                }
                Stmt::Var(s) => {
                    self.declare_current(&s.name);
                }
                Stmt::FuncDecl(s) => {
                    self.declare_current(&s.name);
                }
                Stmt::ClassDecl(s) => {
                    self.declare_current(&s.name);
                }
                Stmt::Block(b) => self.collect_local_declarations(&b.statements),
                Stmt::If(s) => {
                    self.collect_local_declarations(&s.consequence.statements);
                    if let Some(alternative) = &s.alternative {
                        self.collect_local_declarations(std::slice::from_ref(alternative));
                    }
                }
                Stmt::While(s) => self.collect_local_declarations(&s.body.statements),
                Stmt::For(s) => {
                    if let Some(init) = &s.init {
                        self.collect_local_declarations(std::slice::from_ref(init));
                    }
                    self.collect_local_declarations(&s.body.statements);
                }
                Stmt::ForIn(s) => {
                    self.declare_current(&s.name);
                    self.collect_local_declarations(&s.body.statements);
                }
                Stmt::ForOf(s) => {
                    self.declare_current(&s.name);
                    self.collect_local_declarations(&s.body.statements);
                }
                Stmt::Try(s) => {
                    self.collect_local_declarations(&s.block.statements);
                    if let Some(catch) = &s.catch {
                        if !catch.name.is_empty() {
                            self.declare_current(&catch.name);
                        }
                        self.collect_local_declarations(&catch.body.statements);
                    }
                    if let Some(finalizer) = &s.finalizer {
                        self.collect_local_declarations(&finalizer.statements);
                    }
                }
                Stmt::Export(s) => {
                    if let Some(decl) = &s.decl {
                        self.collect_local_declarations(std::slice::from_ref(decl));
                    }
                }
                Stmt::Import(s) => {
                    if !s.default.is_empty() {
                        self.declare_current(&s.default);
                    }
                    if !s.namespace.is_empty() {
                        self.declare_current(&s.namespace);
                    }
                    for alias in s.aliases.values() {
                        self.declare_current(alias);
                    }
                    for name in &s.names {
                        self.declare_current(name);
                    }
                }
                Stmt::Return(_)
                | Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::Throw(_)
                | Stmt::Expr(_)
                | Stmt::Labeled(_) => {}
            }
        }
    }

    fn declare_current(&mut self, name: &str) -> u16 {
        let current = self.scopes.last_mut().expect("active scope");
        current.declare(name)
    }

    fn scan_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let(s) => self.scan_optional_expr(s.value.as_ref()),
            Stmt::Const(s) => self.scan_optional_expr(s.value.as_ref()),
            Stmt::Var(s) => self.scan_optional_expr(s.value.as_ref()),
            Stmt::FuncDecl(s) => {
                self.enter_function(&s.name, &s.params, &s.body, &s.pos);
            }
            Stmt::ClassDecl(s) => self.scan_class(s),
            Stmt::Block(b) => {
                for stmt in &b.statements {
                    self.scan_stmt(stmt);
                }
            }
            Stmt::If(s) => {
                self.scan_expr(&s.cond);
                for stmt in &s.consequence.statements {
                    self.scan_stmt(stmt);
                }
                if let Some(alternative) = &s.alternative {
                    self.scan_stmt(alternative);
                }
            }
            Stmt::While(s) => {
                self.scan_expr(&s.cond);
                for stmt in &s.body.statements {
                    self.scan_stmt(stmt);
                }
            }
            Stmt::For(s) => {
                if let Some(init) = &s.init {
                    self.scan_stmt(init);
                }
                self.scan_optional_expr(s.cond.as_ref());
                self.scan_optional_expr(s.post.as_ref());
                for stmt in &s.body.statements {
                    self.scan_stmt(stmt);
                }
            }
            Stmt::ForIn(s) => {
                self.scan_expr(&s.iterable);
                for stmt in &s.body.statements {
                    self.scan_stmt(stmt);
                }
            }
            Stmt::ForOf(s) => {
                self.scan_expr(&s.iterable);
                for stmt in &s.body.statements {
                    self.scan_stmt(stmt);
                }
            }
            Stmt::Return(s) => self.scan_optional_expr(s.value.as_ref()),
            Stmt::Throw(s) => self.scan_expr(&s.value),
            Stmt::Try(s) => {
                for stmt in &s.block.statements {
                    self.scan_stmt(stmt);
                }
                if let Some(catch) = &s.catch {
                    for stmt in &catch.body.statements {
                        self.scan_stmt(stmt);
                    }
                }
                if let Some(finalizer) = &s.finalizer {
                    for stmt in &finalizer.statements {
                        self.scan_stmt(stmt);
                    }
                }
            }
            Stmt::Expr(s) => self.scan_expr(&s.expr),
            Stmt::Labeled(s) => self.scan_stmt(&s.stmt),
            Stmt::Export(s) => {
                if let Some(decl) = &s.decl {
                    self.scan_stmt(decl);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Import(_) => {}
        }
    }

    fn scan_optional_expr(&mut self, expr: Option<&Expr>) {
        if let Some(expr) = expr {
            self.scan_expr(expr);
        }
    }

    fn scan_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(i) => self.resolve_reference(&i.name),
            Expr::DynamicImport(d) => self.scan_expr(&d.source),
            Expr::Array(a) => {
                for element in &a.elements {
                    self.scan_expr(element);
                }
            }
            Expr::Object(o) => {
                for prop in &o.properties {
                    if prop.computed {
                        self.scan_expr(&prop.key);
                    }
                    if prop.spread {
                        self.scan_expr(&prop.key);
                    }
                    self.scan_expr(&prop.value);
                }
            }
            Expr::Prefix(e) => self.scan_expr(&e.right),
            Expr::Infix(e) => {
                self.scan_expr(&e.left);
                self.scan_optional_expr(e.right.as_ref());
            }
            Expr::Ternary(e) => {
                self.scan_expr(&e.cond);
                self.scan_expr(&e.consequent);
                self.scan_expr(&e.alternate);
            }
            Expr::Assign(e) => {
                if let Expr::Ident(i) = &e.left {
                    self.resolve_reference(&i.name);
                } else {
                    self.scan_expr(&e.left);
                }
                self.scan_expr(&e.right);
            }
            Expr::Call(e) => {
                self.scan_expr(&e.callee);
                for arg in &e.args {
                    self.scan_expr(arg);
                }
            }
            Expr::Member(e) => {
                self.scan_expr(&e.object);
                if e.computed {
                    self.scan_expr(&e.property);
                }
            }
            Expr::Index(e) => {
                self.scan_expr(&e.left);
                self.scan_expr(&e.index);
            }
            Expr::Optional(e) => {
                self.scan_expr(&e.object);
                if e.computed {
                    self.scan_expr(&e.property);
                }
                for arg in &e.args {
                    self.scan_expr(arg);
                }
            }
            Expr::Func(e) => self.enter_function(&e.name, &e.params, &e.body, &e.pos),
            Expr::Arrow(e) => {
                let body = match &e.body {
                    ArrowBody::Expr(expr) => BlockStmt {
                        pos: e.pos.clone(),
                        statements: vec![Stmt::Return(crate::ast::ReturnStmt {
                            pos: e.pos.clone(),
                            value: Some(expr.clone()),
                        })],
                    },
                    ArrowBody::Block(block) => block.clone(),
                };
                self.enter_function("", &e.params, &body, &e.pos);
            }
            Expr::New(e) => {
                self.scan_expr(&e.callee);
                for arg in &e.args {
                    self.scan_expr(arg);
                }
            }
            Expr::Await(e) => self.scan_expr(&e.value),
            Expr::Spread(e) => self.scan_expr(&e.value),
            Expr::Match(e) => self.scan_match(e),
            Expr::Class(e) => self.scan_class(e),
            Expr::Number(_)
            | Expr::String(_)
            | Expr::Template(_)
            | Expr::Regexp(_)
            | Expr::Bool(_)
            | Expr::Null(_)
            | Expr::Undefined(_)
            | Expr::This(_)
            | Expr::Super(_) => {}
        }
    }

    fn scan_match(&mut self, m: &MatchExpr) {
        self.scan_expr(&m.expr);
        for arm in &m.arms {
            scan_pattern_exprs(self, &arm.pattern);
            if !arm.binding_name.is_empty() {
                self.declare_current(&arm.binding_name);
            }
            self.scan_optional_expr(arm.guard.as_ref());
            match &arm.body {
                MatchBody::Expr(expr) => self.scan_expr(expr),
                MatchBody::Block(block) => {
                    for stmt in &block.statements {
                        self.scan_stmt(stmt);
                    }
                }
            }
        }
    }

    fn scan_class(&mut self, class: &crate::ast::ClassDecl) {
        self.scan_optional_expr(class.super_.as_ref());
        for member in &class.body.members {
            self.scan_optional_expr(member.default_val.as_ref());
            if let Some(body) = &member.body {
                self.enter_function(&member.name, &member.params, body, &member.pos);
            }
        }
    }

    fn resolve_reference(&mut self, name: &str) {
        let current_idx = self.scopes.len() - 1;
        let binding = if let Some(slot) = self.scopes[current_idx].local_slot(name) {
            ResolvedBinding::LocalSlot(slot)
        } else if let Some((index, source)) = self.resolve_upvalue(current_idx, name) {
            ResolvedBinding::Upvalue { index, source }
        } else {
            ResolvedBinding::Global
        };
        self.scopes[current_idx].references.push(NameResolution {
            name: name.to_string(),
            binding,
        });
    }

    fn resolve_upvalue(&mut self, function_idx: usize, name: &str) -> Option<(u16, UpvalueSource)> {
        if function_idx == 0 {
            return None;
        }
        let parent_idx = function_idx - 1;
        if !self.scopes[parent_idx].can_capture {
            return None;
        }
        if let Some(slot) = self.scopes[parent_idx].local_slot(name) {
            let source = UpvalueSource::LocalSlot(slot);
            let index = self.add_upvalue(function_idx, name, source);
            return Some((index, source));
        }
        let parent = self.resolve_upvalue(parent_idx, name)?;
        let source = UpvalueSource::ParentUpvalue(parent.0);
        let index = self.add_upvalue(function_idx, name, source);
        Some((index, source))
    }

    fn add_upvalue(&mut self, scope_idx: usize, name: &str, source: UpvalueSource) -> u16 {
        if let Some(index) = self.scopes[scope_idx].upvalue_by_name.get(name) {
            return *index;
        }
        let index = self.scopes[scope_idx].upvalues.len() as u16;
        self.scopes[scope_idx].upvalues.push(UpvalueDesc {
            name: name.to_string(),
            source,
        });
        self.scopes[scope_idx]
            .upvalue_by_name
            .insert(name.to_string(), index);
        index
    }
}

impl FunctionKey {
    pub fn new(name: &str, pos: &Position) -> Self {
        FunctionKey {
            file: pos.file.to_string(),
            line: pos.line,
            col: pos.col,
            offset: pos.offset,
            name: name.to_string(),
        }
    }
}

impl FunctionScope {
    fn root() -> Self {
        FunctionScope {
            can_capture: false,
            ..FunctionScope::default()
        }
    }

    fn function(key: FunctionKey) -> Self {
        FunctionScope {
            key: Some(key),
            can_capture: true,
            ..FunctionScope::default()
        }
    }

    fn declare(&mut self, name: &str) -> u16 {
        if let Some(slot) = self.local_by_name.get(name) {
            return *slot;
        }
        let slot = self.locals.len() as u16;
        self.locals.push(LocalBinding {
            name: name.to_string(),
            slot,
        });
        self.local_by_name.insert(name.to_string(), slot);
        slot
    }

    fn local_slot(&self, name: &str) -> Option<u16> {
        self.local_by_name.get(name).copied()
    }
}

fn scan_pattern_exprs(resolver: &mut Resolver, pattern: &Pattern) {
    match pattern {
        Pattern::Literal(p) => resolver.scan_expr(&p.value),
        Pattern::Or(p) => {
            for alternative in &p.alternatives {
                scan_pattern_exprs(resolver, alternative);
            }
        }
        Pattern::Range(p) => {
            resolver.scan_expr(&p.start);
            resolver.scan_expr(&p.end);
        }
        Pattern::Ident(_) | Pattern::Wildcard(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn resolve_src(src: &str) -> ResolutionMap {
        let lexer = Lexer::new(src);
        let mut parser = Parser::new(lexer, "resolve.gs");
        let program = parser.parse_program();
        assert!(
            program.errors.is_empty(),
            "parse errors: {:?}",
            program.errors
        );
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        resolve_program(&program)
    }

    fn one_named<'a>(map: &'a ResolutionMap, name: &str) -> &'a FunctionResolution {
        let matches = map.functions_named(name);
        assert_eq!(matches.len(), 1, "expected one function named {name}");
        matches[0]
    }

    #[test]
    fn resolves_locals_and_globals() {
        let map = resolve_src("function f(a) { let b = a; return a + b + c; }");
        let f = one_named(&map, "f");

        assert_eq!(f.locals[0].name, "f");
        assert_eq!(f.locals[1].name, "a");
        assert_eq!(f.locals[2].name, "b");
        assert!(f
            .references
            .iter()
            .any(|r| { r.name == "a" && matches!(r.binding, ResolvedBinding::LocalSlot(1)) }));
        assert!(f
            .references
            .iter()
            .any(|r| { r.name == "b" && matches!(r.binding, ResolvedBinding::LocalSlot(2)) }));
        assert!(f
            .references
            .iter()
            .any(|r| r.name == "c" && matches!(r.binding, ResolvedBinding::Global)));
    }

    #[test]
    fn records_direct_upvalue_from_parent_local() {
        let map = resolve_src("function outer() { let x = 1; function inner() { return x; } }");
        let inner = one_named(&map, "inner");

        assert_eq!(inner.upvalues.len(), 1);
        assert_eq!(inner.upvalues[0].name, "x");
        assert_eq!(inner.upvalues[0].source, UpvalueSource::LocalSlot(1));
        assert!(inner.references.iter().any(|r| {
            r.name == "x"
                && matches!(
                    r.binding,
                    ResolvedBinding::Upvalue {
                        index: 0,
                        source: UpvalueSource::LocalSlot(1)
                    }
                )
        }));
    }

    /// Documents the resolver's stance on top-level `let` bindings referenced
    /// from a nested function: because the root scope is `can_capture = false`,
    /// such a reference is **not** modelled as an upvalue — it falls through to
    /// `ResolvedBinding::Global`.
    ///
    /// NOTE: this diverges from the *current* runtime, where a top-level `let`
    /// lives in the root environment's `bindings` (not `vm.globals`). Until that
    /// storage is migrated to `vm.globals`, the compiler cannot lower these
    /// references to `LoadGlobal` — doing so would read `vm.get_global` and miss.
    /// This test pins the resolver behaviour so the migration has a target.
    #[test]
    fn top_level_let_referenced_from_function_is_global_not_upvalue() {
        let map = resolve_src("let x = 1; function f() { return x; }");
        let f = one_named(&map, "f");

        assert_eq!(f.upvalues.len(), 0, "top-level let must not be captured");
        assert!(f
            .references
            .iter()
            .any(|r| { r.name == "x" && matches!(r.binding, ResolvedBinding::Global) }));
    }

    #[test]
    fn records_forwarded_upvalue_through_intermediate_function() {
        let map = resolve_src(
            "function outer() { let x = 1; function mid() { function inner() { return x; } } }",
        );
        let mid = one_named(&map, "mid");
        let inner = one_named(&map, "inner");

        assert_eq!(mid.upvalues.len(), 1);
        assert_eq!(mid.upvalues[0].source, UpvalueSource::LocalSlot(1));
        assert_eq!(inner.upvalues.len(), 1);
        assert_eq!(inner.upvalues[0].source, UpvalueSource::ParentUpvalue(0));
    }
}
