use crate::vm::program::FunctionId;

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
                f.write_fmt(format_args!("{}", self.0))
            }
        }
    };
}

define_opcodes! {
    // Move between registers
    MOV = 0,

    // Get value from constants table
    CONST = 1,

    //Unary operations
    NOT  = 2,
    INEG = 3,
    FNEG = 5,

    // Integer arithmetic
    IADD = 10,
    ISUB = 11,
    IMUL = 12,
    IDIV = 13,
    IREM = 14,

    // Immediate integer arithmetic
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
    FEQ = 35,
    FNE = 36,
    FLT = 37,
    FGT = 38,
    FLE = 39,
    FGE = 40,

    // Jumps
    JMP   = 50,  // Unconditional jump
    JMP_T = 51,  // Conditional: jump if rA is true
    JMP_F = 52,  // Conditional: jump if rA is false

    // Call and return
    RET    = 60,
    CALL   = 61,
    CALLT  = 62,
    CALLN  = 63,
    // CALL/CALLN (register)
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

#[derive(Debug, Clone, Copy)]
pub struct Instruction(u32);

impl Instruction {
    #[inline(always)]
    pub const fn op(self) -> Opcode {
        Opcode((self.0 & 0xFF) as u8)
    }

    // --- ABC Format ---
    #[inline(always)]
    pub const fn a(self) -> u8 {
        ((self.0 >> 8) & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn b(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn c(self) -> u8 {
        ((self.0 >> 24) & 0xFF) as u8
    }

    #[inline(always)]
    pub const fn encode_abc(Opcode(op): Opcode, a: u8, b: u8, c: u8) -> Self {
        Instruction((op as u32) | ((a as u32) << 8) | ((b as u32) << 16) | ((c as u32) << 24))
    }

    // --- ABx Format ---
    #[inline(always)]
    pub const fn bx(self) -> u16 {
        ((self.0 >> 16) & 0xFFFF) as u16
    }

    #[inline(always)]
    pub const fn bx_u64(self) -> u64 {
        self.bx() as u64
    }

    #[inline(always)]
    pub const fn bx_i64(self) -> i64 {
        self.bx() as i16 as i64
    }

    #[inline(always)]
    pub const fn encode_abx(Opcode(op): Opcode, a: u8, bx: u16) -> Self {
        Instruction((op as u32) | ((a as u32) << 8) | ((bx as u32) << 16))
    }

    // --- Ax Format ---
    #[inline(always)]
    pub const fn ax(self) -> u32 {
        self.0 >> 8
    }

    #[inline(always)]
    pub const fn encode_ax(Opcode(op): Opcode, ax: u32) -> Self {
        debug_assert!(ax <= 0xFFFFFF, "ax field must fit in 24 bits");
        Instruction((op as u32) | (ax << 8))
    }
}

#[inline(always)]
pub const fn mov(r_dst: u8, r_src: u8) -> Instruction {
    Instruction::encode_abc(Opcode::MOV, r_dst, r_src, 0)
}

#[inline(always)]
pub const fn konst(r_dst: u8, idx: u16) -> Instruction {
    Instruction::encode_abx(Opcode::CONST, r_dst, idx)
}

// Unary ops
#[inline(always)]
pub const fn not(r_dst: u8) -> Instruction {
    Instruction::encode_abc(Opcode::NOT, r_dst, 0, 0)
}

#[inline(always)]
pub const fn ineg(r_dst: u8) -> Instruction {
    Instruction::encode_abc(Opcode::INEG, r_dst, 0, 0)
}

#[inline(always)]
pub const fn fneg(r_dst: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FNEG, r_dst, 0, 0)
}

// Signed integer arithmetic
#[inline(always)]
pub fn iadd(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::IADD, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn isub(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ISUB, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn imul(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::IMUL, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn idiv(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::IDIV, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn irem(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::IREM, r_dst, r_a, r_b)
}

// Unsigned integer arithmetic
#[inline(always)]
pub fn uadd(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UADD, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn usub(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::USUB, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn umul(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UMUL, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn udiv(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UDIV, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn urem(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UREM, r_dst, r_a, r_b)
}

// Signed integer comparisons
#[inline(always)]
pub fn ieq(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::IEQ, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn ine(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::INE, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn ilt(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ILT, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn igt(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::IGT, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn ile(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ILE, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn ige(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::IGE, r_dst, r_a, r_b)
}

// Unsigned integer comparisons
#[inline(always)]
pub fn ueq(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UEQ, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn une(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UNE, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn ult(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ULT, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn ule(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ULE, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn ugt(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UGT, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn uge(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::UGE, r_dst, r_a, r_b)
}

// Floating-point arithmetic
#[inline(always)]
pub fn fadd(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FADD, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn fsub(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FSUB, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn fmul(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FMUL, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn fdiv(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FDIV, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn frem(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FREM, r_dst, r_a, r_b)
}

// Floating-point comparisons
#[inline(always)]
pub fn feq(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FEQ, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn fne(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FNE, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn flt(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FLT, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn fgt(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FGT, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn fle(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FLE, r_dst, r_a, r_b)
}

#[inline(always)]
pub fn fge(r_dst: u8, r_a: u8, r_b: u8) -> Instruction {
    Instruction::encode_abc(Opcode::FGE, r_dst, r_a, r_b)
}

// Jumps
#[inline(always)]
pub fn jmp(target: u32) -> Instruction {
    Instruction::encode_ax(Opcode::JMP, target)
}

#[inline(always)]
pub fn jmp_t(r_cond: u8, target: u16) -> Instruction {
    Instruction::encode_abx(Opcode::JMP_T, r_cond, target)
}

#[inline(always)]
pub fn jmp_f(r_cond: u8, target: u16) -> Instruction {
    Instruction::encode_abx(Opcode::JMP_F, r_cond, target)
}

// Call and return
#[inline(always)]
pub fn call(r_ret: u8, func: FunctionId) -> Instruction {
    Instruction::encode_abx(Opcode::CALL, r_ret, func)
}

#[inline(always)]
pub fn callr(r_ret: u8, r_func: u8) -> Instruction {
    Instruction::encode_abc(Opcode::CALLR, r_ret, r_func, 0)
}

#[inline(always)]
pub fn calln(r_ret: u8, func: FunctionId) -> Instruction {
    Instruction::encode_abx(Opcode::CALLN, r_ret, func)
}

#[inline(always)]
pub fn callnr(r_ret: u8, func: FunctionId) -> Instruction {
    Instruction::encode_abx(Opcode::CALLN, r_ret, func)
}

#[inline(always)]
pub fn callt(r_ret: u8, func: FunctionId) -> Instruction {
    Instruction::encode_abx(Opcode::CALLT, r_ret, func)
}

#[inline(always)]
pub fn ret(r_src: u8) -> Instruction {
    Instruction::encode_abc(Opcode::RET, r_src, 0, 0)
}

#[inline(always)]
pub const fn alloc_buf(r_dst: u8, r_len: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ALLOC_BUF, r_dst, r_len, 0)
}

#[inline(always)]
pub const fn alloc_dyn(r_dst: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ALLOC_DYN, r_dst, 0, 0)
}

#[inline(always)]
pub const fn alloc_str(r_dst: u8) -> Instruction {
    Instruction::encode_abc(Opcode::ALLOC_STR, r_dst, 0, 0)
}

#[inline(always)]
pub const fn load(r_dst: u8, r_ptr: u8, r_off: u8) -> Instruction {
    Instruction::encode_abc(Opcode::LOAD, r_dst, r_ptr, r_off)
}

#[inline(always)]
pub const fn store(r_ptr: u8, r_off: u8, r_src: u8) -> Instruction {
    Instruction::encode_abc(Opcode::STORE, r_ptr, r_off, r_src)
}

// End program
#[inline(always)]
pub fn halt() -> Instruction {
    Instruction::encode_ax(Opcode::HALT, 0)
}
