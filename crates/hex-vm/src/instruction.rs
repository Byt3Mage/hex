// Fundamental types.
// Change ONLY these two, everything else derives automatically.
//
// To widen registers to 16-bit and instructions to 64-bit:
//   type RegType = u16;
//   type InstWidth = u64;

pub type Reg = u8;
pub type InstType = u32;

// Derived constants. NEVER EDIT DIRECTLY
const REG_BITS: InstType = (std::mem::size_of::<Reg>() * 8) as _;
const INST_BITS: InstType = (std::mem::size_of::<InstType>() * 8) as _;
const OPCODE_BITS: InstType = (std::mem::size_of::<Opcode>() * 8) as _;

const REG_MASK: InstType = (1 << REG_BITS) - 1;

// Remaining bits after opcode and register fields are set (for ABC format)
const FIELD_A: InstType = OPCODE_BITS;
const FIELD_B: InstType = FIELD_A + REG_BITS;
const FIELD_C: InstType = FIELD_B + REG_BITS;

// Remaining bits after opcode + one register (for ABx format)
const BX_BITS: InstType = INST_BITS - FIELD_B;
const BX_MASK: InstType = (1 << BX_BITS) - 1;

// Remaining bits after opcode (for Ax format)
const AX_BITS: InstType = INST_BITS - OPCODE_BITS;
const AX_MASK: InstType = (1 << AX_BITS) - 1;

// Assert that instruction width can safely encode ABC format without overflow
const _: () = assert!(
    OPCODE_BITS + (REG_BITS * 3) <= INST_BITS,
    "ABC format does not fit in instruction width"
);

#[inline(always)]
const fn encode_reg(reg: Reg) -> InstType {
    reg as InstType
}

#[inline(always)]
const fn decode_reg(bits: InstType) -> Reg {
    bits as Reg
}

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
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    COPY, CONST, // Move ops
    NOT, BNOT, INEG, FNEG, // Unary operations
    ADD, SUB, MUL, // signed/unsigned int arithmetic
    SDIV, SREM, // signed int division
    UDIV, UREM, // unsigned int division
    FADD, FSUB, FMUL, FDIV, FREM, // floating point arithmetic
    EQ, NE, // signed/unsigned int equality
    ILT, IGT, ILE, IGE, // signed int comparison
    ULT, UGT, ULE, UGE, // unsigned int comparison
    FEQ, FNE, FLT, FGT, FLE, FGE, // floating point comparison
    JMP, JMP_T, JMP_F, // Jump ops
    RET, CALL, CALLR, // Return and call ops
    LOAD, STORE, // Heap memory ops
    HALT, // end program
}

/// Instruction encoding:
/// - **ABC**: `[opcode(8)] [a: Reg] [b: Reg] [c: Reg]`
/// - **ABx**: `[opcode(8)] [a: Reg] [bx: remaining bits]`
/// - **Ax**:  `[opcode(8)] [ax: remaining bits]`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Instruction(InstType);

impl Instruction {
    #[inline(always)]
    pub const fn raw(self) -> InstType {
        self.0
    }

    #[inline(always)]
    pub const fn op(self) -> Opcode {
        Opcode((self.0 & 0xFF) as u8)
    }

    // ABC format
    #[inline(always)]
    pub const fn a(self) -> Reg {
        decode_reg((self.0 >> FIELD_A) & REG_MASK)
    }

    #[inline(always)]
    pub const fn b(self) -> Reg {
        decode_reg((self.0 >> FIELD_B) & REG_MASK)
    }

    #[inline(always)]
    pub const fn c(self) -> Reg {
        decode_reg((self.0 >> FIELD_C) & REG_MASK)
    }

    // ABx Format
    #[inline(always)]
    pub const fn bx(self) -> InstType {
        (self.0 >> FIELD_B) & BX_MASK
    }

    #[inline(always)]
    pub const fn bx_u64(self) -> u64 {
        self.bx() as u64
    }

    #[inline(always)]
    pub const fn bx_i64(self) -> i64 {
        // Sign-extend from BX_BITS width
        let raw = self.bx() as i64;
        let sign_bit = 1i64 << (BX_BITS - 1);
        (raw ^ sign_bit) - sign_bit
    }

    // Ax Format
    #[inline(always)]
    pub const fn ax(self) -> InstType {
        (self.0 >> FIELD_A) & AX_MASK
    }
}

pub const R0: Reg = 0;

#[inline(always)]
pub const fn encode_abc(Opcode(op): Opcode, a: Reg, b: Reg, c: Reg) -> Instruction {
    Instruction(
        (op as InstType)
            | (encode_reg(a) << FIELD_A)
            | (encode_reg(b) << FIELD_B)
            | (encode_reg(c) << FIELD_C),
    )
}

