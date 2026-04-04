use std::collections::VecDeque;

use ahash::AHashMap;

use crate::{
    heap::GCPtr,
    instruction::Reg,
    object::{GCTask, Value},
    program::CallInfo,
    vm::Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    Task(GCPtr),
    Timer(u64),
    Io(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// Not yet started, waiting to be scheduled
    Pending,
    /// In ready queue, will run when scheduled
    Ready,
    /// Currently executing (is current_task)
    Running,
    /// Waiting on another task, timer, or IO
    Suspended(WaitReason),
    /// Finished execution, results available
    Completed(Reg),
    /// Was cancelled before completion
    Cancelled,
}

pub struct Task {
    pub registers: Vec<Value>,
    pub call_stack: Vec<Frame>,
    pub base_reg: usize,
    pub pc: usize,
    pub state: TaskState,
    pub call_info: CallInfo,
}

impl Task {
    pub(super) fn new(call_info: CallInfo, args: &[Value]) -> Self {
        let mut registers = vec![Value::zero(); call_info.nreg as usize];
        registers[..args.len()].copy_from_slice(args);

        Self {
            registers,
            pc: call_info.entry_pc,
            call_info,
            call_stack: vec![],
            base_reg: 0,
            state: TaskState::Ready,
        }
    }

    #[inline(always)]
    pub(super) fn is_complete(&self) -> bool {
        matches!(self.state, TaskState::Completed(_))
    }

    #[inline(always)]
    pub(super) fn is_cancelled(&self) -> bool {
        self.state == TaskState::Cancelled
    }
}

pub struct Scheduler {
    /// Currently executing task (if in async context)
    current: Option<GCPtr>,

    /// Tasks ready to run
    ready: VecDeque<GCPtr>,

    /// Task -> what it's waiting for
    suspended: AHashMap<GCPtr, WaitReason>,

    /// Reverse index: completed task -> tasks waiting on it
    /// Allows O(1) wake on completion
    task_waiters: AHashMap<GCPtr, Vec<GCPtr>>,

    /// Timer ID -> tasks waiting on it
    timer_waiters: AHashMap<u64, Vec<GCPtr>>,

    /// IO ID -> tasks waiting on it
    io_waiters: AHashMap<u64, Vec<GCPtr>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            current: None,
            ready: VecDeque::new(),
            suspended: AHashMap::new(),
            task_waiters: AHashMap::new(),
            timer_waiters: AHashMap::new(),
            io_waiters: AHashMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.current = None;
        self.ready.clear();
        self.suspended.clear();
        self.task_waiters.clear();
        self.timer_waiters.clear();
        self.io_waiters.clear();
    }

    #[inline]
    pub fn current(&self) -> Option<GCPtr> {
        self.current
    }

    pub fn run(&mut self, mut task_ptr: GCPtr) {
        let task = task_ptr.as_mut::<GCTask>().get_mut();
        if task.state == TaskState::Pending {
            task.state = TaskState::Ready;
            self.ready.push_back(task_ptr);
        }
    }

    /// Suspend current task waiting on another task
    pub fn await_task(&mut self, mut waiter: GCPtr, target: GCPtr) {
        let task = waiter.as_mut::<GCTask>().get_mut();
        task.state = TaskState::Suspended(WaitReason::Task(target));

        self.suspended.insert(waiter, WaitReason::Task(target));
        self.task_waiters.entry(target).or_default().push(waiter);
    }

    /// Mark task as completed, wake waiters
    pub fn complete(&mut self, mut task_ptr: GCPtr, ret_reg: Reg) {
        let task = task_ptr.as_mut::<GCTask>().get_mut();
        task.state = TaskState::Completed(ret_reg);

        // Wake all tasks waiting on this one
        if let Some(waiters) = self.task_waiters.remove(&task_ptr) {
            for mut waiter in waiters {
                self.suspended.remove(&waiter);
                waiter.as_mut::<GCTask>().get_mut().state = TaskState::Ready;
                self.ready.push_back(waiter);
            }
        }
    }

    /// Get all task pointers (for GC roots)
    pub fn all_tasks(&self) -> impl Iterator<Item = GCPtr> + '_ {
        let current = self.current.iter().copied();
        let ready = self.ready.iter().copied();
        let suspended = self.suspended.keys().copied();
        current.chain(ready).chain(suspended)
    }
}
