use super::tokens::TokenType;
use crate::tt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixRule {
    None,
    LiteralInt,
    LiteralFloat,
    LiteralString,
    True,
    False,
    LiteralNull,
    LiteralVoid,
    LiteralArray,
    LiteralStruct,
    Identifier,
    Grouping,
    Unary,
    If,
    Block,
    While,
    Loop,
    Match,
    Return,
    Break,
    Continue,
    ModuleType,
    StructType,
    UnionType,
    EnumType,
    PointerType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfixRule {
    None,
    Binary,
    Call,
    Dot,
    Index,
    Assign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Precedence {
    None = 0,
    Assignment, // =, += , -=, *=, /=, %=
    Or,         // or
    And,        // and
    Equality,   // ==, !=
    Comparison, // <, >, <=, >=
    Term,       // +, -
    Factor,     // *, /, %
    Unary,      // !, -
    Call,       // ., ()
    Primary,
}

impl Precedence {
    pub(crate) fn next(&self) -> Self {
        match self {
            Precedence::None => Precedence::Assignment,
            Precedence::Assignment => Precedence::Or,
            Precedence::Or => Precedence::And,
            Precedence::And => Precedence::Equality,
            Precedence::Equality => Precedence::Comparison,
            Precedence::Comparison => Precedence::Term,
            Precedence::Term => Precedence::Factor,
            Precedence::Factor => Precedence::Unary,
            Precedence::Unary => Precedence::Call,
            Precedence::Call => Precedence::Primary,
            Precedence::Primary => Precedence::Primary,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParseRule {
    pub(crate) prefix: PrefixRule,
    pub(crate) infix: InfixRule,
    pub(crate) precedence: Precedence,
}

impl ParseRule {
    pub(crate) fn get(ty: TokenType) -> &'static Self {
        &RULES[ty as usize]
    }
}

static RULES: [ParseRule; TokenType::COUNT] = {
    let mut rules = [ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::None,
        precedence: Precedence::None,
    }; TokenType::COUNT];

    rules[tt![ident] as usize] = ParseRule {
        prefix: PrefixRule::Identifier,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt![int_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralInt,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt![float_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralFloat,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt![str_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralString,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt![null] as usize] = ParseRule {
        prefix: PrefixRule::LiteralNull,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt![void] as usize] = ParseRule {
        prefix: PrefixRule::LiteralVoid,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt!['('] as usize] = ParseRule {
        prefix: PrefixRule::Grouping,
        infix: InfixRule::Call,
        precedence: Precedence::Call,
    };

    rules[tt!['['] as usize] = ParseRule {
        prefix: PrefixRule::LiteralArray,
        infix: InfixRule::Index,
        precedence: Precedence::Call,
    };

    rules[tt![.] as usize] = ParseRule {
        prefix: PrefixRule::LiteralStruct,
        infix: InfixRule::Dot,
        precedence: Precedence::Call,
    };

    rules[tt![%] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Factor,
    };

    rules[tt![+] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Term,
    };

    rules[tt![*] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Factor,
    };

    rules[tt![/] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Factor,
    };

    rules[tt![!] as usize] = ParseRule {
        prefix: PrefixRule::Unary,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![!=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Equality,
    };

    rules[tt![==] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Equality,
    };

    rules[tt![>] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Comparison,
    };

    rules[tt![>=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Comparison,
    };

    rules[tt![<] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Comparison,
    };

    rules[tt![<=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Comparison,
    };

    rules[tt![-] as usize] = ParseRule {
        prefix: PrefixRule::Unary,
        infix: InfixRule::Binary,
        precedence: Precedence::Term,
    };

    rules[tt![=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        precedence: Precedence::Assignment,
    };
    rules[tt![+=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        precedence: Precedence::Assignment,
    };

    rules[tt![-=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        precedence: Precedence::Assignment,
    };

    rules[tt![*=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        precedence: Precedence::Assignment,
    };

    rules[tt![/=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        precedence: Precedence::Assignment,
    };

    rules[tt![%=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        precedence: Precedence::Assignment,
    };

    rules[tt![and] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::And,
    };

    rules[tt![false] as usize] = ParseRule {
        prefix: PrefixRule::False,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt![or] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        precedence: Precedence::Or,
    };

    rules[tt![true] as usize] = ParseRule {
        prefix: PrefixRule::True,
        infix: InfixRule::None,
        precedence: Precedence::Primary,
    };

    rules[tt![if] as usize] = ParseRule {
        prefix: PrefixRule::If,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt!['{'] as usize] = ParseRule {
        prefix: PrefixRule::Block,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![while] as usize] = ParseRule {
        prefix: PrefixRule::While,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![loop] as usize] = ParseRule {
        prefix: PrefixRule::Loop,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![match] as usize] = ParseRule {
        prefix: PrefixRule::Match,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![return] as usize] = ParseRule {
        prefix: PrefixRule::Return,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![break] as usize] = ParseRule {
        prefix: PrefixRule::Break,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![continue] as usize] = ParseRule {
        prefix: PrefixRule::Continue,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![mod] as usize] = ParseRule {
        prefix: PrefixRule::ModuleType,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![struct] as usize] = ParseRule {
        prefix: PrefixRule::StructType,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![union] as usize] = ParseRule {
        prefix: PrefixRule::UnionType,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![enum] as usize] = ParseRule {
        prefix: PrefixRule::EnumType,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules[tt![@] as usize] = ParseRule {
        prefix: PrefixRule::PointerType,
        infix: InfixRule::None,
        precedence: Precedence::None,
    };

    rules
};