#[inline(always)]
pub const fn encode_abx(Opcode(op): Opcode, a: Reg, bx: InstType) -> Instruction {
    assert!(bx <= BX_MASK, "bx field overflow");
    Instruction((op as InstType) | (encode_reg(a) << FIELD_A) | (bx << FIELD_B))
}

#[inline(always)]
pub const fn encode_ax(Opcode(op): Opcode, ax: InstType) -> Instruction {
    assert!(ax <= AX_MASK, "ax field overflow");
    Instruction((op as InstType) | (ax << FIELD_A))
}

// Move
#[inline(always)]
pub const fn copy(dst: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::COPY, dst, src, R0)
}

#[inline(always)]
pub const fn const_(dst: Reg, idx: InstType) -> Instruction {
    encode_abx(Opcode::CONST, dst, idx)
}

// Unary ops
#[inline(always)]
pub const fn not(dst: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::NOT, dst, src, R0)
}

#[inline(always)]
pub const fn bnot(dst: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::BNOT, dst, src, R0)
}

#[inline(always)]
pub const fn ineg(dst: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::INEG, dst, src, R0)
}

#[inline(always)]
pub const fn fneg(dst: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::FNEG, dst, src, R0)
}

// Signed/unsigned integer arithmetic
#[inline(always)]
pub const fn add(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::ADD, dst, a, b)
}

#[inline(always)]
pub const fn sub(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::SUB, dst, a, b)
}

#[inline(always)]
pub const fn mul(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::MUL, dst, a, b)
}

// Signed integer division
#[inline(always)]
pub const fn sdiv(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::SDIV, dst, a, b)
}

#[inline(always)]
pub const fn srem(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::SREM, dst, a, b)
}

// Unsigned integer division
#[inline(always)]
pub const fn udiv(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UDIV, dst, a, b)
}

#[inline(always)]
pub const fn urem(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UREM, dst, a, b)
}

// Signed integer comparisons
#[inline(always)]
pub const fn eq(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::EQ, dst, a, b)
}

#[inline(always)]
pub const fn ne(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::NE, dst, a, b)
}

#[inline(always)]
pub const fn ilt(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::ILT, dst, a, b)
}

#[inline(always)]
pub const fn igt(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::IGT, dst, a, b)
}

#[inline(always)]
pub const fn ile(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::ILE, dst, a, b)
}

#[inline(always)]
pub const fn ige(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::IGE, dst, a, b)
}

// Unsigned integer comparisons
#[inline(always)]
pub const fn ult(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::ULT, dst, a, b)
}

#[inline(always)]
pub const fn ugt(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UGT, dst, a, b)
}

#[inline(always)]
pub const fn ule(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::ULE, dst, a, b)
}

#[inline(always)]
pub const fn uge(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UGE, dst, a, b)
}

// Floating-point arithmetic
#[inline(always)]
pub const fn fadd(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FADD, dst, a, b)
}

#[inline(always)]
pub const fn fsub(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FSUB, dst, a, b)
}

#[inline(always)]
pub const fn fmul(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FMUL, dst, a, b)
}

#[inline(always)]
pub const fn fdiv(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FDIV, dst, a, b)
}

#[inline(always)]
pub const fn frem(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FREM, dst, a, b)
}

// Floating-point comparisons
#[inline(always)]
pub const fn feq(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FEQ, dst, a, b)
}

#[inline(always)]
pub const fn fne(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FNE, dst, a, b)
}

#[inline(always)]
pub const fn flt(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FLT, dst, a, b)
}

#[inline(always)]
pub const fn fgt(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FGT, dst, a, b)
}

#[inline(always)]
pub const fn fle(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FLE, dst, a, b)
}

#[inline(always)]
pub const fn fge(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::FGE, dst, a, b)
}

// Jumps
#[inline(always)]
pub const fn jmp(target: InstType) -> Instruction {
    encode_ax(Opcode::JMP, target)
}

#[inline(always)]
pub const fn jmp_t(cond: Reg, target: InstType) -> Instruction {
    encode_abx(Opcode::JMP_T, cond, target)
}

#[inline(always)]
pub const fn jmp_f(cond: Reg, target: InstType) -> Instruction {
    encode_abx(Opcode::JMP_F, cond, target)
}

// Call and return
#[inline(always)]
pub const fn call(ret: Reg, func: InstType) -> Instruction {
    encode_abx(Opcode::CALL, ret, func)
}

#[inline(always)]
pub const fn callr(ret: Reg, func: Reg) -> Instruction {
    encode_abc(Opcode::CALLR, ret, func, R0)
}

#[inline(always)]
pub const fn ret() -> Instruction {
    encode_ax(Opcode::RET, 0)
}

// End program
#[inline(always)]
pub const fn halt() -> Instruction {
    encode_ax(Opcode::HALT, 0)
}
