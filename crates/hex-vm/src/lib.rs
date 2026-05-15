use crate::memory::{Buffer, Memory, MemoryError};

pub mod disassemble;
mod instruction;
pub mod memory;
mod program;

pub use instruction::*;
pub use program::*;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Value(u64);

impl Value {
    pub const ZERO: Self = Self(0);

    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn from_le_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
    }

    pub const fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    #[inline(always)]
    pub fn get<T: IsValue>(self) -> T {
        T::from_value(self)
    }

    #[inline(always)]
    pub fn set<T: IsValue>(&mut self, v: T) {
        *self = v.into_value();
    }
}

pub trait IsValue: Copy {
    fn from_value(v: Value) -> Self;
    fn into_value(self) -> Value;
}

impl IsValue for usize {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as usize
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for u64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self)
    }
}

impl IsValue for i64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as i64
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for f64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        f64::from_bits(v.0)
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self.to_bits())
    }
}

impl IsValue for bool {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 != 0
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for u32 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as u32
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for u16 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as u16
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

#[derive(thiserror::Error, Copy, Clone, Debug)]
pub enum VMError {
    #[error("Unknown opcode: {0}")]
    UnknownOp(Opcode),
    #[error("Program counter out of bounds")]
    PCOutOfBounds,
    #[error("Division by zero")]
    DivisionByZero,
    #[error("Memory error: {0}")]
    MemoryError(#[from] MemoryError),
    #[error("Empty call stack")]
    EmptyCallStack,
}

pub type VMResult<T> = Result<T, VMError>;

pub struct Frame {
    pub ret_pc: usize,
    pub ret_base: usize,
}

pub struct VM<'p, B: Buffer> {
    pub registers: Vec<Value>,
    pub call_stack: Vec<Frame>,
    pub pc: usize,
    pub base: usize,
    pub memory: Memory<B>,
    pub pending_interrupt: Option<Interrupt>,
    pub program: &'p Program,
}

impl<'p, B: Buffer> VM<'p, B> {
    pub fn new(buffer: B, program: &'p Program) -> Self {
        Self {
            registers: Vec::new(),
            call_stack: Vec::new(),
            pc: 0,
            base: 0,
            memory: Memory::new(buffer),
            pending_interrupt: None,
            program,
        }
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.memory.reset();
        self.registers.clear();
        self.call_stack.clear();
        self.pc = 0;
        self.base = 0;
    }

    #[inline(always)]
    pub fn reg<T: IsValue>(&self, reg: Reg) -> T {
        T::from_value(self.registers[self.base + reg as usize])
    }

    #[inline(always)]
    pub fn reg_raw(&self, reg: Reg) -> Value {
        self.registers[self.base + reg as usize]
    }

    #[inline(always)]
    pub fn reg_mut(&mut self, reg: Reg) -> &mut Value {
        &mut self.registers[self.base + reg as usize]
    }

    #[inline(always)]
    pub fn set_reg(&mut self, reg: Reg, value: impl IsValue) {
        self.registers[self.base + reg as usize] = value.into_value();
    }

    #[inline(always)]
    pub fn set_reg_raw(&mut self, reg: Reg, value: Value) {
        self.registers[self.base + reg as usize] = value;
    }

    #[inline(always)]
    fn two_reg<T: IsValue>(&self, reg_a: Reg, reg_b: Reg) -> (T, T) {
        (self.reg(reg_a), self.reg(reg_b))
    }

    #[inline(always)]
    fn reg_offset(&self, reg: Reg) -> usize {
        self.base + reg as usize
    }

    #[inline(always)]
    fn pop_frame(&mut self) -> VMResult<Frame> {
        self.call_stack.pop().ok_or(VMError::EmptyCallStack)
    }

    #[inline(always)]
    fn call_vm(&mut self, ret_reg: Reg, entry_pc: usize, nreg: Reg) {
        // Call convention: args are in caller registers[Rret..Rn]
        //
        // We use overlapping register windows for caller and callee
        // to avoid copying arguments. This is safe because argument
        // registers are not clobbered by caller until callee returns.

        // Callee window starts at caller's return register.
        let base = self.reg_offset(ret_reg);
        let last = base + nreg as usize;

        // Grow regs to fit callee's full register count beyond the arg base.
        if last > self.registers.len() {
            self.registers.resize(last, Value::ZERO);
        }

        // Create new call frame and save return point
        self.call_stack.push(Frame {
            ret_pc: self.pc,
            ret_base: self.base,
        });

        // Jump to callee code
        self.base = base;
        self.pc = entry_pc;
    }

    #[inline(always)]
    fn call_host(&mut self, ret_reg: Reg, func: HostFn, nreg: Reg) -> VMResult<()> {
        // Host function call convention: args and rets use the same window.
        // - args are read from  [Rret...Rnarg]
        // - rets are written to [Rret..Rnret]
        //
        // Up to callee not to clobber args before reading from them.
        // Garbage values or crash if args are read out of bounds.
        let ret = self.reg_offset(ret_reg);
        func(&mut self.registers[ret..ret + nreg as usize])
    }

