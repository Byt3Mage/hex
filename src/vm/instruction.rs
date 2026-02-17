// Fundamental types.
// Change ONLY these two, everything else derives automatically.
//
// To widen registers to 16-bit and instructions to 64-bit:
//   type RegType = u16;
//   type InstWidth = u64;

pub type RegType = u8;
pub type InstType = u32;

// Derived constants. NEVER EDIT DIRECTLY
const REG_BITS: InstType = (std::mem::size_of::<RegType>() * 8) as _;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Reg(RegType);

impl Reg {
    #[inline(always)]
    pub const fn raw(self) -> RegType {
        self.0
    }

    #[inline(always)]
    pub const fn new(index: RegType) -> Self {
        Self(index)
    }

    #[inline(always)]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline(always)]
    const fn encode(self) -> InstType {
        self.0 as InstType
    }

    #[inline(always)]
    const fn decode(bits: InstType) -> Self {
        Self(bits as RegType)
    }
}

impl std::fmt::Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "R{}", self.0)
    }
}

macro_rules! define_opcodes {
    ($($name:ident = $value:expr),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(transparent)]
        pub struct Opcode(pub u8);

        impl Opcode {
            $(pub const $name: Self = Self($value);)*
        }

        impl std::fmt::Display for Opcode {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match *self {
                    $(Self($value) => f.write_str(stringify!($name)),)*
                    _ => write!(f, "UNKNOWN({})", self.0),
                }
            }
        }
    };
}

define_opcodes! {
    // Move between registers
    MOV = 0,

    // Get value from constants table
    CONST = 1,

    // Unary operations
    BNOT = 2,
    INOT = 3,
    UNOT = 4,
    INEG = 5,
    FNEG = 6,

    // Signed integer arithmetic
    IADD = 10,
    ISUB = 11,
    IMUL = 12,
    IDIV = 13,
    IREM = 14,

    // Unsigned integer arithmetic
    UADD = 15,
    USUB = 16,
    UMUL = 17,
    UDIV = 18,
    UREM = 19,

    // Floating-point arithmetic
    FADD = 20,
    FSUB = 21,
    FMUL = 22,
    FDIV = 23,
    FREM = 24,

    // Signed integer comparisons
    IEQ = 25,
    INE = 26,
    ILT = 27,
    IGT = 28,
    ILE = 29,
    IGE = 30,

    // Unsigned integer comparisons
    UEQ = 31,
    UNE = 32,
    ULT = 33,
    UGT = 34,
    ULE = 35,
    UGE = 36,

    // Floating-point comparisons
    FEQ = 37,
    FNE = 38,
    FLT = 39,
    FGT = 40,
    FLE = 41,
    FGE = 42,

    // Jumps
    JMP   = 50,
    JMP_T = 51,
    JMP_F = 52,

    // Call and return
    RET    = 60,
    CALL   = 61,
    CALLT  = 62,
    CALLN  = 63,
    CALLR  = 64,
    CALLNR = 65,

    // Heap allocations
    ALLOC_BUF = 70,
    ALLOC_DYN = 71,
    ALLOC_STR = 72,

    // Heap operations
    LOAD  = 73,
    STORE = 74,

    SPAWN = 80,
    AWAIT = 81,

    // End program
    HALT = 82,
}

// Instruction encoding
//
// ABC: [opcode] [a: Reg] [b: Reg] [c: Reg]
// ABx: [opcode] [a: Reg] [bx: remaining bits]
// Ax:  [opcode] [ax: remaining bits]

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Instruction(InstType);

impl Instruction {
    #[inline(always)]
    pub const fn op(self) -> Opcode {
        Opcode((self.0 & 0xFF) as u8)
    }

    // ABC format

    #[inline(always)]
    pub const fn a(self) -> Reg {
        Reg::decode((self.0 >> FIELD_A) & REG_MASK)
    }

    #[inline(always)]
    pub const fn b(self) -> Reg {
        Reg::decode((self.0 >> FIELD_B) & REG_MASK)
    }

