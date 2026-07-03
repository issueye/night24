//! Abstract Syntax Tree node definitions.
//!
//! The AST uses `Box<Expr>` / `Box<Stmt>` wrappers and `Vec` collections so
//! that nodes own their children. Positions are recorded on every node for
//! error reporting.

use std::fmt;
use std::rc::Rc;

/// A location in source code.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Position {
    pub file: Rc<str>,
    pub line: usize,
    pub col: usize,
    pub offset: usize,
}

impl Position {
    pub fn new(file: impl Into<String>, line: usize, col: usize, offset: usize) -> Position {
        Position {
            file: Rc::from(file.into().as_str()),
            line,
            col,
            offset,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.line == 0
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file: &str = if self.file.is_empty() {
            "<source>"
        } else {
            &self.file
        };
        write!(f, "{}:{}:{}", file, self.line, self.col)
    }
}

// ============================================================================
// Statements
// ============================================================================

/// The root of every AST.
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub pos: Position,
    pub body: Vec<Stmt>,
    pub errors: Vec<String>,
}

/// A top-level statement.
#[derive(Debug, Clone)]
pub enum Stmt {
    Let(LetStmt),
    Const(ConstStmt),
    Var(VarStmt),
    FuncDecl(FuncDecl),
    ClassDecl(ClassDecl),
    Block(BlockStmt),
    If(IfStmt),
    While(WhileStmt),
    For(Box<ForStmt>),
    ForIn(Box<ForInStmt>),
    ForOf(Box<ForOfStmt>),
    Return(ReturnStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Throw(ThrowStmt),
    Try(Box<TryStmt>),
    Expr(ExprStmt),
    Labeled(Box<LabeledStmt>),
    Import(ImportDecl),
    Export(ExportDecl),
}

impl Stmt {
    pub fn pos(&self) -> Position {
        match self {
            Stmt::Let(s) => s.pos.clone(),
            Stmt::Const(s) => s.pos.clone(),
            Stmt::Var(s) => s.pos.clone(),
            Stmt::FuncDecl(s) => s.pos.clone(),
            Stmt::ClassDecl(s) => s.pos.clone(),
            Stmt::Block(s) => s.pos.clone(),
            Stmt::If(s) => s.pos.clone(),
            Stmt::While(s) => s.pos.clone(),
            Stmt::For(s) => s.pos.clone(),
            Stmt::ForIn(s) => s.pos.clone(),
            Stmt::ForOf(s) => s.pos.clone(),
            Stmt::Return(s) => s.pos.clone(),
            Stmt::Break(s) => s.pos.clone(),
            Stmt::Continue(s) => s.pos.clone(),
            Stmt::Throw(s) => s.pos.clone(),
            Stmt::Try(s) => s.pos.clone(),
            Stmt::Expr(s) => s.pos.clone(),
            Stmt::Labeled(s) => s.pos.clone(),
            Stmt::Import(s) => s.pos.clone(),
            Stmt::Export(s) => s.pos.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LetStmt {
    pub pos: Position,
    pub name: String,
    /// Destructuring binding (let [a,b]=… / let {x}=…). When `Some`, the
    /// declaration destructures `value` into the pattern instead of binding a
    /// single `name`. `name` stays empty for destructuring declarations.
    pub binding: Option<BindingPattern>,
    pub type_anno: Option<TypeAnnotation>,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct ConstStmt {
    pub pos: Position,
    pub name: String,
    pub binding: Option<BindingPattern>,
    pub type_anno: Option<TypeAnnotation>,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct VarStmt {
    pub pos: Position,
    pub name: String,
    pub binding: Option<BindingPattern>,
    pub type_anno: Option<TypeAnnotation>,
    pub value: Option<Expr>,
}

/// A destructuring binding pattern (B3.2).
#[derive(Debug, Clone)]
pub enum BindingPattern {
    /// Array destructuring: `let [a, b] = arr`.
    /// Each element is `(name, default, is_rest)`. A hole (`[, b]`) has an
    /// empty name. Rest `...rest` captures the tail.
    Array(Vec<ArrayBindingElem>),
    /// Object destructuring: `let {x, y: z = d} = obj`.
    /// Each element binds `target` from property `key` (key==target if no
    /// rename), with an optional default.
    Object(Vec<ObjectBindingElem>),
}

#[derive(Debug, Clone)]
pub struct ArrayBindingElem {
    pub name: String,
    pub default: Option<Expr>,
    pub is_rest: bool,
}

#[derive(Debug, Clone)]
pub struct ObjectBindingElem {
    pub key: String,
    pub target: String,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub pos: Position,
    pub name: String,
    pub params: Vec<Param>,
    pub return_t: Option<TypeAnnotation>,
    pub body: BlockStmt,
    pub is_async: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub pos: Position,
    pub name: String,
    pub type_anno: Option<TypeAnnotation>,
    pub default: Option<Expr>,
    pub spread: bool,
    pub optional: bool,
}

#[derive(Debug, Clone, Default)]
pub struct BlockStmt {
    pub pos: Position,
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub pos: Position,
    pub cond: Expr,
    pub consequence: BlockStmt,
    pub alternative: Option<Box<Stmt>>,
}

#[derive(Debug, Clone)]
pub struct WhileStmt {
    pub pos: Position,
    pub cond: Expr,
    pub body: BlockStmt,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub pos: Position,
    pub init: Option<Box<Stmt>>,
    pub cond: Option<Expr>,
    pub post: Option<Expr>,
    pub body: BlockStmt,
}

#[derive(Debug, Clone)]
pub struct ForInStmt {
    pub pos: Position,
    pub name: String,
    pub iterable: Expr,
    pub body: BlockStmt,
}

#[derive(Debug, Clone)]
pub struct ForOfStmt {
    pub pos: Position,
    pub name: String,
    pub iterable: Expr,
    pub body: BlockStmt,
}

#[derive(Debug, Clone)]
pub struct ReturnStmt {
    pub pos: Position,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct BreakStmt {
    pub pos: Position,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct ContinueStmt {
    pub pos: Position,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct ThrowStmt {
    pub pos: Position,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct TryStmt {
    pub pos: Position,
    pub block: BlockStmt,
    pub catch: Option<CatchClause>,
    pub finalizer: Option<BlockStmt>,
}

#[derive(Debug, Clone)]
pub struct CatchClause {
    pub pos: Position,
    pub name: String,
    pub type_anno: Option<TypeAnnotation>,
    pub body: BlockStmt,
}

#[derive(Debug, Clone)]
pub struct ExprStmt {
    pub pos: Position,
    pub expr: Expr,
}

#[derive(Debug, Clone)]
pub struct LabeledStmt {
    pub pos: Position,
    pub label: String,
    pub stmt: Box<Stmt>,
}

// ============================================================================
// Expressions
// ============================================================================

/// A top-level expression.
#[derive(Debug, Clone)]
pub enum Expr {
    Ident(Ident),
    Number(NumberLit),
    String(StringLit),
    Template(TemplateLit),
    Regexp(RegExpLit),
    Bool(BoolLit),
    Null(NullLit),
    Undefined(UndefinedLit),
    This(ThisExpr),
    Super(SuperExpr),
    Array(ArrayLit),
    Object(ObjectLit),
    Prefix(Box<PrefixExpr>),
    Infix(Box<InfixExpr>),
    Ternary(Box<TernaryExpr>),
    Assign(Box<AssignExpr>),
    Call(Box<CallExpr>),
    Member(Box<MemberExpr>),
    Index(Box<IndexExpr>),
    Optional(Box<OptionalExpr>),
    Func(Box<FuncExpr>),
    Arrow(Box<ArrowFuncExpr>),
    New(Box<NewExpr>),
    Await(Box<AwaitExpr>),
    Spread(Box<SpreadExpr>),
    Match(Box<MatchExpr>),
    Class(Box<ClassDecl>),
    /// Dynamic `import(specifier)` — returns a Promise resolving to the
    /// module's namespace object (B2).
    DynamicImport(Box<DynamicImportExpr>),
}

#[derive(Debug, Clone)]
pub struct DynamicImportExpr {
    pub pos: Position,
    /// The module specifier expression (usually a string literal).
    pub source: Expr,
}

impl Expr {
    /// Wrap a class declaration as an expression node (used for class expressions).
    pub fn to_class_node(decl: ClassDecl) -> Expr {
        Expr::Class(Box::new(decl))
    }

    pub fn pos(&self) -> Position {
        match self {
            Expr::Ident(e) => e.pos.clone(),
            Expr::Number(e) => e.pos.clone(),
            Expr::String(e) => e.pos.clone(),
            Expr::Template(e) => e.pos.clone(),
            Expr::Regexp(e) => e.pos.clone(),
            Expr::Bool(e) => e.pos.clone(),
            Expr::Null(e) => e.pos.clone(),
            Expr::Undefined(e) => e.pos.clone(),
            Expr::This(e) => e.pos.clone(),
            Expr::Super(e) => e.pos.clone(),
            Expr::Array(e) => e.pos.clone(),
            Expr::Object(e) => e.pos.clone(),
            Expr::DynamicImport(e) => e.pos.clone(),
            Expr::Prefix(e) => e.pos.clone(),
            Expr::Infix(e) => e.pos.clone(),
            Expr::Ternary(e) => e.pos.clone(),
            Expr::Assign(e) => e.pos.clone(),
            Expr::Call(e) => e.pos.clone(),
            Expr::Member(e) => e.pos.clone(),
            Expr::Index(e) => e.pos.clone(),
            Expr::Optional(e) => e.pos.clone(),
            Expr::Func(e) => e.pos.clone(),
            Expr::Arrow(e) => e.pos.clone(),
            Expr::New(e) => e.pos.clone(),
            Expr::Await(e) => e.pos.clone(),
            Expr::Spread(e) => e.pos.clone(),
            Expr::Match(e) => e.pos.clone(),
            Expr::Class(e) => e.pos.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub pos: Position,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct NumberLit {
    pub pos: Position,
    pub literal: String,
    pub value: f64,
    pub is_int: bool,
}

#[derive(Debug, Clone)]
pub struct StringLit {
    pub pos: Position,
    /// The raw literal including quotes.
    pub literal: String,
}

#[derive(Debug, Clone)]
pub struct TemplateLit {
    pub pos: Position,
    /// The raw literal including backticks.
    pub literal: String,
}

#[derive(Debug, Clone)]
pub struct RegExpLit {
    pub pos: Position,
    /// The raw literal including slashes and flags.
    pub literal: String,
}

#[derive(Debug, Clone)]
pub struct BoolLit {
    pub pos: Position,
    pub value: bool,
}

#[derive(Debug, Clone)]
pub struct NullLit {
    pub pos: Position,
}

#[derive(Debug, Clone)]
pub struct UndefinedLit {
    pub pos: Position,
}

#[derive(Debug, Clone)]
pub struct ThisExpr {
    pub pos: Position,
}

#[derive(Debug, Clone)]
pub struct SuperExpr {
    pub pos: Position,
    pub method: String,
}

#[derive(Debug, Clone)]
pub struct ArrayLit {
    pub pos: Position,
    pub elements: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct ObjectLit {
    pub pos: Position,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone)]
pub struct Property {
    pub pos: Position,
    pub key: Expr,
    pub value: Expr,
    pub computed: bool,
    pub shorthand: bool,
    pub spread: bool,
    pub is_accessor: bool,
}

#[derive(Debug, Clone)]
pub struct PrefixExpr {
    pub pos: Position,
    pub op: String,
    pub right: Expr,
}

#[derive(Debug, Clone)]
pub struct InfixExpr {
    pub pos: Position,
    pub op: String,
    pub left: Expr,
    pub right: Option<Expr>,
}

#[derive(Debug, Clone)]
pub struct TernaryExpr {
    pub pos: Position,
    pub cond: Expr,
    pub consequent: Expr,
    pub alternate: Expr,
}

#[derive(Debug, Clone)]
pub struct AssignExpr {
    pub pos: Position,
    pub op: String,
    pub left: Expr,
    pub right: Expr,
}

#[derive(Debug, Clone)]
pub struct CallExpr {
    pub pos: Position,
    pub callee: Expr,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct MemberExpr {
    pub pos: Position,
    pub object: Expr,
    pub property: Expr,
    pub computed: bool,
}

#[derive(Debug, Clone)]
pub struct IndexExpr {
    pub pos: Position,
    pub left: Expr,
    pub index: Expr,
}

#[derive(Debug, Clone)]
pub struct OptionalExpr {
    pub pos: Position,
    pub object: Expr,
    pub property: Expr,
    pub computed: bool,
    pub is_call: bool,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct FuncExpr {
    pub pos: Position,
    pub name: String,
    pub params: Vec<Param>,
    pub return_t: Option<TypeAnnotation>,
    pub body: BlockStmt,
    pub is_async: bool,
}

#[derive(Debug, Clone)]
pub struct ArrowFuncExpr {
    pub pos: Position,
    pub params: Vec<Param>,
    pub return_t: Option<TypeAnnotation>,
    pub body: ArrowBody,
    pub is_async: bool,
}

/// An arrow function body is either an expression (implicit return) or a block.
#[derive(Debug, Clone)]
pub enum ArrowBody {
    Expr(Expr),
    Block(BlockStmt),
}

#[derive(Debug, Clone)]
pub struct NewExpr {
    pub pos: Position,
    pub callee: Expr,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct AwaitExpr {
    pub pos: Position,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct SpreadExpr {
    pub pos: Position,
    pub value: Expr,
}

// ============================================================================
// Match expressions
// ============================================================================

#[derive(Debug, Clone)]
pub struct MatchExpr {
    pub pos: Position,
    pub expr: Expr,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pos: Position,
    pub pattern: Pattern,
    pub binding_name: String,
    pub binding_pos: Position,
    pub guard: Option<Expr>,
    pub body: MatchBody,
}

/// A match arm body is either an expression or a block statement.
#[derive(Debug, Clone)]
pub enum MatchBody {
    Expr(Expr),
    Block(BlockStmt),
}

/// A match arm pattern.
#[derive(Debug, Clone)]
pub enum Pattern {
    Literal(Box<LiteralPattern>),
    Ident(IdentPattern),
    Wildcard(WildcardPattern),
    Or(OrPattern),
    Range(Box<RangePattern>),
}

#[derive(Debug, Clone)]
pub struct LiteralPattern {
    pub pos: Position,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct IdentPattern {
    pub pos: Position,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct WildcardPattern {
    pub pos: Position,
}

#[derive(Debug, Clone)]
pub struct OrPattern {
    pub pos: Position,
    pub alternatives: Vec<Pattern>,
}

#[derive(Debug, Clone)]
pub struct RangePattern {
    pub pos: Position,
    pub start: Expr,
    pub end: Expr,
    pub inclusive: bool,
}

// ============================================================================
// Classes
// ============================================================================

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub pos: Position,
    pub name: String,
    pub super_: Option<Expr>,
    pub body: ClassBody,
}

#[derive(Debug, Clone, Default)]
pub struct ClassBody {
    pub pos: Position,
    pub members: Vec<ClassMember>,
}

#[derive(Debug, Clone)]
pub struct ClassMember {
    pub pos: Position,
    pub is_static: bool,
    pub is_async: bool,
    pub name: String,
    pub params: Vec<Param>,
    pub body: Option<BlockStmt>,
    pub type_anno: Option<TypeAnnotation>,
    pub default_val: Option<Expr>,
    pub kind: ClassMemberKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassMemberKind {
    Method,
    Field,
    Constructor,
}

// ============================================================================
// Modules
// ============================================================================

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub pos: Position,
    pub default: String,
    pub namespace: String,
    pub names: Vec<String>,
    pub aliases: std::collections::HashMap<String, String>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ExportDecl {
    pub pos: Position,
    pub is_default: bool,
    /// `export * from "..."` — re-export ALL named exports from the source.
    /// Requires `from` to be set; `specifiers` is ignored.
    pub is_star: bool,
    pub decl: Option<Box<Stmt>>,
    pub specifiers: Vec<ExportSpec>,
    /// Source module for `export { a, b } from "./m"` re-exports. Empty for
    /// local re-exports (`export { a, b }`).
    pub from: String,
}

#[derive(Debug, Clone)]
pub struct ExportSpec {
    pub name: String,
    pub alias: String,
}

// ============================================================================
// Type annotations
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Primitive,
    Array,
    Union,
    Object,
    Function,
}

#[derive(Debug, Clone)]
pub struct TypeAnnotation {
    pub kind: TypeKind,
    pub name: String,
    pub array_of: Option<Box<TypeAnnotation>>,
    pub union: Vec<TypeAnnotation>,
    pub optional: bool,
}

impl Default for TypeAnnotation {
    fn default() -> Self {
        TypeAnnotation {
            kind: TypeKind::Primitive,
            name: String::new(),
            array_of: None,
            union: Vec::new(),
            optional: false,
        }
    }
}

impl fmt::Display for TypeAnnotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            TypeKind::Primitive => write!(f, "{}", self.name),
            TypeKind::Array => {
                if let Some(inner) = &self.array_of {
                    write!(f, "{}[]", inner)
                } else {
                    write!(f, "any[]")
                }
            }
            TypeKind::Union => {
                for (i, u) in self.union.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    write!(f, "{}", u)?;
                }
                Ok(())
            }
            TypeKind::Object => write!(f, "{{...}}"),
            TypeKind::Function => write!(f, "fn"),
        }
    }
}
