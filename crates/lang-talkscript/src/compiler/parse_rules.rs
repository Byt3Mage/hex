use simple_ternary::tnr;

use super::tokens::TokenType;
use crate::tt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixRule {
    None,

    // Literals
    LiteralCint,
    LiteralUint,
    LiteralInt,
    LiteralFloat,
    LiteralString,
    LiteralBool,
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

    // Types
    Int,
    Uint,
    Bool,
    Void,
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
    Coalesce,   // ?:
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
    fn next(&self) -> Self {
        match self {
            Precedence::None => Precedence::Assignment,
            Precedence::Assignment => Precedence::Coalesce,
            Precedence::Coalesce => Precedence::Or,
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
    pub(crate) prec: Precedence,
    pub(crate) right_assoc: bool,
}

impl ParseRule {
    pub(crate) fn get(ty: TokenType) -> &'static Self {
        &RULES[ty as usize]
    }

    pub(crate) fn rhs_prec(&self) -> Precedence {
        tnr! {self.right_assoc => self.prec : self.prec.next()}
    }
}

static RULES: [ParseRule; TokenType::COUNT] = {
    let mut rules = [ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    }; TokenType::COUNT];

    rules[tt![ident] as usize] = ParseRule {
        prefix: PrefixRule::Identifier,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![cint_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralCint,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![uint_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralUint,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![int_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralInt,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![float_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralFloat,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![str_lit] as usize] = ParseRule {
        prefix: PrefixRule::LiteralString,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![null] as usize] = ParseRule {
        prefix: PrefixRule::LiteralNull,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![void] as usize] = ParseRule {
        prefix: PrefixRule::LiteralVoid,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt!['('] as usize] = ParseRule {
        prefix: PrefixRule::Grouping,
        infix: InfixRule::Call,
        prec: Precedence::Call,
        right_assoc: false,
    };

    rules[tt!['['] as usize] = ParseRule {
        prefix: PrefixRule::LiteralArray,
        infix: InfixRule::Index,
        prec: Precedence::Call,
        right_assoc: false,
    };

    rules[tt![.] as usize] = ParseRule {
        prefix: PrefixRule::LiteralStruct,
        infix: InfixRule::Dot,
        prec: Precedence::Call,
        right_assoc: false,
    };

    rules[tt![%] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Factor,
        right_assoc: false,
    };

    rules[tt![+] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Term,
        right_assoc: false,
    };

    rules[tt![*] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Factor,
        right_assoc: false,
    };

    rules[tt![/] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Factor,
        right_assoc: false,
    };

    rules[tt![&] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Factor,
        right_assoc: false,
    };

    rules[tt![|] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Term,
        right_assoc: false,
    };

    rules[tt![^] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Term,
        right_assoc: false,
    };

    rules[tt![<<] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Factor,
        right_assoc: false,
    };

    rules[tt![>>] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Factor,
        right_assoc: false,
    };

    rules[tt![!] as usize] = ParseRule {
        prefix: PrefixRule::Unary,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![!=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Equality,
        right_assoc: false,
    };

    rules[tt![==] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Equality,
        right_assoc: false,
    };

    rules[tt![>] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Comparison,
        right_assoc: false,
    };

    rules[tt![>=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Comparison,
        right_assoc: false,
    };

    rules[tt![<] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Comparison,
        right_assoc: false,
    };

    rules[tt![<=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Comparison,
        right_assoc: false,
    };

    rules[tt![-] as usize] = ParseRule {
        prefix: PrefixRule::Unary,
        infix: InfixRule::Binary,
        prec: Precedence::Term,
        right_assoc: false,
    };

    rules[tt![?:] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Coalesce,
        right_assoc: false,
    };

    rules[tt![=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        prec: Precedence::Assignment,
        right_assoc: true,
    };

    rules[tt![+=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        prec: Precedence::Assignment,
        right_assoc: true,
    };

    rules[tt![-=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        prec: Precedence::Assignment,
        right_assoc: true,
    };

    rules[tt![*=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        prec: Precedence::Assignment,
        right_assoc: true,
    };

    rules[tt![/=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        prec: Precedence::Assignment,
        right_assoc: true,
    };

    rules[tt![%=] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Assign,
        prec: Precedence::Assignment,
        right_assoc: true,
    };

    rules[tt![and] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::And,
        right_assoc: false,
    };

    rules[tt![false] as usize] = ParseRule {
        prefix: PrefixRule::LiteralBool,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![or] as usize] = ParseRule {
        prefix: PrefixRule::None,
        infix: InfixRule::Binary,
        prec: Precedence::Or,
        right_assoc: false,
    };

    rules[tt![true] as usize] = ParseRule {
        prefix: PrefixRule::LiteralBool,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![if] as usize] = ParseRule {
        prefix: PrefixRule::If,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt!['{'] as usize] = ParseRule {
        prefix: PrefixRule::Block,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![while] as usize] = ParseRule {
        prefix: PrefixRule::While,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![loop] as usize] = ParseRule {
        prefix: PrefixRule::Loop,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![match] as usize] = ParseRule {
        prefix: PrefixRule::Match,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![return] as usize] = ParseRule {
        prefix: PrefixRule::Return,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![break] as usize] = ParseRule {
        prefix: PrefixRule::Break,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![continue] as usize] = ParseRule {
        prefix: PrefixRule::Continue,
        infix: InfixRule::None,
        prec: Precedence::None,
        right_assoc: false,
    };

    rules[tt![int] as usize] = ParseRule {
        prefix: PrefixRule::Int,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![uint] as usize] = ParseRule {
        prefix: PrefixRule::Uint,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules[tt![bool] as usize] = ParseRule {
        prefix: PrefixRule::Bool,
        infix: InfixRule::None,
        prec: Precedence::Primary,
        right_assoc: false,
    };

    rules
};
