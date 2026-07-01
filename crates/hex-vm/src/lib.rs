#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
mod disassemble;
mod error;
mod host;
mod instruction;
mod native;
mod program;
mod storage;
mod value;

#[cfg(feature = "alloc")]
pub use disassemble::disassemble;
pub use error::*;
pub use host::{Flow, Host, HostCtx, Syscode};
pub use instruction::*;
pub use native::{NativeCtx, NativeFn, NativeOutcome};
pub use program::*;
pub use storage::{Slab, max_frames};
pub use value::{AsWord, word};

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum RunOutcome {
    /// HALT or top-level RET. Returns are in registers.
    Completed,
    /// A syscall suspended execution. Resumable: call run() again.
    Suspended,
    /// The program faulted. The language decides what to do next.
    Trapped(Fault),
}

#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub ret_pc: usize,
    pub ret_base: usize,
    pub caller: FunctionId,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            ret_pc: usize::MAX,
            ret_base: usize::MAX,
            caller: FunctionId::MAX,
        }
    }
}

/// A register-based VM over caller-provided storage.
///
/// `R` is the register file and `F` is the call-frame stack. Both are
/// fixed-capacity slabs (`Vec`, `[T; N]`, `&mut [T]`, ...); the VM never
/// grows them, it bounds-checks and raises [`Fault::StackOverflow`].
/// `registers.len()` is the maximum stack size.
#[derive(Debug, Clone)]
pub struct VM<R, F> {
    pub registers: R,
    frames: F,
    frame_top: usize,
    pc: usize,
    base: usize,
    curr_func: FunctionId,
}

/// Owned, heap-backed VM. Only available with the `alloc` feature.
#[cfg(feature = "alloc")]
pub type HeapVM = VM<Vec<word>, Vec<Frame>>;

#[cfg(feature = "alloc")]
impl HeapVM {
    /// One-line setup: pick a register budget; frames are sized automatically.
    pub fn new(reg_cap: usize) -> Self {
        Self::from_parts(
            alloc::vec![0 as word; reg_cap],
            alloc::vec![Frame::default(); max_frames(reg_cap)],
        )
    }

    pub fn from_entry(program: &Program, func: FunctionId, args: &[word], reg_cap: usize) -> Result<Self, Error> {
        let mut vm = Self::new(reg_cap);
        vm.set_entry(program, func, args)?;
        Ok(vm)
    }
}
impl<R, F> Default for VM<R, F>
where
    R: Default,
    F: Default,
{
    fn default() -> Self {
        Self {
            registers: Default::default(),
            frames: Default::default(),
            frame_top: 0,
            pc: usize::MAX,
            base: usize::MAX,
            curr_func: FunctionId::MAX,
        }
    }
}

impl<R: Slab<word>, F: Slab<Frame>> VM<R, F> {
    pub fn from_parts(registers: R, frames: F) -> Self {
        Self {
            registers,
            frames,
            frame_top: 0,
            pc: usize::MAX,
            base: usize::MAX,
            curr_func: FunctionId::MAX,
        }
    }

    pub fn set_entry(&mut self, program: &Program, entry: FunctionId, args: &[word]) -> Result<(), Error> {
        let func = program.function(entry);
        let argc = args.len();

        if argc != func.narg as usize {
            return Err(Error::ArgcMismatch { exp: func.narg, got: argc as Reg });
        }

        self.reset();

        let regs = self.registers.slots_mut();

        regs[..func.nreg as usize].fill(0);
        regs[..args.len()].copy_from_slice(&args);

        self.pc = func.ty.entry_pc()?;
        self.base = 0;
        self.curr_func = entry;
        Ok(())
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.frame_top = 0;
        self.pc = 0;
        self.base = 0;
        self.curr_func = FunctionId::MAX;
    }

    #[inline]
    pub fn pc(&self) -> usize {
        self.pc
    }

    #[inline]
    pub fn base(&self) -> usize {
        self.base
    }

