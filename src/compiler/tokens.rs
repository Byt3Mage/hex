#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum TokenType {
    // Literals
    Ident = 0,
    IntLit,
    FloatLit,
    StringLit,
    CharLit,

    // Single character symbols
    LeftParen,    // (
    RightParen,   // )
    LeftBrace,    // {
    RightBrace,   // }
    LeftBracket,  // [
    RightBracket, // ]
    Comma,        // ,
    Dot,          // .
    Semicolon,    // ;
    Colon,        // :
    At,           // @

    // One or two character symbols
    Question,            // ?
    QuestionDot,         // ?.
    QuestionQuestion,    // ??
    Bang,                // !
    BangEqual,           // !=
    Equal,               // =
    EqualEqual,          // ==
    Greater,             // >
    GreaterEqual,        // >=
    GreaterGreater,      // >>
    GreaterGreaterEqual, // >>=
    Less,                // <
    LessEqual,           // <=
    LessLess,            // <<
    LessLessEqual,       // <<=
    Plus,                // +
    PlusEqual,           // +=
    Minus,               // -
    MinusEqual,          // -=
    Arrow,               // ->
    Star,                // *
    StarEqual,           // *=
    Slash,               // /
    SlashEqual,          // /=
    Percent,             // %
    PercentEqual,        // %=
    Ampersand,           // &
    AmpersandEqual,      // &=
    Pipe,                // |
    PipeEqual,           // |=
    Caret,               // ^
    CaretEqual,          // ^=
    ColonColon,          // ::
    DotDot,              // ..
    DotDotEqual,         // ..=
    FatArrow,            // =>

    // Keywords
    And,
    Any,
    As,
    Bool,
    Break,
    Char,
    Comptime,
    Const,
    Continue,
    Cstr,
    Else,
    Enum,
    False,
    Float,
    Fn,
    For,
    If,
    Import,
    In,
    Int,
    Let,
    Loop,
    Macro,
    Match,
    Mod,
    Mut,
    Null,
    Or,
    Package,
    Pub,
    Return,
    SelfTy,
    Static,
    Str,
    Struct,
    Trait,
    True,
    Type,
    Union,
    Use,
    Void,
    While,

    // End of file
    Eof,
}

impl TokenType {
    pub const COUNT: usize = TokenType::Eof as usize + 1;
}

