use thiserror::Error;

use crate::vm::{
    async_runtime::{Scheduler, Task, TaskState},
    heap::{GCPtr, Heap},
    instruction::*,
    memory::PageAllocator,
    object::{AsValue, GCBuffer, GCTask, Value, try_get_ptr},
    program::{CallInfo, FunctionPtr, NativeFunctionInfo, Program},
};

mod async_runtime;
pub mod function;
mod heap;
pub mod instruction;
mod memory;
pub mod name;
pub mod object;
pub mod program;

#[derive(Debug, Copy, Clone, Error)]
pub enum VMError {
    #[error("Illegal opcode received: {0:?}")]
    IllegalOp(Opcode),
    #[error("Operation stack expected, but the call stack is empty")]
    EmptyCallStack,
    #[error("Invalid argument count: expected {exp}, got {got}")]
    InvalidArgCount { exp: u8, got: u8 },
    #[error("Attempted to await in a cancelled async task")]
    TaskCancelled,
    #[error("Await instruction received outside of an async task")]
    IllegalAwait,
    #[error("Program counter out of bounds")]
    PCOutOfBounds,
    #[error("Invalid conversion")]
    ValueConversionFailed,
    #[error("Heap allocation failed")]
    AllocFailed,
}

pub type VMResult<T> = Result<T, VMError>;

struct CallerInfo {
    /// PC in caller instructions to return to
    ret_pc: usize,

    /// Caller registers base offset
    base_reg: usize,

    /// Start register in caller to put return value
    ret_reg: Reg,
}

struct Frame {
    /// Caller function metadata.
    /// `None` means top-level function with no caller
    caller_info: Option<CallerInfo>,

    /// Current function metadata,
    callee_info: CallInfo,
}

pub struct VM<'a, A: PageAllocator> {
    regs: Vec<Value>,
    call_stack: Vec<Frame>,
    native_call_ret: Box<[Value; u8::MAX as usize]>,
    heap: Heap<A>,
    scheduler: Scheduler,

    pc: usize,
    base_reg: usize,

    program: &'a Program,
}

impl<'a, A: PageAllocator> VM<'a, A> {
    pub fn reset(&mut self) {
        self.base_reg = 0;
        self.pc = 0;

        self.regs.clear();
        self.call_stack.clear();
        self.heap.reset();
        self.scheduler.reset();
    }

    #[inline(always)]
    fn reg(&self, reg: Reg) -> Value {
        self.regs[self.base_reg + reg.index()]
    }

    #[inline(always)]
    fn reg_mut(&mut self, reg: Reg) -> &mut Value {
        &mut self.regs[self.base_reg + reg.index()]
    }

    #[inline(always)]
    fn set_reg(&mut self, reg: Reg, value: impl AsValue) {
        self.regs[self.base_reg + reg.index()] = value.into_value();
    }

    #[inline(always)]
    fn set_reg_raw(&mut self, reg: Reg, value: Value) {
        self.regs[self.base_reg + reg.index()] = value;
    }

    #[inline(always)]
    fn two_reg<T: AsValue>(&self, reg_a: Reg, reg_b: Reg) -> (T, T) {
        (self.reg(reg_a).get(), self.reg(reg_b).get())
    }

    #[inline(always)]
    fn last_frame_mut(&mut self) -> VMResult<&mut Frame> {
        self.call_stack.last_mut().ok_or(VMError::EmptyCallStack)
    }

    #[inline(always)]
    fn exec_mov(&mut self, i: Instruction) {
        self.set_reg_raw(i.a(), self.reg(i.b()));
    }

    #[inline(always)]
    fn exec_const(&mut self, i: Instruction) {
        self.set_reg_raw(i.a(), self.program.constants[i.bx() as usize]);
    }

    #[inline(always)]
    fn exec_bnot(&mut self, i: Instruction) {
        let val: bool = self.reg(i.b()).get();
        self.set_reg(i.a(), !val)
    }

    #[inline(always)]
    fn exec_inot(&mut self, i: Instruction) {
        let val: i64 = self.reg(i.b()).get();
        self.set_reg(i.a(), !val)
    }

