use std::usize;

use thiserror::Error;

use crate::{
    async_runtime::{Scheduler, Task, TaskState},
    heap::{GCPtr, Heap},
    instruction::*,
    memory::PageAllocator,
    object::{AsValue, GCBuffer, GCTask, Value, try_get_ptr},
    program::{CallInfo, FunctionPtr, NativeFunc, Program},
};

#[derive(Debug, Copy, Clone, Error)]
pub enum VMError {
    #[error("Illegal opcode received: {0:?}")]
    UnknownOp(Opcode),
    #[error("Invalid argument count: expected {exp}, got {got}")]
    InvalidArgCount { exp: u8, got: u8 },
    #[error("Attempted to await in a cancelled async task")]
    TaskCancelled,
    #[error("Await instruction received outside of an async task")]
    AwaitOutsideAsync,
    #[error("Program counter out of bounds")]
    PCOutOfBounds,
    #[error("Heap allocation failed")]
    AllocFailed,
}

type RunMode = u8;

const SYNC: RunMode = 0;
const ASYNC: RunMode = 1;

enum ReturnType {
    Internal,
    TopLevel(Reg),
}

use VMError::*;

pub type VMResult<T> = Result<T, VMError>;

struct CallerInfo {
    ret_pc: usize,
    base_reg: usize,
}

pub(crate) struct Frame {
    callee_info: CallInfo,
    caller_info: CallerInfo,
}

pub struct VM<'a, A: PageAllocator> {
    regs: Vec<Value>,
    call_stack: Vec<Frame>,
    native_call_ret: Box<[Value; u8::MAX as usize]>,
    heap: Heap<A>,
    scheduler: Scheduler,
    pc: usize,
    base: usize,
    program: &'a Program,
}

impl<'a, A: PageAllocator> VM<'a, A> {
    pub fn reset(&mut self) {
        self.base = 0;
        self.pc = 0;

        self.regs.clear();
        self.call_stack.clear();
        self.heap.reset();
        self.scheduler.reset();
    }

    #[inline(always)]
    fn reg(&self, reg: Reg) -> Value {
        self.regs[self.base + reg as usize]
    }

    #[inline(always)]
    fn reg_mut(&mut self, reg: Reg) -> &mut Value {
        &mut self.regs[self.base + reg as usize]
    }

    #[inline(always)]
    fn set_reg(&mut self, reg: Reg, value: impl AsValue) {
        self.regs[self.base + reg as usize] = value.into_value();
    }

    #[inline(always)]
    fn set_reg_raw(&mut self, reg: Reg, value: Value) {
        self.regs[self.base + reg as usize] = value;
    }

    #[inline(always)]
    fn two_reg<T: AsValue>(&self, reg_a: Reg, reg_b: Reg) -> (T, T) {
        (self.reg(reg_a).get(), self.reg(reg_b).get())
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
        // Call convention: args are in caller registers[RRet..RN]
        //
        // We use overlapping register windows for caller and callee
        // to avoid copying arguments. This is safe because argument registers
        // are not clobbered by the caller until the callee returns.

        // Callee window starts at caller's return register.
        let base = self.base + ret_reg as usize;
        let last = base + cinfo.nreg as usize;

        // Grow regs to fit callee's full register count beyond the arg base.
        if last > self.regs.len() {
            self.regs.resize(last, Value::zero());
        }

        // Create new call frame and save caller info
        self.call_stack.push(Frame {
            callee_info: cinfo,
            caller_info: CallerInfo {
                ret_pc: self.pc,
                base_reg: self.base,
            },
        });

        // Jump to callee code
        self.base = base;
        self.pc = cinfo.entry_pc;
    }

    #[inline(always)]
    fn exec_call(&mut self, i: Instruction) {
        let cinfo = self.program.funcs[i.bx() as usize].call_info;
        self.call(i.a(), cinfo);
    }

    #[inline(always)]
    fn exec_callr(&mut self, i: Instruction) {
        let func = self.reg(i.b()).get::<FunctionPtr>();
        let cinfo = self.program.funcs[func as usize].call_info;
        self.call(i.a(), cinfo);
    }

    fn exec_callt(&mut self, i: Instruction) {
        let cinfo = self.program.funcs[i.bx() as usize].call_info;
        let last = self.base + cinfo.nreg as usize;

        // Grow regs to fit callee's full register count beyond the arg base.
        if last > self.regs.len() {
            self.regs.resize(last, Value::zero());
        }

        // copy args into the same window
        let start = self.base + i.a() as usize;
        let end = start + cinfo.narg as usize;
        self.regs.copy_within(start..end, self.base);

        // Reuse current frame and update the callee info.
        // If we are top level function, nret must be the same
        // for caller and tail callee, so we don't need to update
        // callee info.
        if let Some(frame) = self.call_stack.last_mut() {
            frame.callee_info = cinfo
        }

        self.pc = cinfo.entry_pc;
    }

    #[inline(always)]
    fn calln(&mut self, ret_reg: Reg, func: &NativeFunc) -> VMResult<()> {
        let narg = func.narg as usize;
        let nret = func.nret as usize;

        // Immutably borrow args from caller registers
        // Call convention: args are in caller registers[RRet..RN]
        let ret = self.base + ret_reg as usize;
        let arg = &self.regs[ret..ret + narg];
        let res = &mut self.native_call_ret[..nret];

        // Call native function
        (func.func)(arg, res)?;

        // Copy results back into caller registers
        self.regs[ret..ret + nret].copy_from_slice(res);
        Ok(())
    }

    #[inline(always)]
    fn exec_calln(&mut self, i: Instruction) -> VMResult<()> {
        let func = &self.program.native_funcs[i.bx() as usize];
        self.calln(i.a(), func)
    }

