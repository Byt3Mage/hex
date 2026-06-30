use hex_vm::{Args, AsWord, Flow, FunctionId, HeapVM, HostCtx, Program, RunOutcome, Syscode};

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
                let func = FunctionId::from_word(*f);
                let args = Args::new(args).unwrap();
                let task_id = self.scheduler.new_task(func, args)?;
                ctx.ret(0, task_id)?;
                Ok(Flow::Continue)
            }
            syscode::AWAIT_TASK => {
                let task_id: TaskId = ctx.arg(0)?;
                let task = &mut self.scheduler.tasks[task_id];

                match task.state {
                    TaskState::Ready | TaskState::Running | TaskState::Waiting => {
                        // Do nothing, wait for task to complete
                    }
                    TaskState::Done => {
                        // Return and resume immediately if done
                        let nret = ctx.nrets() as usize;
                        let rets = ctx.rets();
                        rets.copy_from_slice(&task.vm.registers[..nret]);
                        return Ok(Flow::Continue);
                    }
                    TaskState::Pending => {
                        // Mark as ready and add to ready queue
                        task.state = TaskState::Ready;
                        self.scheduler.ready.push_back(task_id);
                    }
                }

                // Add current task as a waiter
                if let Some(curr_id) = self.scheduler.current {
                    task.joiners.push(curr_id);
                    let curr_task = &mut self.scheduler.tasks[curr_id];
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
        let mut runner = HeapVM::new(0);
        let program = self.scheduler.program;

        while let Some(task_id) = self.scheduler.ready.pop_front() {
            self.scheduler.current = Some(task_id);

            let task = &mut self.scheduler.tasks[task_id];
            task.state = TaskState::Running;
            std::mem::swap(&mut task.vm, &mut runner);

            let outcome = hex_vm::run(&mut runner, &program, self, mem)?;

            let task = &mut self.scheduler.tasks[task_id];
            std::mem::swap(&mut task.vm, &mut runner);

            match outcome {
                RunOutcome::Completed => self.scheduler.complete(task_id),
                RunOutcome::Suspended => {}
                RunOutcome::Trapped(f) => {
                    eprintln!("task {task_id:#?} trapped: {f}");
                    self.scheduler.complete(task_id);
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
    pub fn new(program: Program<'p>, memory_size: usize) -> Self {
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
