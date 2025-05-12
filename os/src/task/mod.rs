//! Task management implementation
//!
//! Everything about task management, like starting and switching tasks is
//! implemented here.
//!
//! A single global instance of [`TaskManager`] called `TASK_MANAGER` controls
//! all the tasks in the whole operating system.
//!
//! A single global instance of [`Processor`] called `PROCESSOR` monitors running
//! task(s) for each core.
//!
//! A single global instance of `PID_ALLOCATOR` allocates pid for user apps.
//!
//! Be careful when you see `__switch` ASM function in `switch.S`. Control flow around this function
//! might not be what you expect.
mod context;
mod id;
mod manager;
mod processor;
mod switch;
#[allow(clippy::module_inception)]
mod task;

use crate::mm::{VirtPageNum, MapPermission, VirtAddr, StepByOne};
use crate::loader::get_app_data_by_name;
use alloc::sync::Arc;
use lazy_static::*;
pub use manager::{fetch_task, TaskManager};
use switch::__switch;
pub use task::{TaskControlBlock, TaskStatus};

pub use context::TaskContext;
pub use id::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
pub use manager::add_task;
pub use processor::{
    current_task, current_trap_cx, current_user_token, run_tasks, schedule, take_current_task,
    Processor,
};

const BIG_STRIDE: usize = 1000000;

/// Suspend the current 'Running' task and run the next task in task list.
pub fn suspend_current_and_run_next() {
    // There must be an application running.
    let task = take_current_task().unwrap();

    // ---- access current TCB exclusively
    let mut task_inner = task.inner_exclusive_access();
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    // Change status to Ready
    task_inner.task_status = TaskStatus::Ready;
    // increase stride
    let pass = BIG_STRIDE / (task_inner.priority as usize);
    task_inner.stride += pass;
    drop(task_inner);
    // ---- release current PCB

    // push back to ready queue.
    add_task(task);
    // jump to scheduling cycle
    schedule(task_cx_ptr);
}

/// pid of usertests app in make run TEST=1
pub const IDLE_PID: usize = 0;

/// Exit the current 'Running' task and run the next task in task list.
pub fn exit_current_and_run_next(exit_code: i32) {
    // take from Processor
    let task = take_current_task().unwrap();

    let pid = task.getpid();
    if pid == IDLE_PID {
        println!(
            "[kernel] Idle process exit with exit_code {} ...",
            exit_code
        );
        panic!("All applications completed!");
    }

    // **** access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    // Change status to Zombie
    inner.task_status = TaskStatus::Zombie;
    // Record exit code
    inner.exit_code = exit_code;
    // do not move to its parent but under initproc

    // ++++++ access initproc TCB exclusively
    {
        let mut initproc_inner = INITPROC.inner_exclusive_access();
        for child in inner.children.iter() {
            child.inner_exclusive_access().parent = Some(Arc::downgrade(&INITPROC));
            initproc_inner.children.push(child.clone());
        }
    }
    // ++++++ release parent PCB

    inner.children.clear();
    // deallocate user space
    inner.memory_set.recycle_data_pages();
    drop(inner);
    // **** release current PCB
    // drop task manually to maintain rc correctly
    drop(task);
    // we do not have to save task context
    let mut _unused = TaskContext::zero_init();
    schedule(&mut _unused as *mut _);
}

lazy_static! {
    /// Creation of initial process
    ///
    /// the name "initproc" may be changed to any other app name like "usertests",
    /// but we have user_shell, so we don't need to change it.
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new(TaskControlBlock::new(
        get_app_data_by_name("ch5b_initproc").unwrap()
    ));
}

///Add init process to the manager
pub fn add_initproc() {
    add_task(INITPROC.clone());
}

///
pub fn map_for_current_task(start_vpn: VirtPageNum, num_pages: usize, map_perm: MapPermission) -> isize {
    let cur_task = current_task().unwrap();
    let memory_set = &mut cur_task.inner_exclusive_access().memory_set;
    let mut end_vpn = start_vpn;
    for _ in 0..num_pages {
        if let Some(pte) = memory_set.translate(end_vpn) {
            if pte.is_valid() { // vpn 已经被映射到了已经存在的物理页
                return -1;
            }
        }
        end_vpn.step();
    }
    let start_va = VirtAddr::from(start_vpn);
    let end_va = VirtAddr::from(end_vpn);
    // 一块新的 MapArea，会自动 map PageTable
    memory_set.insert_framed_area(start_va, end_va, map_perm);
    return 0;
}

///
pub fn unmap_for_current_task(start_vpn: VirtPageNum, num_pages: usize) -> isize {
    let cur_task = current_task().unwrap();
    let memory_set = &mut cur_task.inner_exclusive_access().memory_set;
    let mut end_vpn = start_vpn;
    for _ in 0..num_pages {
        if let Some(pte) = memory_set.translate(end_vpn) {
            if !pte.is_valid() { // 不能解除不存在的映射
                return -1;
            }
            // PageTable 里面的 frames 存储了页表所有节点对应的物理帧，这些帧用来存 pte
            // 而 MapArea 里面的 data_frames 存储了页表三级节点中的最后一级（叶节点）指向的物理帧，这些帧用来存数据
            // 这里并没有释放掉 MapArea，但从 PageTable 中去掉了对 MapArea 中相应 data_frames 的映射
            //（当然，可能并非该 MapArea 全部 data_frames），因此该 MapArea 中再无法通过 PageTable 映射到的 data_frames
            // 对后续也没有影响了，没有被取消映射因此仍可以通过 PageTable 映射到的哪些 data_frames 没有受到影响  
            memory_set.unmap_from_page_table(end_vpn);
            end_vpn.step();
        } else { // 不能解除不存在的映射
            return -1;
        }
    }
    return 0;
}