//! Types related to task management

use super::TaskContext;
use crate::syscall::SYSCALL_ID_UPPER_BOUND;

/// The task control block (TCB) of a task.
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// task 调用 id 对应的 syscall 的次数
    pub syscall_cnt: [usize; SYSCALL_ID_UPPER_BOUND],
}

/// The status of a task
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}
