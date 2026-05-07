#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TokenType {
    // Literals
    Ident = 0,
    CintLit,
    UintLit,
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
    QuestionColon,       // ?:
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
    CInt,
    CStr,
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
    Uint,
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
        $crate::compiler::token::TokenType::Ident
    };
    (cint_lit) => {
        $crate::compiler::token::TokenType::CintLit
    };
    (uint_lit) => {
        $crate::compiler::token::TokenType::UintLit
    };
    (int_lit) => {
        $crate::compiler::token::TokenType::IntLit
    };
    (float_lit) => {
        $crate::compiler::token::TokenType::FloatLit
    };
    (str_lit) => {
        $crate::compiler::token::TokenType::StringLit
    };
    (char_lit) => {
        $crate::compiler::token::TokenType::CharLit
    };

    // Single character
    ('(') => {
        $crate::compiler::token::TokenType::LeftParen
    };
    (')') => {
        $crate::compiler::token::TokenType::RightParen
    };
    ('{') => {
        $crate::compiler::token::TokenType::LeftBrace
    };
    ('}') => {
        $crate::compiler::token::TokenType::RightBrace
    };
    ('[') => {
        $crate::compiler::token::TokenType::LeftBracket
    };
    (']') => {
        $crate::compiler::token::TokenType::RightBracket
    };
    (,) => {
        $crate::compiler::token::TokenType::Comma
    };
    (.) => {
        $crate::compiler::token::TokenType::Dot
    };
    (;) => {
        $crate::compiler::token::TokenType::Semicolon
    };
    (:) => {
        $crate::compiler::token::TokenType::Colon
    };
    (@) => {
        $crate::compiler::token::TokenType::At
    };

    // One or two character
    (!) => {
        $crate::compiler::token::TokenType::Bang
    };
    (!=) => {
        $crate::compiler::token::TokenType::BangEqual
    };
    (=) => {
        $crate::compiler::token::TokenType::Equal
    };
    (==) => {
        $crate::compiler::token::TokenType::EqualEqual
    };
    (>) => {
        $crate::compiler::token::TokenType::Greater
    };
    (>=) => {
        $crate::compiler::token::TokenType::GreaterEqual
    };
    (>>) => {
        $crate::compiler::token::TokenType::GreaterGreater
    };
    (>>=) => {
        $crate::compiler::token::TokenType::GreaterGreaterEqual
    };
    (<) => {
        $crate::compiler::token::TokenType::Less
    };
    (<=) => {
        $crate::compiler::token::TokenType::LessEqual
    };
    (<<) => {
        $crate::compiler::token::TokenType::LessLess
    };
    (<<=) => {
        $crate::compiler::token::TokenType::LessLessEqual
    };
    (+) => {
        $crate::compiler::token::TokenType::Plus
    };
    (+=) => {
        $crate::compiler::token::TokenType::PlusEqual
    };
    (-) => {
        $crate::compiler::token::TokenType::Minus
    };
    (-=) => {
        $crate::compiler::token::TokenType::MinusEqual
    };
    (->) => {
        $crate::compiler::token::TokenType::Arrow
    };
    (*) => {
        $crate::compiler::token::TokenType::Star
    };
    (*=) => {
        $crate::compiler::token::TokenType::StarEqual
    };
    (/) => {
        $crate::compiler::token::TokenType::Slash
    };
    (/=) => {
        $crate::compiler::token::TokenType::SlashEqual
    };
    (%) => {
        $crate::compiler::token::TokenType::Percent
    };
    (%=) => {
        $crate::compiler::token::TokenType::PercentEqual
    };
    (&) => {
        $crate::compiler::token::TokenType::Ampersand
    };
    (&=) => {
        $crate::compiler::token::TokenType::AmpersandEqual
    };
    (|) => {
        $crate::compiler::token::TokenType::Pipe
    };
    (|=) => {
        $crate::compiler::token::TokenType::PipeEqual
    };
    (^) => {
        $crate::compiler::token::TokenType::Caret
    };
    (^=) => {
        $crate::compiler::token::TokenType::CaretEqual
    };
    (::) => {
        $crate::compiler::token::TokenType::ColonColon
    };
    (..) => {
        $crate::compiler::token::TokenType::DotDot
    };
    (..=) => {
        $crate::compiler::token::TokenType::DotDotEqual
    };
    (?) => {
        $crate::compiler::token::TokenType::Question
    };
    (?.) => {
        $crate::compiler::token::TokenType::QuestionDot
    };
    (?:) => {
        $crate::compiler::token::TokenType::QuestionColon
    };
    (=>) => {
        $crate::compiler::token::TokenType::FatArrow
    };

    // Keywords
    (and) => {
        $crate::compiler::token::TokenType::And
    };
    (any) => {
        $crate::compiler::token::TokenType::Any
    };
    (as) => {
        $crate::compiler::token::TokenType::As
    };
    (bool) => {
        $crate::compiler::token::TokenType::Bool
    };
    (break) => {
        $crate::compiler::token::TokenType::Break
    };
    (char) => {
        $crate::compiler::token::TokenType::Char
    };
    (comptime) => {
        $crate::compiler::token::TokenType::Comptime
    };
    (const) => {
        $crate::compiler::token::TokenType::Const
    };
    (continue) => {
        $crate::compiler::token::TokenType::Continue
    };
    (cint) => {
        $crate::compiler::token::TokenType::CInt
    };
    (cstr) => {
        $crate::compiler::token::TokenType::CStr
    };
    (else) => {
        $crate::compiler::token::TokenType::Else
    };
    (enum) => {
        $crate::compiler::token::TokenType::Enum
    };
    (false) => {
        $crate::compiler::token::TokenType::False
    };
    (float) => {
        $crate::compiler::token::TokenType::Float
    };
    (fn) => {
        $crate::compiler::token::TokenType::Fn
    };
    (for) => {
        $crate::compiler::token::TokenType::For
    };
    (if) => {
        $crate::compiler::token::TokenType::If
    };
    (import) => {
        $crate::compiler::token::TokenType::Import
    };
    (in) => {
        $crate::compiler::token::TokenType::In
    };
    (int) => {
        $crate::compiler::token::TokenType::Int
    };
    (let) => {
        $crate::compiler::token::TokenType::Let
    };
    (loop) => {
        $crate::compiler::token::TokenType::Loop
    };
    (macro) => {
        $crate::compiler::token::TokenType::Macro
    };
    (match) => {
        $crate::compiler::token::TokenType::Match
    };
    (mod) => {
        $crate::compiler::token::TokenType::Mod
    };
    (mut) => {
        $crate::compiler::token::TokenType::Mut
    };
    (null) => {
        $crate::compiler::token::TokenType::Null
    };
    (or) => {
        $crate::compiler::token::TokenType::Or
    };
    (package) => {
        $crate::compiler::token::TokenType::Package
    };
    (pub) => {
        $crate::compiler::token::TokenType::Pub
    };
    (return) => {
        $crate::compiler::token::TokenType::Return
    };
    (self) => {
        $crate::compiler::token::TokenType::SelfTy
    };
    (static) => {
        $crate::compiler::token::TokenType::Static
    };
    (str) => {
        $crate::compiler::token::TokenType::Str
    };
    (struct) => {
        $crate::compiler::token::TokenType::Struct
    };
    (trait) => {
        $crate::compiler::token::TokenType::Trait
    };
    (true) => {
        $crate::compiler::token::TokenType::True
    };
    (type) => {
        $crate::compiler::token::TokenType::Type
    };
    (uint) => {
        $crate::compiler::token::TokenType::Uint
    };
    (union) => {
        $crate::compiler::token::TokenType::Union
    };
    (use) => {
        $crate::compiler::token::TokenType::Use
    };
    (void) => {
        $crate::compiler::token::TokenType::Void
    };
    (while) => {
        $crate::compiler::token::TokenType::While
    };

    (eof) => {
        $crate::compiler::token::TokenType::Eof
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
pub struct Token<'a> {
    pub ty: TokenType,
    pub lexeme: &'a str,
    pub span: Span,
}
