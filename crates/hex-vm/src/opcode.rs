macro_rules! count_tts {
    ($($tts:tt)*) => {
        0usize $(+ replace_expr!($tts 1usize))*
    };
}

macro_rules! replace_expr {
    ($_t:tt $sub:expr) => {
        $sub
    };
}

macro_rules! define_opcodes {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(transparent)]
        pub struct Opcode(pub u8);

        impl Opcode {
            pub const CORE_COUNT: usize = count_tts!($($name)*);
            define_opcodes!(@consts 0u8, $($name),*);
        }

        impl std::fmt::Display for Opcode {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    $(x if x == Self::$name.0 => f.write_str(stringify!($name)),)*
                    other => write!(f, "UNKNOWN({other})"),
                }
            }
        }
    };

    (@consts $idx:expr, $name:ident $(, $rest:ident)*) => {
        pub const $name: Self = Self($idx);
        define_opcodes!(@consts ($idx + 1), $($rest),*);
    };

    (@consts $idx:expr,) => {};
}

define_opcodes! {
    MOV,
    CONST,
    BNOT,
    INOT,
    UNOT,
    INEG,
    FNEG,
    IADD,
    ISUB,
    IMUL,
    IDIV,
    IREM,
    UADD,
    USUB,
    UMUL,
    UDIV,
    UREM,
    FADD,
    FSUB,
    FMUL,
    FDIV,
    FREM,
    IEQ,
    INE,
    ILT,
    IGT,
    ILE,
    IGE,
    UEQ,
    UNE,
    ULT,
    UGT,
    ULE,
    UGE,
    FEQ,
    FNE,
    FLT,
    FGT,
    FLE,
    FGE,
    JMP,
    JMP_T,
    JMP_F,
    RET,
    CALL,
    CALLT,
    CALLN,
    CALLR,
    CALLNR,
    HALT,
}
