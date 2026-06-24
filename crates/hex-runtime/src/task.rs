use std::collections::VecDeque;

use hex_vm::{Args, FunctionId, IsValue, Program, Reg, VM, Value};
use slotmap::{Key, KeyData, SlotMap};
use smallvec::{SmallVec, smallvec};

slotmap::new_key_type! {
    pub struct TaskId;
}

impl IsValue for TaskId {
    fn from_value(v: Value) -> Self {
        TaskId::from(KeyData::from_ffi(v.to_bits()))
    }

    fn into_value(self) -> Value {
        Value::from_bits(self.data().as_ffi())
    }
}

pub enum TaskState {
    Pending,
    Ready,
    Running,
    Waiting,
    Done,
}

pub struct Task {
    pub(crate) func: FunctionId,
    pub(crate) vm: VM,
    pub(crate) state: TaskState,
    pub(crate) joiners: SmallVec<[TaskId; 4]>,
    pub(crate) resume_into: Option<(usize, Reg)>,
}

pub struct Scheduler<'p> {
    pub(crate) program: &'p Program,
    pub(crate) tasks: SlotMap<TaskId, Task>,
    pub(crate) ready: VecDeque<TaskId>,
    pub(crate) current: Option<TaskId>,
}

impl<'p> Scheduler<'p> {
    pub fn new(program: &'p Program) -> Self {
        Self {
            program,
            tasks: SlotMap::with_key(),
            ready: VecDeque::new(),
            current: None,
        }
    }

    pub fn reset(&mut self) {
        self.tasks.clear();
        self.ready.clear();
        self.current = None;
    }

    pub fn new_task(&mut self, func: FunctionId, args: Args<'_>) -> Result<TaskId, hex_vm::Error> {
        Ok(self.tasks.insert(Task {
            func,
            vm: VM::from_entry(self.program, func, args)?,
            state: TaskState::Pending,
            joiners: smallvec![],
            resume_into: None,
        }))
    }

    /// A task finished. Capture its returns, mark Done, wake all joiners.
    pub fn complete(&mut self, tid: TaskId) {
        let task = &mut self.tasks[tid];
        let nret = self.program.function(task.func).nret as usize;
        let result = SmallVec::<[Value; 4]>::from(&task.vm.registers[..nret]);
        let joiners = std::mem::take(&mut task.joiners);

        task.state = TaskState::Done;

        // Wake joiners
        for j in joiners {
            self.wake(j, &result);
        }
    }

    /// Deliver a result into a blocked task's saved return window and ready it.
    fn wake(&mut self, tid: TaskId, result: &[Value]) {
        let task = &mut self.tasks[tid];
        if let Some((base, nret)) = task.resume_into.take() {
            let n = nret as usize;
            task.vm.registers[base..base + n].copy_from_slice(&result[..n]);
        }
        task.state = TaskState::Ready;
        self.ready.push_back(tid);
    }
}
