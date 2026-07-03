//! Lexical tokens for the GoScript language.

use std::fmt;

/// A lexical token with position information.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub literal: String,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

impl Token {
    pub fn new(
        kind: TokenKind,
        literal: impl Into<String>,
        line: usize,
        column: usize,
        offset: usize,
    ) -> Token {
        Token {
            kind,
            literal: literal.into(),
            line,
            column,
            offset,
        }
    }
}

/// The category of each lexeme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // --- Literals ---
    Ident,
    Number,
    String,
    Template,
    Regexp,

    // --- Keywords ---
    Let,
    Const,
    Var,
    Function,
    Class,
    Extends,
    If,
    Else,
    While,
    For,
    In,
    Of,
    Return,
    Break,
    Continue,
    True,
    False,
    Null,
    Undefined,
    New,
    This,
    Super,
    Try,
    Catch,
    Finally,
    Throw,
    Async,
    Await,
    Import,
    Export,
    From,
    As,
    Delete,
    Typeof,
    Instanceof,
    Void,
    Static,
    Match,
    Default, // (reserved)

    // --- Single-character operators ---
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Question,
    Colon,
    Dot,

    // --- Multi-character operators ---
    PlusPlus,
    MinusMinus,
    Pow,
    Eq,
    EqEqEq,
    NeqEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AndAnd,
    OrOr,
    QmQm,
    Arrow,
    Ellipsis,
    DotDot,
    DotDotEq,
    QmDot,

    // --- Compound assignment ---
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    PowEq,
    LShift,
    RShift,
    UrShift,
    LShiftEq,
    RShiftEq,
    UrShiftEq,
    AmpEq,
    PipeEq,
    CaretEq,

    // --- Delimiters ---
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    Comma,
    Semi,

    // --- Special ---
    Eof,
    Illegal,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Look up an identifier, returning the keyword kind if it is reserved.
pub fn lookup_ident(ident: &str) -> TokenKind {
    match ident {
        "let" => TokenKind::Let,
        "const" => TokenKind::Const,
        "var" => TokenKind::Var,
        "function" => TokenKind::Function,
        "class" => TokenKind::Class,
        "extends" => TokenKind::Extends,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "of" => TokenKind::Of,
        "return" => TokenKind::Return,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "null" => TokenKind::Null,
        "undefined" => TokenKind::Undefined,
        "new" => TokenKind::New,
        "this" => TokenKind::This,
        "super" => TokenKind::Super,
        "try" => TokenKind::Try,
        "catch" => TokenKind::Catch,
        "finally" => TokenKind::Finally,
        "throw" => TokenKind::Throw,
        "async" => TokenKind::Async,
        "await" => TokenKind::Await,
        "import" => TokenKind::Import,
        "export" => TokenKind::Export,
        "from" => TokenKind::From,
        "as" => TokenKind::As,
        "delete" => TokenKind::Delete,
        "typeof" => TokenKind::Typeof,
        "instanceof" => TokenKind::Instanceof,
        "void" => TokenKind::Void,
        "static" => TokenKind::Static,
        "match" => TokenKind::Match,
        "default" => TokenKind::Default,
        _ => TokenKind::Ident,
    }
}

/// Whether the token kind is a reserved keyword.
pub fn is_keyword(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Let
            | TokenKind::Const
            | TokenKind::Var
            | TokenKind::Function
            | TokenKind::Class
            | TokenKind::Extends
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::While
            | TokenKind::For
            | TokenKind::In
            | TokenKind::Of
            | TokenKind::Return
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
            | TokenKind::Undefined
            | TokenKind::New
            | TokenKind::This
            | TokenKind::Super
            | TokenKind::Try
            | TokenKind::Catch
            | TokenKind::Finally
            | TokenKind::Throw
            | TokenKind::Async
            | TokenKind::Await
            | TokenKind::Import
            | TokenKind::Export
            | TokenKind::From
            | TokenKind::As
            | TokenKind::Delete
            | TokenKind::Typeof
            | TokenKind::Instanceof
            | TokenKind::Void
            | TokenKind::Static
            | TokenKind::Match
            | TokenKind::Default
    )
}