    #[inline(always)]
    fn exec_unot(&mut self, i: Instruction) {
        let val: u64 = self.reg(i.b()).get();
        self.set_reg(i.a(), !val)
    }

    #[inline(always)]
    fn exec_ineg(&mut self, i: Instruction) {
        let val: i64 = self.reg(i.b()).get();
        self.set_reg(i.a(), -val)
    }

    #[inline(always)]
    fn exec_fneg(&mut self, i: Instruction) {
        let val: f64 = self.reg(i.b()).get();
        self.set_reg(i.a(), -val)
    }

    #[inline(always)]
    fn exec_iadd(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a + b);
    }

    #[inline(always)]
    fn exec_isub(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a - b);
    }

    #[inline(always)]
    fn exec_imul(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a * b);
    }

    #[inline(always)]
    fn exec_idiv(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a / b);
    }

    #[inline(always)]
    fn exec_irem(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a & b);
    }

    #[inline(always)]
    fn exec_uadd(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a + b);
    }

    #[inline(always)]
    fn exec_usub(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a - b);
    }

    #[inline(always)]
    fn exec_umul(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a * b);
    }

    #[inline(always)]
    fn exec_udiv(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a / b);
    }

    #[inline(always)]
    fn exec_urem(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a % b);
    }

    #[inline(always)]
    fn exec_fadd(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<f64>(i.b(), i.c());
        self.set_reg(i.a(), a + b);
    }

    #[inline(always)]
    fn exec_fsub(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<f64>(i.b(), i.c());
        self.set_reg(i.a(), a - b);
    }

    #[inline(always)]
    fn exec_fmul(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<f64>(i.b(), i.c());
        self.set_reg(i.a(), a * b);
    }

    #[inline(always)]
    fn exec_fdiv(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<f64>(i.b(), i.c());
        self.set_reg(i.a(), a / b);
    }

    #[inline(always)]
    fn exec_ieq(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a == b);
    }

    #[inline(always)]
    fn exec_ine(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a != b);
    }

    #[inline(always)]
    fn exec_ilt(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a < b);
    }

    #[inline(always)]
    fn exec_igt(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a > b);
    }

    #[inline(always)]
    fn exec_ile(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a <= b);
    }

    #[inline(always)]
    fn exec_ige(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<i64>(i.b(), i.c());
        self.set_reg(i.a(), a >= b);
    }

    #[inline(always)]
    fn exec_ueq(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a == b);
    }

    #[inline(always)]
    fn exec_une(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a != b);
    }

    #[inline(always)]
    fn exec_ult(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a < b);
    }

    #[inline(always)]
    fn exec_ule(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a <= b);
    }

    #[inline(always)]
    fn exec_ugt(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a > b);
    }

    #[inline(always)]
    fn exec_uge(&mut self, i: Instruction) {
        let (a, b) = self.two_reg::<u64>(i.b(), i.c());
        self.set_reg(i.a(), a >= b);
    }

    #[inline(always)]
    fn exec_jump(&mut self, i: Instruction) {
        self.pc = i.ax() as usize;
    }

    #[inline(always)]
    fn exec_jump_true(&mut self, i: Instruction) {
        let cond = self.reg(i.a()).get::<bool>();
        if cond {
            self.pc = i.bx() as usize;
        }
    }

    #[inline(always)]
    fn exec_jump_false(&mut self, i: Instruction) {
        let cond = self.reg(i.a()).get::<bool>();
        if !cond {
            self.pc = i.bx() as usize;
        }
    }

    #[inline(always)]
    fn call(&mut self, ret_reg: Reg, cinfo: CallInfo) {
        // Create register window
        let base = self.regs.len();
        self.regs.resize(base + cinfo.nreg as usize, Value::zero());

        // Copy args from caller into callee registers
        // Call convention: args are in caller registers[ret..ret + narg]
        let start = self.base_reg + ret_reg.index();
        let end = start + cinfo.narg as usize;
        self.regs.copy_within(start..end, base);

        // Create new call frame and save caller info
        self.call_stack.push(Frame {
            caller_info: Some(CallerInfo {
                ret_pc: self.pc,
                base_reg: self.base_reg,
                ret_reg,
            }),
            callee_info: cinfo,
        });

        // Jump to function code
        self.base_reg = base;
        self.pc = cinfo.entry_pc;
    }

    #[inline(always)]
    fn exec_call(&mut self, i: Instruction) {
        let ret_reg = i.a();
        let cinfo = self.program.functions[i.bx() as usize].call_info;
        self.call(ret_reg, cinfo);
    }

    #[inline(always)]
    fn exec_callr(&mut self, i: Instruction) {
        let ret_reg = i.a();
        let func = self.reg(i.b()).get::<FunctionPtr>() as usize;
        let cinfo = self.program.functions[func as usize].call_info;
        self.call(ret_reg, cinfo);
    }

    fn exec_callt(&mut self, i: Instruction) -> VMResult<()> {
        let ret_reg = i.a();
        let cinfo = self.program.functions[i.bx() as usize].call_info;

        // Reuse register window: shrink or grow in-place
        let base = self.base_reg;
        self.regs.resize(base + cinfo.nreg as usize, Value::zero());

        // copy args into the same window
        let start = self.base_reg + ret_reg.index();
        let end = start + cinfo.narg as usize;
        self.regs.copy_within(start..end, base);

        // Reuse current frame and update the callee info.
        self.last_frame_mut()?.callee_info = cinfo;
        self.pc = cinfo.entry_pc;
        Ok(())
    }

    #[inline(always)]
    fn calln(&mut self, ret_reg: Reg, func: &NativeFunctionInfo) -> VMResult<()> {
        let narg = func.narg as usize;
        let nret = func.nret as usize;

        // Immutably borrow args from caller registers
        // Call convention: args are in caller registers[ret..ret + narg]
        let ret = self.base_reg + ret_reg.index();
        let args = &self.regs[ret..ret + narg];
        let results = &mut self.native_call_ret[..nret];

        // Call native function
        (func.func)(args, results)?;

        // Copy results back into caller registers
        self.regs[ret..ret + nret].copy_from_slice(results);
        Ok(())
    }

    #[inline(always)]
    fn exec_calln(&mut self, i: Instruction) -> VMResult<()> {
        let ret_reg = i.a();
        let func = &self.program.native_functions[i.bx() as usize];
        self.calln(ret_reg, func)
    }

    #[inline(always)]
    fn exec_callnr(&mut self, i: Instruction) -> VMResult<()> {
        let ret_reg = i.a();
        let func_id = self.reg(i.b()).get::<FunctionPtr>() as usize;
        let func = &self.program.native_functions[func_id];
        self.calln(ret_reg, func)
    }

    fn exec_ret(&mut self, i: Instruction) -> VMResult<Option<Frame>> {
        let frame = self.call_stack.pop().ok_or(VMError::EmptyCallStack)?;

        match &frame.caller_info {
            // No caller means top level function, exit run loop
            None => Ok(Some(frame)),

            // Return to caller
            Some(caller_info) => {
                // Copy return values to caller's registers
                let start = self.base_reg + i.a().index();
                let range = start..(start + frame.callee_info.nret as usize);
                let ret_start = caller_info.base_reg + caller_info.ret_reg.index();
                self.regs.copy_within(range, ret_start);

                // Clear register window
                self.regs.truncate(self.base_reg);
                self.base_reg = caller_info.base_reg;
                self.pc = caller_info.ret_pc;
                Ok(None)
            }
        }
    }

    #[inline(always)]
    fn exec_alloc_buf(&mut self, i: Instruction) -> VMResult<()> {
        let len = self.reg(i.b()).get::<u64>() as usize;
        let ptr = self.heap.alloc_buff(len, 0).ok_or(VMError::AllocFailed)?;
        self.set_reg(i.a(), ptr);
        Ok(())
    }

    fn exec_alloc_dyn(&mut self, i: Instruction) -> VMResult<()> {
        let ptr = self.heap.alloc_dyn_buff(0).ok_or(VMError::AllocFailed)?;
        self.set_reg(i.a(), ptr);
        Ok(())
    }

    fn exec_alloc_str(&mut self, i: Instruction) -> VMResult<()> {
        let ptr = self.heap.alloc_str(0).ok_or(VMError::AllocFailed)?;
        self.set_reg(i.a(), ptr);
        Ok(())
    }

    fn exec_load(&mut self, i: Instruction) {
        let ptr = self.reg(i.b()).get::<GCPtr>();
        let offset = self.reg(i.c()).get::<u64>() as usize;
        self.set_reg_raw(i.a(), ptr.as_ref::<GCBuffer>().get(offset));
    }

    fn exec_store(&mut self, i: Instruction) {
        let mut ptr = self.reg(i.a()).get::<GCPtr>();
        let offset = self.reg(i.b()).get::<u64>() as usize;
        ptr.as_mut::<GCBuffer>().set(offset, self.reg(i.c()));
    }

    fn exec_barrier_fwd(&mut self, i: Instruction) {
        let parent: GCPtr = self.reg(i.a()).get();
        let child: GCPtr = self.reg(i.b()).get();
        self.heap.barrier_forward(parent, child);
    }

    fn exec_barrier_back(&mut self, i: Instruction) {
        let obj: GCPtr = self.reg(i.a()).get();
        self.heap.barrier_back(obj);
    }

    fn exec_spawn_task(&mut self, i: Instruction) -> VMResult<()> {
        let dst = i.a();
        let cinfo = self.program.functions[i.bx() as usize].call_info;
        let start = self.base_reg + dst.index();
        let args = &self.regs[start..start + cinfo.narg as usize];
        let task = Task::new(cinfo, args);
        let ptr = self.heap.alloc_task(task, 0).ok_or(VMError::AllocFailed)?;

        self.set_reg(dst, ptr);
        Ok(())
    }

    pub fn exec_await(&mut self, i: Instruction) -> VMResult<bool> {
        let mut current = self.scheduler.current().ok_or(VMError::IllegalAwait)?;

        if current.as_ref::<GCTask>().get().is_cancelled() {
            return Err(VMError::TaskCancelled);
        }

        let task: GCPtr = self.reg(i.a()).get();

        if task.as_ref::<GCTask>().get().is_complete() {
            //TODO: copy results into caller registers
            return Ok(true);
        }

        self.scheduler.await_task(current, task);

        let waiter = current.as_mut::<GCTask>().get_mut();
        std::mem::swap(&mut self.regs, &mut waiter.registers);
        std::mem::swap(&mut self.call_stack, &mut waiter.call_stack);
        waiter.base_reg = self.base_reg;
        waiter.pc = self.pc - 1; // pc pushed back so await is retried on resume

        Ok(false)
    }

    fn exec_join(&mut self, tasks: &[GCPtr]) -> VMResult<()> {
        // Start all pending tasks
        for &task_ptr in tasks {
            let task = task_ptr.as_ref::<GCTask>().get();
            if task.state == TaskState::Pending {
                self.scheduler.run(task_ptr);
            }
        }

        // Run until all joined tasks complete
        while !tasks
            .iter()
            .all(|&t| t.as_ref::<GCTask>().get().state == TaskState::Completed)
        {
            self.run::<false>()?;
        }

        // Results are now in each task's registers
        Ok(())
    }

    fn run<const SYNC: bool>(&mut self) -> VMResult<Option<Frame>> {
        while self.pc < self.program.bytecode.len() {
            let i = self.program.bytecode[self.pc];
            self.pc += 1;

            match i.op() {
                // Move operations
                Opcode::MOV => self.exec_mov(i),
                Opcode::CONST => self.exec_const(i),

                // Unary operations
                Opcode::BNOT => self.exec_bnot(i),
                Opcode::INOT => self.exec_inot(i),
                Opcode::UNOT => self.exec_unot(i),
                Opcode::INEG => self.exec_ineg(i),
                Opcode::FNEG => self.exec_fneg(i),

                // Signed integer arithmetic
                Opcode::IADD => self.exec_iadd(i),
                Opcode::ISUB => self.exec_isub(i),
                Opcode::IMUL => self.exec_imul(i),
                Opcode::IDIV => self.exec_idiv(i),
                Opcode::IREM => self.exec_irem(i),

                // Unsigned integer arithmetic
                Opcode::UADD => self.exec_uadd(i),
                Opcode::USUB => self.exec_usub(i),
                Opcode::UMUL => self.exec_umul(i),
                Opcode::UDIV => self.exec_udiv(i),
                Opcode::UREM => self.exec_urem(i),

                // Floating point arithmetic
                Opcode::FADD => self.exec_fadd(i),
                Opcode::FSUB => self.exec_fsub(i),
                Opcode::FMUL => self.exec_fmul(i),
                Opcode::FDIV => self.exec_fdiv(i),

                // Signed integer comparisons
                Opcode::IEQ => self.exec_ieq(i),
                Opcode::INE => self.exec_ine(i),
                Opcode::ILT => self.exec_ilt(i),
                Opcode::IGT => self.exec_igt(i),
                Opcode::ILE => self.exec_ile(i),
                Opcode::IGE => self.exec_ige(i),

                // Unsigned integer comparisons
                Opcode::UEQ => self.exec_ueq(i),
                Opcode::UNE => self.exec_une(i),
                Opcode::ULT => self.exec_ult(i),
                Opcode::UGT => self.exec_ugt(i),
                Opcode::ULE => self.exec_ule(i),
                Opcode::UGE => self.exec_uge(i),

                // Jump operations
                Opcode::JMP => self.exec_jump(i),
                Opcode::JMP_T => self.exec_jump_true(i),
                Opcode::JMP_F => self.exec_jump_false(i),

                Opcode::CALL => self.exec_call(i),
                Opcode::CALLT => self.exec_callt(i)?,
                Opcode::CALLN => self.exec_calln(i)?,
                Opcode::CALLR => self.exec_callr(i),
                Opcode::CALLNR => self.exec_callnr(i)?,
                Opcode::RET => {
                    if let Some(frame) = self.exec_ret(i)? {
                        return Ok(Some(frame));
                    }
                }

                Opcode::ALLOC_BUF => self.exec_alloc_buf(i)?,
                Opcode::ALLOC_DYN => self.exec_alloc_dyn(i)?,
                Opcode::ALLOC_STR => self.exec_alloc_str(i)?,

                Opcode::LOAD => self.exec_load(i),
                Opcode::STORE => self.exec_store(i),

                Opcode::SPAWN => self.exec_spawn_task(i)?,
                Opcode::AWAIT => {
                    if SYNC {
                        return Err(VMError::IllegalAwait);
                    }

                    if !self.exec_await(i)? {
                        return Ok(None);
                    }
                }

                Opcode::HALT => return Ok(None),
                op => return Err(VMError::IllegalOp(op)),
            }
        }
        Err(VMError::PCOutOfBounds)
    }

    pub fn execute(&mut self, func_id: FunctionPtr, args: &[Value]) -> VMResult<&[Value]> {
        // Reset VM state before running top level function.
        self.reset();

        let call_info = self.program.functions[func_id as usize].call_info;
        let argc = args.len() as u8;

        if call_info.narg != argc {
            return Err(VMError::InvalidArgCount {
                exp: call_info.narg,
                got: argc,
            });
        }

        self.regs.resize(call_info.nreg as usize, Value::zero());
        self.regs[..args.len()].copy_from_slice(args);
        self.base_reg = 0;
        self.pc = call_info.entry_pc;

        // Push synthetic frame
        self.call_stack.push(Frame {
            caller_info: None,
            callee_info: call_info,
        });

        match self.run::<true>()? {
            None => Err(VMError::EmptyCallStack),
            Some(frame) => {
                let start = self.base_reg;
                let range = start..(start + frame.callee_info.nret as usize);
                Ok(&self.regs[range])
            }
        }
    }

    fn collect_roots(&self) -> Vec<GCPtr> {
        self.regs.iter().filter_map(try_get_ptr).collect()
    }
}
