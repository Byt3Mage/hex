use std::collections::VecDeque;

use hex_vm::{Args, AsWord, FunctionId, Program, Reg, VM, word};
use slotmap::{Key, KeyData, SlotMap};
use smallvec::{SmallVec, smallvec};

slotmap::new_key_type! {
    pub struct TaskId;
}

impl AsWord for TaskId {
    fn from_word(w: word) -> Self {
        TaskId::from(KeyData::from_ffi(w as u64))
    }

    fn into_word(self) -> word {
        self.data().as_ffi() as word
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
        let result = SmallVec::<[word; 4]>::from(&task.vm.registers[..nret]);
        let joiners = std::mem::take(&mut task.joiners);

        task.state = TaskState::Done;

        for j in joiners {
            self.wake(j, &result);
        }
    }

    /// Deliver a result into a blocked task's saved return window and ready it.
    fn wake(&mut self, tid: TaskId, result: &[word]) {
        let task = &mut self.tasks[tid];
        if let Some((base, nret)) = task.resume_into.take() {
            let n = nret as usize;
            task.vm.registers[base..base + n].copy_from_slice(&result[..n]);
        }
        task.state = TaskState::Ready;
        self.ready.push_back(tid);
    }
}