    #[inline(always)]
    pub const fn c(self) -> Reg {
        Reg::decode((self.0 >> FIELD_C) & REG_MASK)
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

pub const R0: Reg = Reg::new(0);

#[inline(always)]
pub const fn encode_abc(Opcode(op): Opcode, a: Reg, b: Reg, c: Reg) -> Instruction {
    Instruction(
        (op as InstType)
            | (a.encode() << FIELD_A)
            | (b.encode() << FIELD_B)
            | (c.encode() << FIELD_C),
    )
}

#[inline(always)]
pub const fn encode_abx(Opcode(op): Opcode, a: Reg, bx: InstType) -> Instruction {
    debug_assert!(bx <= BX_MASK, "bx field overflow");
    Instruction((op as InstType) | (a.encode() << FIELD_A) | (bx << FIELD_B))
}

#[inline(always)]
pub const fn encode_ax(Opcode(op): Opcode, ax: InstType) -> Instruction {
    debug_assert!(ax <= AX_MASK, "ax field overflow");
    Instruction((op as InstType) | (ax << FIELD_A))
}

// Move
#[inline(always)]
pub const fn mov(dst: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::MOV, dst, src, R0)
}

#[inline(always)]
pub const fn konst(dst: Reg, idx: InstType) -> Instruction {
    encode_abx(Opcode::CONST, dst, idx)
}

// Unary ops
#[inline(always)]
pub const fn bnot(dst: Reg) -> Instruction {
    encode_abc(Opcode::BNOT, dst, R0, R0)
}

#[inline(always)]
pub const fn inot(dst: Reg) -> Instruction {
    encode_abc(Opcode::INOT, dst, R0, R0)
}

#[inline(always)]
pub const fn unot(dst: Reg) -> Instruction {
    encode_abc(Opcode::UNOT, dst, R0, R0)
}

#[inline(always)]
pub const fn ineg(dst: Reg) -> Instruction {
    encode_abc(Opcode::INEG, dst, R0, R0)
}

#[inline(always)]
pub const fn fneg(dst: Reg) -> Instruction {
    encode_abc(Opcode::FNEG, dst, R0, R0)
}

// Signed integer arithmetic
#[inline(always)]
pub const fn iadd(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::IADD, dst, a, b)
}

#[inline(always)]
pub const fn isub(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::ISUB, dst, a, b)
}

#[inline(always)]
pub const fn imul(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::IMUL, dst, a, b)
}

#[inline(always)]
pub const fn idiv(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::IDIV, dst, a, b)
}

#[inline(always)]
pub const fn irem(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::IREM, dst, a, b)
}

// Unsigned integer arithmetic
#[inline(always)]
pub const fn uadd(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UADD, dst, a, b)
}

#[inline(always)]
pub const fn usub(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::USUB, dst, a, b)
}

#[inline(always)]
pub const fn umul(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UMUL, dst, a, b)
}

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
pub const fn ieq(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::IEQ, dst, a, b)
}

#[inline(always)]
pub const fn ine(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::INE, dst, a, b)
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
pub const fn ueq(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UEQ, dst, a, b)
}

#[inline(always)]
pub const fn une(dst: Reg, a: Reg, b: Reg) -> Instruction {
    encode_abc(Opcode::UNE, dst, a, b)
}

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
pub const fn calln(ret: Reg, func: InstType) -> Instruction {
    encode_abx(Opcode::CALLN, ret, func)
}

#[inline(always)]
pub const fn callnr(ret: Reg, func: Reg) -> Instruction {
    encode_abc(Opcode::CALLNR, ret, func, R0)
}

#[inline(always)]
pub const fn callt(ret: Reg, func: InstType) -> Instruction {
    encode_abx(Opcode::CALLT, ret, func)
}

#[inline(always)]
pub const fn ret(src: Reg) -> Instruction {
    encode_abc(Opcode::RET, src, R0, R0)
}

// Heap allocations
#[inline(always)]
pub const fn alloc_buf(dst: Reg, len: Reg) -> Instruction {
    encode_abc(Opcode::ALLOC_BUF, dst, len, R0)
}

#[inline(always)]
pub const fn alloc_dyn(dst: Reg) -> Instruction {
    encode_abc(Opcode::ALLOC_DYN, dst, R0, R0)
}

#[inline(always)]
pub const fn alloc_str(dst: Reg) -> Instruction {
    encode_abc(Opcode::ALLOC_STR, dst, R0, R0)
}

// Heap operations
#[inline(always)]
pub const fn load(dst: Reg, ptr: Reg, off: Reg) -> Instruction {
    encode_abc(Opcode::LOAD, dst, ptr, off)
}

#[inline(always)]
pub const fn store(ptr: Reg, off: Reg, src: Reg) -> Instruction {
    encode_abc(Opcode::STORE, ptr, off, src)
}

// End program
#[inline(always)]
pub const fn halt() -> Instruction {
    encode_ax(Opcode::HALT, 0)
}