    #[inline(always)]
    fn reg_raw(&self, reg: Reg) -> word {
        self.registers.slots()[self.base + reg as usize]
    }

    #[inline(always)]
    fn reg<T: AsWord>(&self, reg: Reg) -> T {
        T::from_word(self.registers.slots()[self.base + reg as usize])
    }

    #[inline(always)]
    fn set_reg(&mut self, reg: Reg, value: impl AsWord) {
        self.registers.slots_mut()[self.base + reg as usize] = value.into_word();
    }

    #[inline(always)]
    fn set_reg_raw(&mut self, reg: Reg, value: word) {
        self.registers.slots_mut()[self.base + reg as usize] = value;
    }

    #[inline(always)]
    fn two_reg<T: AsWord>(&self, reg_a: Reg, reg_b: Reg) -> (T, T) {
        (self.reg(reg_a), self.reg(reg_b))
    }
}

#[inline(always)]
pub fn run<R, F, H>(vm: &mut VM<R, F>, program: &Program, host: &mut H, memory: &mut [u8]) -> Result<RunOutcome, Error>
where
    R: Slab<word>,
    F: Slab<Frame>,
    H: Host,
{
    while vm.pc < program.len() {
        let i = program.instruction(vm.pc);
        vm.pc += 1;

        // Compare and branch: jump to the next instruction if condition is false.
        macro_rules! cmp_branch {
            ($cond:expr) => {
                vm.pc = if $cond { program.instruction(vm.pc) as usize } else { vm.pc + 1 };
            };
        }

        // Raise a fault: unwind to a handler, or exit run if uncaught.
        macro_rules! trap {
            ($f:expr) => {{
                let f = $f;
                match unwind(vm, program, fault_to_value(f), f) {
                    // caught: resume at handler
                    Ok(()) => {}
                    // uncaught: terminal
                    Err(fault) => return Ok(RunOutcome::Trapped(fault)),
                }
            }};

            ($f:expr, $v:expr) => {{
                match unwind(vm, program, $v, $f) {
                    // caught: resume at handler
                    Ok(()) => {}
                    // uncaught: terminal
                    Err(fault) => return Ok(RunOutcome::Trapped(fault)),
                }
            }};
        }

        // Push a return frame, trapping on frame-stack overflow.
        macro_rules! push_frame {
            () => {{
                if vm.frame_top >= vm.frames.len() {
                    trap!(Fault::StackOverflow);
                }

                vm.frames.slots_mut()[vm.frame_top] = Frame {
                    ret_pc: vm.pc,
                    ret_base: vm.base,
                    caller: vm.curr_func,
                };
                vm.frame_top += 1;
            }};
        }

        // Pop a return frame; if the stack is empty the program has completed.
        macro_rules! pop_frame {
            () => {{
                if vm.frame_top == 0 {
                    return Ok(RunOutcome::Completed);
                }
                vm.frame_top -= 1;
                &vm.frames.slots()[vm.frame_top]
            }};
        }

        // Call a function: push a frame and set pc/func/base accordingly.
        macro_rules! call {
            ($id:expr) => {{
                let func_id = $id;
                let func = program.function(func_id);
                let base = vm.base + inst::a(i) as usize;
                match func.ty {
                    FnType::Hxvm { entry_pc } => {
                        let last = base + func.nreg as usize;
                        if last > vm.registers.len() {
                            trap!(Fault::StackOverflow);
                        }
                        push_frame!();
                        vm.curr_func = func_id;
                        vm.base = base;
                        vm.pc = entry_pc;
                    }
                    FnType::Host { syscode } => {
                        let ctx = HostCtx::new(vm.registers.slots_mut(), base, func.narg, func.nret);
                        match host.syscall(syscode, ctx)? {
                            Flow::Suspend => return Ok(RunOutcome::Suspended),
                            Flow::Trap(f) => trap!(f),
                            Flow::Continue => {}
                        }
                    }
                    FnType::Native { fn_ptr } => {
                        let max = func.narg.max(func.nret) as usize;
                        let window = &mut vm.registers.slots_mut()[base..base + max];
                        match fn_ptr(NativeCtx { window, memory }) {
                            NativeOutcome::Continue => {}
                            NativeOutcome::Trap(f) => trap!(f),
                        }
                    }
                }
            }};
            ($id:expr, tail) => {{
                let id = $id;
                let func = program.function(id);
                let base = vm.base;
                let src = vm.base + inst::a(i) as usize;
                let regs = vm.registers.slots_mut();
                regs.copy_within(src..src + func.narg as usize, base);

                match func.ty {
                    FnType::Hxvm { entry_pc } => {
                        let last = vm.base + func.nreg as usize;
                        if last > regs.len() {
                            trap!(Fault::StackOverflow);
                        }
                        vm.curr_func = id;
                        vm.pc = entry_pc;
                    }
                    FnType::Host { syscode } => {
                        let ctx = HostCtx::new(regs, base, func.narg, func.nret);
                        match host.syscall(syscode, ctx)? {
                            Flow::Suspend => return Ok(RunOutcome::Suspended),
                            Flow::Trap(f) => trap!(f),
                            Flow::Continue => {
                                let fr = pop_frame!();
                                vm.pc = fr.ret_pc;
                                vm.base = fr.ret_base;
                                vm.curr_func = fr.caller;
                            }
                        }
                    }
                    FnType::Native { fn_ptr } => {
                        let max = func.narg.max(func.nret) as usize;
                        let window = &mut vm.registers.slots_mut()[base..base + max];
                        match fn_ptr(NativeCtx { window, memory }) {
                            NativeOutcome::Trap(f) => trap!(f),
                            NativeOutcome::Continue => {
                                let fr = pop_frame!();
                                vm.pc = fr.ret_pc;
                                vm.base = fr.ret_base;
                                vm.curr_func = fr.caller;
                            }
                        }
                    }
                }
            }};
        }

        match inst::op(i) {
            // Moves
            Opcode::COPY => vm.set_reg_raw(inst::a(i), vm.reg_raw(inst::b(i))),
            Opcode::LOADK => vm.set_reg_raw(inst::a(i), program.constant(inst::bx(i) as ConstantId)),
            Opcode::LOADI => vm.set_reg(inst::a(i), inst::bx_imm(i)),
            Opcode::LOADF => vm.set_reg(inst::a(i), inst::bx_imm(i) as f64),

            // Unary operations
            Opcode::NOT => vm.set_reg(inst::a(i), !vm.reg::<u64>(inst::b(i))),
            Opcode::BNOT => vm.set_reg(inst::a(i), !vm.reg::<bool>(inst::b(i))),
            Opcode::INEG => vm.set_reg(inst::a(i), -vm.reg::<i64>(inst::b(i))),
            Opcode::FNEG => vm.set_reg(inst::a(i), -vm.reg::<f64>(inst::b(i))),

            // Signed/unsigned integer arithmetic
            Opcode::ADD => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a.wrapping_add(b));
            }
            Opcode::SUB => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a.wrapping_sub(b));
            }
            Opcode::MUL => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a.wrapping_mul(b));
            }
            Opcode::ADDI => {
                let src = vm.reg::<u64>(inst::b(i));
                vm.set_reg(inst::a(i), src.wrapping_add(inst::imm8(i) as u64));
            }
            Opcode::SUBI => {
                let src = vm.reg::<u64>(inst::b(i));
                vm.set_reg(inst::a(i), src.wrapping_sub(inst::imm8(i) as u64));
            }
            Opcode::MULI => {
                let src = vm.reg::<u64>(inst::b(i));
                vm.set_reg(inst::a(i), src.wrapping_mul(inst::imm8(i) as u64));
            }
            Opcode::ADDK => {
                let a = vm.reg::<word>(inst::b(i));
                let k = u64::from_word(program.constant(inst::c(i) as ConstantId));
                vm.set_reg(inst::a(i), a.wrapping_add(k));
            }
            Opcode::SUBK => {
                let a = vm.reg::<word>(inst::b(i));
                let k = u64::from_word(program.constant(inst::c(i) as ConstantId));
                vm.set_reg(inst::a(i), a.wrapping_sub(k));
            }
            Opcode::MULK => {
                let a = vm.reg::<word>(inst::b(i));
                let k = u64::from_word(program.constant(inst::c(i) as ConstantId));
                vm.set_reg(inst::a(i), a.wrapping_mul(k));
            }

            // Float arithmetic with constant-pool operand
            Opcode::FADDK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = f64::from_word(program.constant(inst::c(i) as ConstantId));
                vm.set_reg(inst::a(i), a + k);
            }
            Opcode::FSUBK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = f64::from_word(program.constant(inst::c(i) as ConstantId));
                vm.set_reg(inst::a(i), a - k);
            }
            Opcode::FMULK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = f64::from_word(program.constant(inst::c(i) as ConstantId));
                vm.set_reg(inst::a(i), a * k);
            }
            Opcode::FDIVK => {
                let a = vm.reg::<f64>(inst::b(i));
                let k = f64::from_word(program.constant(inst::c(i) as ConstantId));
                vm.set_reg(inst::a(i), a / k);
            }

            // Signed/unsigned integer division
            Opcode::SDIV => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                match a.checked_div(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => trap!(Fault::DivisionByZero),
                }
            }
            Opcode::SREM => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                match a.checked_rem(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => trap!(Fault::DivisionByZero),
                }
            }
            Opcode::UDIV => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                match a.checked_div(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => trap!(Fault::DivisionByZero),
                }
            }
            Opcode::UREM => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                match a.checked_rem(b) {
                    Some(v) => vm.set_reg(inst::a(i), v),
                    None => trap!(Fault::DivisionByZero),
                }
            }

            // Floating point arithmetic
            Opcode::FADD => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a + b);
            }
            Opcode::FSUB => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a - b);
            }
            Opcode::FMUL => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a * b);
            }
            Opcode::FDIV => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a / b);
            }
            Opcode::FREM => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a % b);
            }

            Opcode::EQ => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a == b);
            }
            Opcode::NE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a != b);
            }

            Opcode::SGT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a > b);
            }
            Opcode::SLT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a < b);
            }
            Opcode::SGE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a >= b);
            }
            Opcode::SLE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a <= b);
            }

            Opcode::UGT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a > b);
            }
            Opcode::ULT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a < b);
            }
            Opcode::UGE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a >= b);
            }
            Opcode::ULE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a <= b);
            }

            Opcode::FEQ => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a == b);
            }
            Opcode::FNE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a != b);
            }
            Opcode::FGT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a > b);
            }
            Opcode::FLT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a < b);
            }
            Opcode::FGE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a >= b);
            }
            Opcode::FLE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                vm.set_reg(inst::a(i), a <= b);
            }

            // Jumps
            Opcode::JMP => {
                vm.pc = inst::ax(i) as usize;
            }
            Opcode::JMP_T => {
                if vm.reg::<bool>(inst::a(i)) {
                    vm.pc = inst::bx(i) as usize;
                }
            }
            Opcode::JMP_F => {
                if !vm.reg::<bool>(inst::a(i)) {
                    vm.pc = inst::bx(i) as usize;
                }
            }
            Opcode::JEQ => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch!(a == b);
            }
            Opcode::JNE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch!(a != b);
            }
            Opcode::JSLT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch!(a < b);
            }
            Opcode::JSGT => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch!(a > b);
            }
            Opcode::JSLE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch!(a <= b);
            }
            Opcode::JSGE => {
                let (a, b) = vm.two_reg::<i64>(inst::b(i), inst::c(i));
                cmp_branch!(a >= b);
            }
            Opcode::JULT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch!(a < b);
            }
            Opcode::JUGT => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch!(a > b);
            }
            Opcode::JULE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch!(a <= b);
            }
            Opcode::JUGE => {
                let (a, b) = vm.two_reg::<u64>(inst::b(i), inst::c(i));
                cmp_branch!(a >= b);
            }
            Opcode::JFEQ => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch!(a == b);
            }
            Opcode::JFNE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch!(a != b);
            }
            Opcode::JFLT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch!(a < b);
            }
            Opcode::JFGT => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch!(a > b);
            }
            Opcode::JFLE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch!(a <= b);
            }
            Opcode::JFGE => {
                let (a, b) = vm.two_reg::<f64>(inst::b(i), inst::c(i));
                cmp_branch!(a >= b);
            }

            // Memory ops
            Opcode::LOAD => {
                let (ptr, off) = vm.two_reg::<word>(inst::b(i), inst::c(i));
                let start = (ptr + off) as usize;
                match memory.get(start..start + size_of::<word>()) {
                    Some(slice) => {
                        let mut b = [0u8; 8];
                        b.copy_from_slice(slice);
                        vm.set_reg_raw(inst::a(i), word::from_le_bytes(b));
                    }
                    None => trap!(Fault::MemoryOOB),
                }
            }
            Opcode::STORE => {
                let (ptr, off) = vm.two_reg::<word>(inst::b(i), inst::c(i));
                let start = (ptr + off) as usize;
                match memory.get_mut(start..start + size_of::<word>()) {
                    Some(slice) => {
                        let val = vm.reg_raw(inst::a(i));
                        let bytes = word::to_le_bytes(val);
                        slice.copy_from_slice(&bytes);
                    }
                    None => trap!(Fault::MemoryOOB),
                }
            }

            // Calls
            Opcode::CALL => call!(inst::bx(i) as FunctionId),
            Opcode::TCALL => call!(inst::bx(i) as FunctionId, tail),
            Opcode::CALL_IND => call!(vm.reg::<FunctionId>(inst::b(i))),
            Opcode::TCALL_IND => call!(vm.reg::<FunctionId>(inst::b(i)), tail),

            Opcode::RET => {
                let fr = pop_frame!();
                vm.pc = fr.ret_pc;
                vm.base = fr.ret_base;
                vm.curr_func = fr.caller;
            }

            Opcode::THROW => trap!(Fault::Uncaught, vm.reg_raw(inst::a(i))),
            Opcode::HALT => return Ok(RunOutcome::Completed),

            op => return Err(Error::UnknownOp(op)),
        }
    }
    Err(Error::PcOOB)
}

