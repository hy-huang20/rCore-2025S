use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

/// 返回 true 代表不安全，false 代表安全
fn banker_algorithm(
    num_resources: usize, num_threads: usize, 
    worker: &mut Vec<u32>, allocation: &Vec<Vec<u32>>, need: &Vec<Vec<u32>>) -> bool 
{
    let mut finish = vec![false; num_threads];
    loop {
        let mut all_finished = true;
        let mut matching_thread_exist = false;
        for i in 0..num_threads {
            if finish[i] {
                continue;
            }
            all_finished = false;
            let mut is_matching = true;
            for j in 0..num_resources {
                if need[i][j] > worker[j] {
                    is_matching = false;
                    break;
                }
            }
            if is_matching {
                finish[i] = true;
                matching_thread_exist = true;
                for j in 0..num_resources {
                    worker[j] += allocation[i][j];
                }
                break;
            }
        }
        if !matching_thread_exist {
            return !all_finished;
        }
    }
}

fn mutex_deadlock_exist(tid: usize, mid: usize) -> bool {
    let process = current_process();
    let num_threads = process.inner_exclusive_access().tasks.len();
    let num_resources = process.inner_exclusive_access().mutex_list.len();
    let mut worker = vec![0u32; num_resources]; // worker = available
    let allocation = vec![vec![0u32; num_resources]; num_threads];
    let mut need = vec![vec![0u32; num_resources]; num_threads]; // need = max - allocation
    for i in 0..num_resources { 
        if let Some(mutex_any) = &process.inner_exclusive_access().mutex_list[i] {
            let mutex_any = mutex_any.clone();
            if let Some(mutex_blocking) = mutex_any.as_any().downcast_ref::<MutexBlocking>() {
                let inner = mutex_blocking.inner.exclusive_access();
                if !inner.locked {
                    worker[i] = 1;
                }
            }
        }
    }
    need[tid][mid] = 1;

    return banker_algorithm(num_resources, num_threads, &mut worker, &allocation, &need);
}

fn semaphore_deadlock_exist(tid: usize, sid: usize) -> bool {
    return tid + sid != 0;
}

/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        process_inner.mutex_list.len() as isize - 1
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let pid = current_task().unwrap().process.upgrade().unwrap().getpid();
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    trace!("kernel:pid[{}] tid[{}] sys_mutex_lock", pid, tid);
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    // TODO
    if process_inner.deadlock_detect_enabled && mutex_deadlock_exist(tid, mutex_id) {
        return -0xdead;
    }
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.lock();
    0
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .semaphore_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
        id
    } else {
        process_inner
            .semaphore_list
            .push(Some(Arc::new(Semaphore::new(res_count))));
        process_inner.semaphore_list.len() - 1
    };
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.up();
    0
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    let pid = current_task().unwrap().process.upgrade().unwrap().getpid();
    let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
    trace!("kernel:pid[{}] tid[{}] sys_semaphore_down", pid, tid);
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    // TODO
    if process_inner.deadlock_detect_enabled && semaphore_deadlock_exist(tid, sem_id) {
        return -0xdead;
    }
    let sem = Arc::clone(process_inner.semaphore_list[sem_id].as_ref().unwrap());
    drop(process_inner);
    sem.down();
    0
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let condvar = Arc::clone(process_inner.condvar_list[condvar_id].as_ref().unwrap());
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    condvar.wait(mutex);
    0
}
/// enable deadlock detection syscall
///
/// YOUR JOB: Implement deadlock detection, but might not all in this syscall
pub fn sys_enable_deadlock_detect(_enabled: usize) -> isize {
    trace!("kernel: sys_enable_deadlock_detect");
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    match _enabled {
        1 => process_inner.deadlock_detect_enabled = true,
        0 => process_inner.deadlock_detect_enabled = false,
        _ => return -1, // 参数不合法
    };
    return 0;
}