    #[inline(always)]
    pub fn fetch(&mut self) -> VMResult<Instruction> {
        match self.program.instructions.get(self.pc) {
            Some(&instr) => Ok(instr),
            None => Err(VMError::PCOutOfBounds),
        }
    }

    #[inline]
    pub fn execute(&mut self, i: Instruction) -> VMResult<()> {
        match i.op() {
            // Moves
            Opcode::COPY => self.set_reg_raw(i.a(), self.reg_raw(i.b())),
            Opcode::CONST => self.set_reg_raw(i.a(), self.program.constants[i.bx() as usize]),

            // Unary operations
            Opcode::NOT => self.set_reg(i.a(), !self.reg::<u64>(i.b())),
            Opcode::BNOT => self.set_reg(i.a(), !self.reg::<bool>(i.b())),
            Opcode::INEG => self.set_reg(i.a(), -self.reg::<i64>(i.b())),
            Opcode::FNEG => self.set_reg(i.a(), -self.reg::<f64>(i.b())),

            // Signed/unsigned integer arithmetic
            Opcode::ADD => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a.wrapping_add(b));
            }
            Opcode::SUB => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a.wrapping_sub(b));
            }
            Opcode::MUL => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a.wrapping_mul(b));
            }

            // Signed/unsigned integer division
            Opcode::SDIV => {
                let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                self.set_reg(i.a(), a.checked_div(b).ok_or(VMError::DivisionByZero)?);
            }
            Opcode::SREM => {
                let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                self.set_reg(i.a(), a.checked_rem(b).ok_or(VMError::DivisionByZero)?);
            }
            Opcode::UDIV => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a.checked_div(b).ok_or(VMError::DivisionByZero)?);
            }
            Opcode::UREM => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a.checked_rem(b).ok_or(VMError::DivisionByZero)?);
            }

            // Floating point arithmetic
            Opcode::FADD => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a + b);
            }
            Opcode::FSUB => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a - b);
            }
            Opcode::FMUL => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a * b);
            }
            Opcode::FDIV => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a / b);
            }
            Opcode::FREM => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a % b);
            }

            Opcode::EQ => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a == b);
            }
            Opcode::NE => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a != b);
            }

            Opcode::IGT => {
                let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                self.set_reg(i.a(), a > b);
            }
            Opcode::ILT => {
                let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                self.set_reg(i.a(), a < b);
            }
            Opcode::IGE => {
                let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                self.set_reg(i.a(), a >= b);
            }
            Opcode::ILE => {
                let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                self.set_reg(i.a(), a <= b);
            }

            Opcode::UGT => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a > b);
            }
            Opcode::ULT => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a < b);
            }
            Opcode::UGE => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a >= b);
            }
            Opcode::ULE => {
                let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                self.set_reg(i.a(), a <= b);
            }

            Opcode::FEQ => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a == b);
            }
            Opcode::FNE => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a != b);
            }
            Opcode::FGT => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a > b);
            }
            Opcode::FLT => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a < b);
            }
            Opcode::FGE => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a >= b);
            }
            Opcode::FLE => {
                let (a, b) = self.two_reg::<f64>(i.b(), i.c());
                self.set_reg(i.a(), a <= b);
            }

            // Jumps
            Opcode::JMP => self.pc = i.ax() as usize,
            Opcode::JMP_T => {
                if self.reg::<bool>(i.a()) {
                    self.pc = i.bx() as usize;
                }
            }
            Opcode::JMP_F => {
                if !self.reg::<bool>(i.a()) {
                    self.pc = i.bx() as usize;
                }
            }

            // Calls
            Opcode::CALL => {
                let func = self.program.functions[i.bx() as usize];
                match func.callable {
                    Callable::Vm(entry) => self.call_vm(i.a(), entry, func.nreg),
                    Callable::Host(host) => self.call_host(i.a(), host, func.nreg)?,
                }
            }
            Opcode::CALLR => {
                let id = self.reg::<FunctionId>(i.b());
                let func = self.program.functions[id as usize];
                match func.callable {
                    Callable::Vm(entry) => self.call_vm(i.a(), entry, func.nreg),
                    Callable::Host(host) => self.call_host(i.a(), host, func.nreg)?,
                }
            }
            Opcode::RET => {
                let frame = self.pop_frame()?;
                self.pc = frame.ret_pc;
                self.base = frame.ret_base;
            }

            Opcode::LOAD => {
                let (ptr, off) = self.two_reg::<usize>(i.b(), i.c());
                let val = self.memory.load_value(ptr + off)?;
                self.set_reg_raw(i.a(), val);
            }
            Opcode::STORE => {
                let (ptr, off) = self.two_reg::<usize>(i.a(), i.b());
                let val = self.reg_raw(i.c());
                self.memory.store_value(ptr + off, val)?;
            }

            Opcode::HALT => return Ok(()),

            op => return Err(VMError::UnknownOp(op)),
        }

        Ok(())
    }

    #[inline(always)]
    pub fn raise_interrupt(&mut self, interrupt: Interrupt) {
        self.pending_interrupt = Some(interrupt);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Interrupt {
    Timer,
    Io,
    Trap,
    Fault,
}
