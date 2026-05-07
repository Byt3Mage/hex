use crate::extensions::{DispatchResult, ExtensionOps, NoExtensions};

pub mod disassemble;
pub mod extensions;
pub mod instruction;
pub mod program;

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

#[derive(Debug, thiserror::Error)]
pub enum VMError {
    #[error("Illegal opcode: {0}")]
    UnknownOp(Opcode),
    #[error("Invalid argument count: expected {exp}, got {got}")]
    InvalidArgCount { exp: Reg, got: Reg },
    #[error("Program counter out of bounds")]
    PCOutOfBounds,
}

pub type VMResult<T> = Result<T, VMError>;

struct CallerInfo {
    base: usize,
    ret_pc: usize,
}

pub struct Frame {
    func: Function,
    caller_info: CallerInfo,
}

/// Everything an extension op or runtime can manipulate.
pub struct VMState {
    pub regs: Vec<Value>,
    pub call_stack: Vec<Frame>,
    pub pc: usize,
    pub base: usize,
}

impl VMState {
    #[inline(always)]
    pub fn reg<T: IsValue>(&self, reg: Reg) -> T {
        T::from_value(self.regs[self.base + reg as usize])
    }

    #[inline(always)]
    pub fn reg_raw(&self, reg: Reg) -> Value {
        self.regs[self.base + reg as usize]
    }

    #[inline(always)]
    pub fn reg_mut(&mut self, reg: Reg) -> &mut Value {
        &mut self.regs[self.base + reg as usize]
    }

    #[inline(always)]
    pub fn set_reg(&mut self, reg: Reg, value: impl IsValue) {
        self.regs[self.base + reg as usize] = value.into_value();
    }

    #[inline(always)]
    pub fn set_reg_raw(&mut self, reg: Reg, value: Value) {
        self.regs[self.base + reg as usize] = value;
    }
}

pub struct VM<'p, E: ExtensionOps = NoExtensions> {
    state: VMState,
    extensions: E,
    program: &'p Program,
}

impl<'p, E: ExtensionOps> VM<'p, E> {
    pub fn new(program: &'p Program, extensions: E) -> Self {
        Self {
            state: VMState {
                regs: Vec::new(),
                call_stack: Vec::new(),
                pc: 0,
                base: 0,
            },
            extensions,
            program,
        }
    }

    pub fn reset(&mut self) {
        self.state.base = 0;
        self.state.pc = 0;
        self.state.regs.clear();
        self.state.call_stack.clear();
    }

    #[inline(always)]
    fn reg<T: IsValue>(&self, reg: Reg) -> T {
        self.state.reg(reg)
    }

    #[inline(always)]
    fn reg_raw(&self, reg: Reg) -> Value {
        self.state.reg_raw(reg)
    }

    #[inline(always)]
    fn set_reg(&mut self, reg: Reg, value: impl IsValue) {
        self.state.set_reg(reg, value);
    }

    #[inline(always)]
    fn set_reg_raw(&mut self, reg: Reg, value: Value) {
        self.state.set_reg_raw(reg, value);
    }

    #[inline(always)]
    fn two_reg<T: IsValue>(&self, reg_a: Reg, reg_b: Reg) -> (T, T) {
        (self.reg(reg_a), self.reg(reg_b))
    }

    #[inline(always)]
    fn reg_offset(&self, reg: Reg) -> usize {
        self.state.base + reg as usize
    }

    #[inline(always)]
    fn call(&mut self, ret_reg: Reg, func: Function) {
        // Call convention: args are in caller registers[Rret..Rn]
        //
        // We use overlapping register windows for caller and callee
        // to avoid copying arguments. This is safe because argument
        // registers are not clobbered by caller until callee returns.

        // Callee window starts at caller's return register.
        let base = self.reg_offset(ret_reg);
        let last = base + func.nreg as usize;

        // Grow regs to fit callee's full register count beyond the arg base.
        if last > self.state.regs.len() {
            self.state.regs.resize(last, Value::ZERO);
        }

        // Create new call frame and save caller info
        self.state.call_stack.push(Frame {
            func,
            caller_info: CallerInfo {
                ret_pc: self.state.pc,
                base: self.state.base,
            },
        });

        // Jump to callee code
        self.state.base = base;
        self.state.pc = func.entry_pc;
    }

    #[inline(always)]
    fn callh(&mut self, ret_reg: Reg, func: HostFunction) -> VMResult<()> {
        // Host function call convention: args and rets use the same window.
        // - args are read from  [Rret...Rnarg]
        // - rets are written to [Rret..Rnret]
        //
        // Up to callee not to clobber args before reading from them.
        // Garbage values or crash if args are read out of bounds.
        let ret = self.reg_offset(ret_reg);
        (func.func)(&mut self.state.regs[ret..ret + func.nreg as usize])
    }

