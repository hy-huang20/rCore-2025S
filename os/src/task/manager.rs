//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
//use alloc::collections::VecDeque;
use alloc::vec::Vec;
use alloc::sync::Arc;
use lazy_static::*;
/*
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }
}
*/

///
pub struct TaskManager {
    ready_vec: Vec<Arc<TaskControlBlock>>,
}

/// 暴力搜索 stride 最小的进程的 scheduler
impl TaskManager {
    ///
    pub fn new() -> Self {
        Self {
            ready_vec: Vec::new(),
        }
    }
    ///
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_vec.push(task);
    }
    ///
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let mut min_stride: usize = 0xffffffff;
        let mut min_index: usize = self.ready_vec.len();
        for i in 0..self.ready_vec.len() {
            if let Some(task) = self.ready_vec.get(i) {
                let task_inner = task.inner_exclusive_access();
                if min_stride > task_inner.stride {
                    min_stride = task_inner.stride;
                    min_index = i;
                }
            }
        }
        if min_index >= self.ready_vec.len() {
            None
        } else {
            Some(self.ready_vec.swap_remove(min_index))
        }
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
