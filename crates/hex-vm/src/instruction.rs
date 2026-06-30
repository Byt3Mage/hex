// Fundamental types.
// Change ONLY these two, everything else derives automatically.

/// Register type.
pub type Reg = u8;

/// Instruction encoding:
/// - ABC: `[opcode(8)] [a: Reg] [b: Reg] [c: Reg]`
/// - ABx: `[opcode(8)] [a: Reg] [bx: remaining bits]`
/// - Ax:  `[opcode(8)] [ax: remaining bits]`
pub type Instruction = u32;

// Derived constants. NEVER EDIT DIRECTLY
const REG_BITS: Instruction = (core::mem::size_of::<Reg>() * 8) as _;
const INST_BITS: Instruction = (core::mem::size_of::<Instruction>() * 8) as _;
const OPCODE_BITS: Instruction = (core::mem::size_of::<Opcode>() * 8) as _;

const REG_MASK: Instruction = (1 << REG_BITS) - 1;

// Remaining bits after opcode and register fields are set (for ABC format)
const FIELD_A: Instruction = OPCODE_BITS;
const FIELD_B: Instruction = FIELD_A + REG_BITS;
const FIELD_C: Instruction = FIELD_B + REG_BITS;

// Remaining bits after opcode + one register (for ABx format)
const BX_BITS: Instruction = INST_BITS - FIELD_B;
const BX_MASK: Instruction = (1 << BX_BITS) - 1;

// Remaining bits after opcode (for Ax format)
const AX_BITS: Instruction = INST_BITS - OPCODE_BITS;
const AX_MASK: Instruction = (1 << AX_BITS) - 1;

// Assert that instruction width can safely encode ABC format without overflow
const _: () = assert!(
    OPCODE_BITS + (REG_BITS * 3) <= INST_BITS,
    "ABC format does not fit in instruction width"
);

#[inline(always)]
const fn encode_reg(reg: Reg) -> Instruction {
    reg as Instruction
}

#[inline(always)]
const fn decode_reg(bits: Instruction) -> Reg {
    bits as Reg
}

macro_rules! count_tts {
    () => { 0usize };
    ($head:ident $(, $tail:ident)*) => {
        1usize + count_tts!($($tail),*)
    };
}