/// Unwind from the current pc carrying `thrown`. On finding a handler, sets
/// pc/base/cur_func to resume at the handler and returns Ok(()). If no handler
/// exists anywhere on the stack, returns Err(outcome) to exit `run`.
fn unwind<R, F>(vm: &mut VM<R, F>, program: &Program, thrown: word, uncaught: Fault) -> Result<(), Fault>
where
    R: Slab<word>,
    F: Slab<Frame>,
{
    let mut check_pc = vm.pc - 1;

    loop {
        if let Some(h) = program.handler_for(vm.curr_func, check_pc) {
            vm.set_reg_raw(h.catch_reg, thrown);
            vm.pc = h.handler_pc;
            return Ok(()); // Resume dispatch at handler; base unchanged (this frame)
        }

        if vm.frame_top == 0 {
            // stack empty, no handler anywhere
            return Err(uncaught);
        }

        // pop frames until we find a handler or the stack is empty
        vm.frame_top -= 1;
        let frame = vm.frames.slots()[vm.frame_top - 1];
        vm.base = frame.ret_base;
        vm.pc = frame.ret_pc;
        vm.curr_func = frame.caller;
        check_pc = frame.ret_pc - 1;
    }
}

#[inline]
fn fault_to_value(f: Fault) -> word {
    match f {
        Fault::Uncaught => 0,
        Fault::DivisionByZero => 1,
        Fault::MemoryOOB => 2,
        Fault::StackOverflow => 3,
        Fault::Abort(c) => 0x100 | c as u64,
    }
    .into_word()
}
