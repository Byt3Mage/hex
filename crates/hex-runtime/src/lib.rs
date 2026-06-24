use hex_vm::{Args, Flow, FunctionId, HostCtx, Program, RunOutcome, Syscode};

use crate::task::{Scheduler, TaskId, TaskState};

pub mod allocator;
pub mod task;
//pub mod gc;
//pub mod metadata;

pub const ABORT_UNKNOWN: u8 = 0;
pub const ABORT_OOM: u8 = 1;
pub const ABORT_TIMEOUT: u8 = 2;
pub const ABORT_DEADLOCK: u8 = 3;

// Syscodes this runtime's ABI defines.
pub mod syscode {
    use hex_vm::Syscode;

    pub const SPAWN_TASK: Syscode = 0; // (entry_fn, ...args) -> TaskId
    pub const AWAIT_TASK: Syscode = 1; // (task_id) -> the awaited task's return values
    pub const YIELD_TASK: Syscode = 2; // () -> () ; cooperative reschedule
}

pub struct Host<'p> {
    scheduler: Scheduler<'p>,
}

impl<'p> hex_vm::Host for Host<'p> {
    fn syscall(&mut self, code: Syscode, mut ctx: HostCtx) -> Result<Flow, hex_vm::Error> {
        match code {
            syscode::SPAWN_TASK => {
                let [f, args @ ..] = ctx.args() else { panic!("invalid spawn args") };
                let args = Args::new(args).unwrap();
                let tid = self.scheduler.new_task(f.get(), args)?;
                ctx.ret(0, tid)?;
                Ok(Flow::Continue)
            }
            syscode::AWAIT_TASK => {
                let tid: TaskId = ctx.arg(0)?;
                let task = &mut self.scheduler.tasks[tid];

                // Return value and resume if already done
                if matches!(task.state, TaskState::Done) {}

                match task.state {
                    TaskState::Ready | TaskState::Running | TaskState::Waiting => {}
                    TaskState::Done => {
                        // Return and resume immediately if done
                        ctx.ret_all(&task.vm.registers);
                        return Ok(Flow::Continue);
                    }
                    TaskState::Pending => {
                        // Mark as ready and add to ready queue
                        task.state = TaskState::Ready;
                        self.scheduler.ready.push_back(tid);
                    }
                }

                if let Some(curr) = self.scheduler.current {
                    // Add current task as a waiter
                    task.joiners.push(curr);
                    let curr_task = &mut self.scheduler.tasks[curr];
                    curr_task.state = TaskState::Waiting;
                    curr_task.resume_into = Some((ctx.arg_base(), ctx.nrets()));
                }

                Ok(Flow::Suspend)
            }
            _ => Err(hex_vm::Error::UnknownSys(code)),
        }
    }
}

impl<'p> Host<'p> {
    fn run_to_completion(&mut self, mem: &mut [u8]) -> Result<(), hex_vm::Error> {
        while let Some(tid) = self.scheduler.ready.pop_front() {
            self.scheduler.current = Some(tid);

            let task = &mut self.scheduler.tasks[tid];
            task.state = TaskState::Running;

            // Take VM state to avoid borrowing issues
            let mut vm = std::mem::take(&mut task.vm);

            let outcome = hex_vm::run(&mut vm, self.scheduler.program, self, mem)?;

            self.scheduler.tasks[tid].vm = vm;

            match outcome {
                RunOutcome::Completed => self.scheduler.complete(tid),
                RunOutcome::Suspended => {}
                RunOutcome::Trapped(f) => {
                    eprintln!("task {tid:#?} trapped: {f}");
                    self.scheduler.complete(tid);
                }
            }
        }
        Ok(())
    }
}

pub struct Runtime<'p> {
    host: Host<'p>,
    memory: Vec<u8>,
}

impl<'p> Runtime<'p> {
    pub fn new(program: &'p Program, memory_size: usize) -> Self {
        Self {
            memory: vec![0; memory_size],
            host: Host { scheduler: Scheduler::new(program) },
        }
    }

    pub fn execute(&mut self, func: FunctionId, args: Args<'_>) -> Result<(), hex_vm::Error> {
        // Clear previous state and reset
        self.host.scheduler.reset();

        // Spawn root task from entry point
        let root_id = self.host.scheduler.new_task(func, args)?;
        self.host.scheduler.tasks[root_id].state = TaskState::Ready;
        self.host.scheduler.ready.push_back(root_id);
        self.host.run_to_completion(&mut self.memory)
    }
}