macro_rules! define_opcodes {
    ($($name:ident),*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct Opcode(pub u8);

        impl Opcode {
            pub const COUNT: usize = count_tts!($($name),*);
            define_opcodes!(@consts 0u8, $($name),*);
        }

        impl core::fmt::Display for Opcode {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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
    COPY, LOADK, // Move ops
    LOADI, LOADF, // small inline immediate loads
    ADDK, SUBK, MULK, // int arith with constant-pool operand
    FADDK, FSUBK, FMULK, FDIVK, // float arith with constant-pool operand
    NOT, BNOT, INEG, FNEG, // Unary operations
    ADD, SUB, MUL, // signed/unsigned int arithmetic
    ADDI, SUBI, MULI, // int arithmetic with small signed immediate
    SDIV, SREM, // signed int division
    UDIV, UREM, // unsigned int division
    FADD, FSUB, FMUL, FDIV, FREM, // floating point arithmetic
    EQ, NE, // signed/unsigned int equality
    SLT, SGT, SLE, SGE, // signed int comparison
    ULT, UGT, ULE, UGE, // unsigned int comparison
    FEQ, FNE, FLT, FGT, FLE, FGE, // floating point comparison
    JMP, JMP_T, JMP_F, // Jump ops
    JEQ, JNE, // int equality compare-branch
    JSLT, JSGT, JSLE, JSGE, // signed int compare-branch
    JULT, JUGT, JULE, JUGE, // unsigned int compare-branch
    JFEQ, JFNE, JFLT, JFGT, JFLE, JFGE, // float compare-branch
    LOAD, STORE, STORE_ADDRESS, // Heap memory ops
    RET, CALL, TCALL, CALL_IND, TCALL_IND, // Return and call ops
    THROW, // Unwind with a value in register a
    HALT // end program
}

pub const R0: Reg = 0;
const BX_BIAS: i64 = (BX_MASK >> 1) as i64;
const IMM8_BIAS: i64 = 128;

/// A signed immediate that provably fits the 8-bit instruction field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Imm8(u8);

impl Imm8 {
    /// Construct from any integer, returning None if it doesn't fit -128..=127.
    /// This is the fusion gate: `None` means "don't fuse, fall back to CONST".
    #[inline(always)]
    pub const fn from_int(value: i64) -> Option<Self> {
        if value >= -127 && value <= 127 { Some(Self((value + IMM8_BIAS) as u8)) } else { None }
    }

    /// Try to fit an unsigned constant. Only 0..=127 succeed
    /// (the non-negative half of the i8 range).
    #[inline(always)]
    pub const fn from_uint(value: u64) -> Option<Self> {
        if value <= 127 { Some(Self((value as i64 + IMM8_BIAS) as u8)) } else { None }
    }

    /// Biased bits as they sit in the `c` field.
    #[inline(always)]
    pub const fn bits(self) -> Reg {
        self.0 as Reg
    }
}

pub mod inst {
    use super::*;

    #[inline(always)]
    pub const fn op(inst: Instruction) -> Opcode {
        Opcode((inst & 0xFF) as u8)
    }

    // ABC format
    #[inline(always)]
    pub const fn a(inst: Instruction) -> Reg {
        decode_reg((inst >> FIELD_A) & REG_MASK)
    }

    #[inline(always)]
    pub const fn b(inst: Instruction) -> Reg {
        decode_reg((inst >> FIELD_B) & REG_MASK)
    }

    #[inline(always)]
    pub const fn c(inst: Instruction) -> Reg {
        decode_reg((inst >> FIELD_C) & REG_MASK)
    }

    /// Decode the `c` field as a signed 8-bit immediate.
    #[inline(always)]
    pub const fn imm8(inst: Instruction) -> i64 {
        ((inst >> FIELD_C) & 0xFF) as i64 - IMM8_BIAS
    }

    // ABx Format
    #[inline(always)]
    pub const fn bx(inst: Instruction) -> Instruction {
        (inst >> FIELD_B) & BX_MASK
    }

    /// Decode `bx` as an excess-K signed immediate.
    #[inline(always)]
    pub const fn bx_imm(inst: Instruction) -> i64 {
        bx(inst) as i64 - (BX_MASK >> 1) as i64
    }

    // Ax Format
    #[inline(always)]
    pub const fn ax(inst: Instruction) -> Instruction {
        (inst >> FIELD_A) & AX_MASK
    }
}

#[inline(always)]
pub const fn encode_abc(Opcode(op): Opcode, a: Reg, b: Reg, c: Reg) -> Instruction {
    (op as Instruction) | (encode_reg(a) << FIELD_A) | (encode_reg(b) << FIELD_B) | (encode_reg(c) << FIELD_C)
}

#[inline(always)]
pub const fn encode_abx(Opcode(op): Opcode, a: Reg, bx: Instruction) -> Instruction {
    assert!(bx <= BX_MASK, "bx field overflow");
    (op as Instruction) | (encode_reg(a) << FIELD_A) | (bx << FIELD_B)
}

#[inline(always)]
pub const fn encode_ax(Opcode(op): Opcode, ax: Instruction) -> Instruction {
    assert!(ax <= AX_MASK, "ax field overflow");
    (op as Instruction) | (ax << FIELD_A)
}

#[inline(always)]
const fn encode_cmp_branch(op: Opcode, a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    [encode_abc(op, R0, a, b), target]
}

#[inline(always)]
const fn encode_abi(op: Opcode, dst: Reg, src: Reg, imm: Imm8) -> Instruction {
    encode_abc(op, dst, src, imm.bits())
}

// Move
#[inline(always)]
pub const fn copy(dst: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::COPY, dst, src, R0)
}

#[inline(always)]
pub const fn loadk(dst: Reg, idx: Instruction) -> Instruction {
    encode_abx(Opcode::LOADK, dst, idx)
}

#[inline(always)]
pub const fn loadi(dst: Reg, imm: i64) -> Instruction {
    assert!(imm >= -BX_BIAS && imm <= BX_BIAS, "loadi immediate out of range");
    encode_abx(Opcode::LOADI, dst, (imm + BX_BIAS) as Instruction)
}

#[inline(always)]
pub const fn loadf(dst: Reg, imm: i64) -> Instruction {
    assert!(imm >= -BX_BIAS && imm <= BX_BIAS, "loadf immediate out of range");
    encode_abx(Opcode::LOADF, dst, (imm + BX_BIAS) as Instruction)
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

#[inline(always)]
pub const fn addi(dst: Reg, src: Reg, imm: Imm8) -> Instruction {
    encode_abi(Opcode::ADDI, dst, src, imm)
}
#[inline(always)]
pub const fn subi(dst: Reg, src: Reg, imm: Imm8) -> Instruction {
    encode_abi(Opcode::SUBI, dst, src, imm)
}
#[inline(always)]
pub const fn muli(dst: Reg, src: Reg, imm: Imm8) -> Instruction {
    encode_abi(Opcode::MULI, dst, src, imm)
}

#[inline(always)]
pub const fn addk(dst: Reg, src: Reg, idx: Reg) -> Instruction {
    encode_abc(Opcode::ADDK, dst, src, idx)
}

#[inline(always)]
pub const fn subk(dst: Reg, src: Reg, idx: Reg) -> Instruction {
    encode_abc(Opcode::SUBK, dst, src, idx)
}

#[inline(always)]
pub const fn mulk(dst: Reg, src: Reg, idx: Reg) -> Instruction {
    encode_abc(Opcode::MULK, dst, src, idx)
}

#[inline(always)]
pub const fn faddk(dst: Reg, src: Reg, idx: Reg) -> Instruction {
    encode_abc(Opcode::FADDK, dst, src, idx)
}

#[inline(always)]
pub const fn fsubk(dst: Reg, src: Reg, idx: Reg) -> Instruction {
    encode_abc(Opcode::FSUBK, dst, src, idx)
}

#[inline(always)]
pub const fn fmulk(dst: Reg, src: Reg, idx: Reg) -> Instruction {
    encode_abc(Opcode::FMULK, dst, src, idx)
}

#[inline(always)]
pub const fn fdivk(dst: Reg, src: Reg, idx: Reg) -> Instruction {
    encode_abc(Opcode::FDIVK, dst, src, idx)
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
    encode_abc(Opcode::SLT, dst, a, b)
}

#[inline(always)]
pub const fn igt(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::SGT, dst, a, b)
}

#[inline(always)]
pub const fn ile(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::SLE, dst, a, b)
}