    #[inline(always)]
    fn exec_callnr(&mut self, i: Instruction) -> VMResult<()> {
        let func_id = self.reg(i.b()).get::<FunctionPtr>() as usize;
        let func = &self.program.native_funcs[func_id];
        self.calln(i.a(), func)
    }

    fn exec_ret(&mut self, i: Instruction) -> ReturnType {
        let Some(frame) = self.call_stack.pop() else {
            // No frame, return from top level function
            return ReturnType::TopLevel(i.a());
        };

        // Copy return values to caller's registers
        let start = self.base + i.a() as usize;
        let end = start + frame.callee_info.nret as usize;
        self.regs.copy_within(start..end, self.base);

        // Return back to caller
        self.base = frame.caller_info.base_reg;
        self.pc = frame.caller_info.ret_pc;
        ReturnType::Internal
    }

    #[inline(always)]
    fn exec_alloc_buf(&mut self, i: Instruction) -> VMResult<()> {
        let len = self.reg(i.b()).get::<u64>() as usize;
        let ptr = self.heap.alloc_buff(len, 0).ok_or(AllocFailed)?;
        self.set_reg(i.a(), ptr);
        Ok(())
    }

    fn exec_alloc_dyn(&mut self, i: Instruction) -> VMResult<()> {
        let ptr = self.heap.alloc_dyn_buff(0).ok_or(AllocFailed)?;
        self.set_reg(i.a(), ptr);
        Ok(())
    }

    fn exec_alloc_str(&mut self, i: Instruction) -> VMResult<()> {
        let ptr = self.heap.alloc_str(0).ok_or(AllocFailed)?;
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
        let cinfo = self.program.funcs[i.bx() as usize].call_info;
        let start = self.base + dst as usize;
        let args = &self.regs[start..start + cinfo.narg as usize];
        let task = Task::new(cinfo, args);
        let ptr = self.heap.alloc_task(task, 0).ok_or(AllocFailed)?;

        self.set_reg(dst, ptr);
        Ok(())
    }

    pub fn exec_await(&mut self, i: Instruction) -> VMResult<bool> {
        let mut current = self.scheduler.current().ok_or(AwaitOutsideAsync)?;

        if current.as_ref::<GCTask>().get().is_cancelled() {
            return Err(TaskCancelled);
        }

        let task_ptr: GCPtr = self.reg(i.a()).get();
        let task = task_ptr.as_ref::<GCTask>().get();

        if let TaskState::Completed(task_ret) = task.state {
            todo!("move results into caller's registers")
            //return Ok(true);
        }

        self.scheduler.await_task(current, task_ptr);

        let waiter = current.as_mut::<GCTask>().get_mut();
        std::mem::swap(&mut self.regs, &mut waiter.registers);
        std::mem::swap(&mut self.call_stack, &mut waiter.call_stack);
        waiter.base_reg = self.base;
        waiter.pc = self.pc - 1; // pc pushed back so await is retried on resume

        Ok(false)
    }

    fn exec_join(&mut self, tasks: &[GCPtr]) -> VMResult<()> {
        // Start all pending tasks
        tasks.iter().for_each(|&t| self.scheduler.run(t));

        // Run until all joined tasks complete
        while !tasks
            .iter()
            .all(|t| t.as_ref::<GCTask>().get().is_complete())
        {
            // Run in async mode
            self.run::<ASYNC>()?;
        }

        // Results are now in each task's registers
        Ok(())
    }

    fn run<const MODE: RunMode>(&mut self) -> VMResult<Reg> {
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

                // Call operations
                Opcode::CALL => self.exec_call(i),
                Opcode::CALLR => self.exec_callr(i),
                Opcode::CALLT => self.exec_callt(i),
                Opcode::CALLN => self.exec_calln(i)?,
                Opcode::CALLNR => self.exec_callnr(i)?,

                Opcode::RET => match self.exec_ret(i) {
                    ReturnType::Internal => {}
                    ReturnType::TopLevel(ret_reg) => return Ok(ret_reg),
                },

                Opcode::ALLOC_BUF => self.exec_alloc_buf(i)?,
                Opcode::ALLOC_DYN => self.exec_alloc_dyn(i)?,
                Opcode::ALLOC_STR => self.exec_alloc_str(i)?,

                Opcode::LOAD => self.exec_load(i),
                Opcode::STORE => self.exec_store(i),

                Opcode::SPAWN => self.exec_spawn_task(i)?,
                Opcode::AWAIT => {
                    if const { MODE == SYNC } {
                        return Err(VMError::AwaitOutsideAsync);
                    }

                    if !self.exec_await(i)? {
                        return Ok(0);
                    }
                }

                Opcode::HALT => return Ok(0),

                op => return Err(UnknownOp(op)),
            }
        }
        Err(PCOutOfBounds)
    }

    pub fn execute(&mut self, entry: FunctionPtr, args: &[Value]) -> VMResult<&[Value]> {
        // Reset VM state before running top level function.
        self.reset();

        let call_info = self.program.funcs[entry as usize].call_info;
        let argc = args.len() as u8;

        if call_info.narg != argc {
            return Err(InvalidArgCount {
                exp: call_info.narg,
                got: argc,
            });
        }

        self.regs.resize(call_info.nreg as usize, Value::zero());
        self.regs[..args.len()].copy_from_slice(args);
        self.base = 0;
        self.pc = call_info.entry_pc;

        let ret_reg = self.run::<SYNC>()?;
        let start = self.base + ret_reg as usize;
        let end = start + call_info.nret as usize;

        Ok(&self.regs[start..end])
    }

    fn collect_roots(&self) -> Vec<GCPtr> {
        self.regs.iter().filter_map(try_get_ptr).collect()
    }
}