/// Macro for convenient token type matching
#[macro_export]
macro_rules! tt {
    (ident) => {
        $crate::compiler::tokens::TokenType::Ident
    };
    (int_lit) => {
        $crate::compiler::tokens::TokenType::IntLit
    };
    (float_lit) => {
        $crate::compiler::tokens::TokenType::FloatLit
    };
    (str_lit) => {
        $crate::compiler::tokens::TokenType::StringLit
    };
    (char_lit) => {
        $crate::compiler::tokens::TokenType::CharLit
    };

    // Single character
    ('(') => {
        $crate::compiler::tokens::TokenType::LeftParen
    };
    (')') => {
        $crate::compiler::tokens::TokenType::RightParen
    };
    ('{') => {
        $crate::compiler::tokens::TokenType::LeftBrace
    };
    ('}') => {
        $crate::compiler::tokens::TokenType::RightBrace
    };
    ('[') => {
        $crate::compiler::tokens::TokenType::LeftBracket
    };
    (']') => {
        $crate::compiler::tokens::TokenType::RightBracket
    };
    (,) => {
        $crate::compiler::tokens::TokenType::Comma
    };
    (.) => {
        $crate::compiler::tokens::TokenType::Dot
    };
    (;) => {
        $crate::compiler::tokens::TokenType::Semicolon
    };
    (:) => {
        $crate::compiler::tokens::TokenType::Colon
    };
    (@) => {
        $crate::compiler::tokens::TokenType::At
    };

    // One or two character
    (!) => {
        $crate::compiler::tokens::TokenType::Bang
    };
    (!=) => {
        $crate::compiler::tokens::TokenType::BangEqual
    };
    (=) => {
        $crate::compiler::tokens::TokenType::Equal
    };
    (==) => {
        $crate::compiler::tokens::TokenType::EqualEqual
    };
    (>) => {
        $crate::compiler::tokens::TokenType::Greater
    };
    (>=) => {
        $crate::compiler::tokens::TokenType::GreaterEqual
    };
    (>>) => {
        $crate::compiler::tokens::TokenType::GreaterGreater
    };
    (>>=) => {
        $crate::compiler::tokens::TokenType::GreaterGreaterEqual
    };
    (<) => {
        $crate::compiler::tokens::TokenType::Less
    };
    (<=) => {
        $crate::compiler::tokens::TokenType::LessEqual
    };
    (<<) => {
        $crate::compiler::tokens::TokenType::LessLess
    };
    (<<=) => {
        $crate::compiler::tokens::TokenType::LessLessEqual
    };
    (+) => {
        $crate::compiler::tokens::TokenType::Plus
    };
    (+=) => {
        $crate::compiler::tokens::TokenType::PlusEqual
    };
    (-) => {
        $crate::compiler::tokens::TokenType::Minus
    };
    (-=) => {
        $crate::compiler::tokens::TokenType::MinusEqual
    };
    (->) => {
        $crate::compiler::tokens::TokenType::Arrow
    };
    (*) => {
        $crate::compiler::tokens::TokenType::Star
    };
    (*=) => {
        $crate::compiler::tokens::TokenType::StarEqual
    };
    (/) => {
        $crate::compiler::tokens::TokenType::Slash
    };
    (/=) => {
        $crate::compiler::tokens::TokenType::SlashEqual
    };
    (%) => {
        $crate::compiler::tokens::TokenType::Percent
    };
    (%=) => {
        $crate::compiler::tokens::TokenType::PercentEqual
    };
    (&) => {
        $crate::compiler::tokens::TokenType::Ampersand
    };
    (&=) => {
        $crate::compiler::tokens::TokenType::AmpersandEqual
    };
    (|) => {
        $crate::compiler::tokens::TokenType::Pipe
    };
    (|=) => {
        $crate::compiler::tokens::TokenType::PipeEqual
    };
    (^) => {
        $crate::compiler::tokens::TokenType::Caret
    };
    (^=) => {
        $crate::compiler::tokens::TokenType::CaretEqual
    };
    (::) => {
        $crate::compiler::tokens::TokenType::ColonColon
    };
    (..) => {
        $crate::compiler::tokens::TokenType::DotDot
    };
    (..=) => {
        $crate::compiler::tokens::TokenType::DotDotEqual
    };
    (?) => {
        $crate::compiler::tokens::TokenType::Question
    };
    (?.) => {
        $crate::compiler::tokens::TokenType::QuestionDot
    };
    (??) => {
        $crate::compiler::tokens::TokenType::QuestionQuestion
    };
    (=>) => {
        $crate::compiler::tokens::TokenType::FatArrow
    };

    // Keywords
    (and) => {
        $crate::compiler::tokens::TokenType::And
    };
    (any) => {
        $crate::compiler::tokens::TokenType::Any
    };
    (as) => {
        $crate::compiler::tokens::TokenType::As
    };
    (bool) => {
        $crate::compiler::tokens::TokenType::Bool
    };
    (break) => {
        $crate::compiler::tokens::TokenType::Break
    };
    (char) => {
        $crate::compiler::tokens::TokenType::Char
    };
    (comptime) => {
        $crate::compiler::tokens::TokenType::Comptime
    };
    (const) => {
        $crate::compiler::tokens::TokenType::Const
    };
    (continue) => {
        $crate::compiler::tokens::TokenType::Continue
    };
    (cstr) => {
        $crate::compiler::tokens::TokenType::Cstr
    };
    (else) => {
        $crate::compiler::tokens::TokenType::Else
    };
    (enum) => {
        $crate::compiler::tokens::TokenType::Enum
    };
    (false) => {
        $crate::compiler::tokens::TokenType::False
    };
    (float) => {
        $crate::compiler::tokens::TokenType::Float
    };
    (fn) => {
        $crate::compiler::tokens::TokenType::Fn
    };
    (for) => {
        $crate::compiler::tokens::TokenType::For
    };
    (if) => {
        $crate::compiler::tokens::TokenType::If
    };
    (import) => {
        $crate::compiler::tokens::TokenType::Import
    };
    (in) => {
        $crate::compiler::tokens::TokenType::In
    };
    (int) => {
        $crate::compiler::tokens::TokenType::Int
    };
    (let) => {
        $crate::compiler::tokens::TokenType::Let
    };
    (loop) => {
        $crate::compiler::tokens::TokenType::Loop
    };
    (macro) => {
        $crate::compiler::tokens::TokenType::Macro
    };
    (match) => {
        $crate::compiler::tokens::TokenType::Match
    };
    (mod) => {
        $crate::compiler::tokens::TokenType::Mod
    };
    (mut) => {
        $crate::compiler::tokens::TokenType::Mut
    };
    (null) => {
        $crate::compiler::tokens::TokenType::Null
    };
    (or) => {
        $crate::compiler::tokens::TokenType::Or
    };
    (package) => {
        $crate::compiler::tokens::TokenType::Package
    };
    (pub) => {
        $crate::compiler::tokens::TokenType::Pub
    };
    (return) => {
        $crate::compiler::tokens::TokenType::Return
    };
    (self) => {
        $crate::compiler::tokens::TokenType::SelfTy
    };
    (static) => {
        $crate::compiler::tokens::TokenType::Static
    };
    (str) => {
        $crate::compiler::tokens::TokenType::Str
    };
    (struct) => {
        $crate::compiler::tokens::TokenType::Struct
    };
    (trait) => {
        $crate::compiler::tokens::TokenType::Trait
    };
    (true) => {
        $crate::compiler::tokens::TokenType::True
    };
    (type) => {
        $crate::compiler::tokens::TokenType::Type
    };
    (union) => {
        $crate::compiler::tokens::TokenType::Union
    };
    (use) => {
        $crate::compiler::tokens::TokenType::Use
    };
    (void) => {
        $crate::compiler::tokens::TokenType::Void
    };
    (while) => {
        $crate::compiler::tokens::TokenType::While
    };

    (eof) => {
        $crate::compiler::tokens::TokenType::Eof
    };
}

/// Represents a region of source code
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Byte offset of first character (0-based)
    pub start: usize,
    /// Byte offset after last character (exclusive)
    pub end: usize,
    /// Line number (1-based)
    pub line: usize,
    /// Column number (1-based, in UTF-8 chars)
    pub column: usize,
}

impl Span {
    /// Creates a new span from individual components
    pub fn new(start: usize, end: usize, line: usize, column: usize) -> Self {
        Self {
            start,
            end,
            line,
            column,
        }
    }

    /// Merges two spans into one that covers both
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line,
            column: self.column,
        }
    }

    /// Returns a zero-length span at a position
    pub fn at_pos(byte_pos: usize, line: usize, column: usize) -> Self {
        Self {
            start: byte_pos,
            end: byte_pos,
            line,
            column,
        }
    }

    /// Checks if this span contains a byte position
    pub fn contains(&self, pos: usize) -> bool {
        self.start <= pos && pos < self.end
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Token<'a> {
    pub(crate) ty: TokenType,
    pub(crate) lexeme: &'a str,
    pub(crate) span: Span,
}