#[inline(always)]
pub const fn ige(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::SGE, dst, a, b)
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
pub const fn jmp(target: Instruction) -> Instruction {
    encode_ax(Opcode::JMP, target)
}

#[inline(always)]
pub const fn jmpt(cond: Reg, target: Instruction) -> Instruction {
    encode_abx(Opcode::JMP_T, cond, target)
}

#[inline(always)]
pub const fn jmpf(cond: Reg, target: Instruction) -> Instruction {
    encode_abx(Opcode::JMP_F, cond, target)
}

#[inline(always)]
pub const fn jeq(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JEQ, a, b, target)
}
#[inline(always)]
pub const fn jne(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JNE, a, b, target)
}
#[inline(always)]
pub const fn jslt(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JSLT, a, b, target)
}
#[inline(always)]
pub const fn jsgt(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JSGT, a, b, target)
}
#[inline(always)]
pub const fn jsle(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JSLE, a, b, target)
}
#[inline(always)]
pub const fn jsge(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JSGE, a, b, target)
}
#[inline(always)]
pub const fn jult(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JULT, a, b, target)
}
#[inline(always)]
pub const fn jugt(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JUGT, a, b, target)
}
#[inline(always)]
pub const fn jule(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JULE, a, b, target)
}
#[inline(always)]
pub const fn juge(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JUGE, a, b, target)
}
#[inline(always)]
pub const fn jfeq(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JFEQ, a, b, target)
}
#[inline(always)]
pub const fn jfne(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JFNE, a, b, target)
}
#[inline(always)]
pub const fn jflt(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JFLT, a, b, target)
}
#[inline(always)]
pub const fn jfgt(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JFGT, a, b, target)
}
#[inline(always)]
pub const fn jfle(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JFLE, a, b, target)
}
#[inline(always)]
pub const fn jfge(a: Reg, b: Reg, target: Instruction) -> [Instruction; 2] {
    encode_cmp_branch(Opcode::JFGE, a, b, target)
}

#[inline(always)]
pub const fn load(dst: Reg, ptr: Reg, off: Reg) -> Instruction {
    encode_abc(Opcode::LOAD, dst, ptr, off)
}

#[inline(always)]
pub const fn store(ptr: Reg, off: Reg, val: Reg) -> Instruction {
    encode_abc(Opcode::STORE, ptr, off, val)
}

#[inline(always)]
pub const fn store_address(ptr: Reg, off: Reg, val: Reg) -> Instruction {
    encode_abc(Opcode::STORE_ADDRESS, ptr, off, val)
}

// Call and return
#[inline(always)]
pub const fn call(ret: Reg, func: Instruction) -> Instruction {
    encode_abx(Opcode::CALL, ret, func)
}

#[inline(always)]
pub const fn callr(ret: Reg, func: Reg) -> Instruction {
    encode_abc(Opcode::CALL_IND, ret, func, R0)
}

#[inline(always)]
pub const fn tcall(ret: Reg, func: Instruction) -> Instruction {
    encode_abx(Opcode::TCALL, ret, func)
}

#[inline(always)]
pub const fn tcallr(ret: Reg, func: Reg) -> Instruction {
    encode_abc(Opcode::TCALL_IND, ret, func, R0)
}

#[inline(always)]
pub const fn ret() -> Instruction {
    encode_ax(Opcode::RET, 0)
}

#[inline(always)]
pub const fn throw(val: Reg) -> Instruction {
    encode_abc(Opcode::THROW, val, R0, R0)
}

#[inline(always)]
pub const fn halt() -> Instruction {
    encode_ax(Opcode::HALT, 0)
}