    fn exec_callt(&mut self, i: Instruction) {
        let func = self.program.functions[i.bx() as usize];
        let last = self.reg_offset(func.nreg);

        // Grow regs to fit callee's full register count beyond the arg base.
        if last > self.state.regs.len() {
            self.state.regs.resize(last, Value::ZERO);
        }

        // copy args into the same window
        let start = self.reg_offset(i.a());
        let end = start + func.narg as usize;
        self.state.regs.copy_within(start..end, self.state.base);

        // Reuse current frame and update the callee info.
        // If we are top level function, nret must be the same
        // for caller and tail callee, so we don't need to update
        // callee info.
        if let Some(frame) = self.state.call_stack.last_mut() {
            frame.func = func;
            todo!("validate argument above")
        }

        self.state.pc = func.entry_pc;
    }

    fn run(&mut self) -> VMResult<()> {
        while self.state.pc < self.program.instructions.len() {
            let i = self.program.instructions[self.state.pc];
            self.state.pc += 1;

            match i.op() {
                // Moves
                Opcode::MOV => self.set_reg_raw(i.a(), self.reg_raw(i.b())),
                Opcode::CONST => self.set_reg_raw(i.a(), self.program.constants[i.bx() as usize]),

                // Unary operations
                Opcode::INOT => self.set_reg(i.a(), !self.reg::<i64>(i.b())),
                Opcode::UNOT => self.set_reg(i.a(), !self.reg::<u64>(i.b())),
                Opcode::INEG => self.set_reg(i.a(), -self.reg::<i64>(i.b())),
                Opcode::FNEG => self.set_reg(i.a(), -self.reg::<f64>(i.b())),
                Opcode::BNOT => self.set_reg(i.a(), !self.reg::<bool>(i.b())),

                // Signed integer arithmetic
                Opcode::IADD => {
                    let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_add(b));
                }
                Opcode::ISUB => {
                    let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_sub(b));
                }
                Opcode::IMUL => {
                    let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_mul(b));
                }
                Opcode::IDIV => {
                    let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_div(b));
                }
                Opcode::IREM => {
                    let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_rem(b));
                }

                // Unsigned integer arithmetic
                Opcode::UADD => {
                    let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_add(b));
                }
                Opcode::USUB => {
                    let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_sub(b));
                }
                Opcode::UMUL => {
                    let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                    self.set_reg(i.a(), a.wrapping_mul(b));
                }
                Opcode::UDIV => {
                    let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                    self.set_reg(i.a(), a / b);
                }
                Opcode::UREM => {
                    let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                    self.set_reg(i.a(), a % b);
                }

                // Float arithmetic
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

                // Signed integer comparisons
                Opcode::IEQ => {
                    let (a, b) = self.two_reg::<i64>(i.b(), i.c());
                    self.set_reg(i.a(), a == b);
                }
                Opcode::INE => {
                    let (a, b) = self.two_reg::<i64>(i.b(), i.c());
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

                // Unsigned integer comparisons
                Opcode::UEQ => {
                    let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                    self.set_reg(i.a(), a == b);
                }
                Opcode::UNE => {
                    let (a, b) = self.two_reg::<u64>(i.b(), i.c());
                    self.set_reg(i.a(), a != b);
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

                // Floating point comparisons
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
                Opcode::JMP => self.state.pc = i.ax() as usize,
                Opcode::JMP_T => {
                    if self.reg::<bool>(i.a()) {
                        self.state.pc = i.bx() as usize;
                    }
                }
                Opcode::JMP_F => {
                    if !self.reg::<bool>(i.a()) {
                        self.state.pc = i.bx() as usize;
                    }
                }

                // Calls
                Opcode::CALL => {
                    self.call(i.a(), self.program.functions[i.bx() as usize]);
                }
                Opcode::CALLR => {
                    let func = self.reg::<FunctionPtr>(i.b());
                    self.call(i.a(), self.program.functions[func as usize]);
                }
                Opcode::CALLH => {
                    self.callh(i.a(), self.program.host_functions[i.bx() as usize])?;
                }
                Opcode::CALLHR => {
                    let func = self.reg::<FunctionPtr>(i.b());
                    self.callh(i.a(), self.program.host_functions[func as usize])?;
                }
                Opcode::CALLT => {
                    self.exec_callt(i);
                }
                Opcode::RET => match self.state.call_stack.pop() {
                    Some(frame) => {
                        self.state.base = frame.caller_info.base;
                        self.state.pc = frame.caller_info.ret_pc;
                    }
                    None => return Ok(()),
                },

                Opcode::HALT => return Ok(()),

                op => match self.extensions.dispatch(op, i, &mut self.state)? {
                    DispatchResult::Halt => return Ok(()),
                    DispatchResult::Continue => {}
                },
            }
        }

        Err(VMError::PCOutOfBounds)
    }

    pub fn execute(&mut self, entry: FunctionPtr, args: &[Value]) -> VMResult<&[Value]> {
        let func = self.program.functions[entry as usize];
        let argc = args.len() as Reg;

        if func.narg != argc {
            return Err(VMError::InvalidArgCount {
                exp: func.narg,
                got: argc,
            });
        }

        self.reset();
        self.state.regs.resize(func.nreg as usize, Value::ZERO);
        self.state.regs[..args.len()].copy_from_slice(args);
        self.state.base = 0;
        self.state.pc = func.entry_pc;

        self.run()?;
        Ok(&self.state.regs[..func.nret as usize])
    }
}
