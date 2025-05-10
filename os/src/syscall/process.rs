//! Process management syscalls
use crate::{
    task::{change_program_brk, exit_current_and_run_next, suspend_current_and_run_next, get_current_task_id, get_syscall_cnt, increase_syscall_cnt, current_user_token, MapStatus, map_page_for_current_task, unmap_page_for_current_task},
    timer::get_time_us,
    mm::{PageTable, VirtAddr, translated_byte_buffer, PTEFlags},
    config::PAGE_SIZE,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// 更新当前 task 相应 syscall 调用次数
pub fn update_syscall_cnt(_syscall_id: usize) {
    increase_syscall_cnt(get_current_task_id(), _syscall_id);
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let ts = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let size_of_timeval = core::mem::size_of::<TimeVal>();
    let buffers = translated_byte_buffer(current_user_token(), _ts as *const u8, size_of_timeval);
    let ts_byte_arr: &[u8] = unsafe {
        core::slice::from_raw_parts(
            &ts as *const TimeVal as *const u8,
            size_of_timeval
        )
    };
    let mut ts_idx: usize = 0;
    for buffer in buffers {
        buffer.copy_from_slice(&ts_byte_arr[ts_idx..ts_idx+buffer.len()]);
        ts_idx += buffer.len();
    }
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    let token = current_user_token();
    let page_table = PageTable::from_token(token);
    match _trace_request {
        0 => {
            let vaddr = VirtAddr::from(_id as *const u8 as usize);
            let vpn = vaddr.floor();
            match page_table.translate(vpn) {
                Some(pte) => {
                    if !pte.is_valid() || !pte.usermode() || !pte.readable() { // 不可读
                        -1
                    } else {
                        let ppn = pte.ppn();
                        ppn.get_bytes_array()[vaddr.page_offset()] as isize
                    }
                },
                None => -1, // 不可见
            }
        },
        1 => {
            let vaddr = VirtAddr::from(_id as *mut u8 as usize);
            let vpn = vaddr.floor();
            match page_table.translate(vpn) {
                Some(pte) => {
                    if !pte.is_valid() || !pte.usermode() || !pte.writable() { // 不可写
                        -1
                    } else {
                        let ppn = pte.ppn();
                        ppn.get_bytes_array()[vaddr.page_offset()] = _data as u8; // 只需要低位 1 个字节
                        0
                    }
                },
                None => -1, // 不可见
            }
        },
        2 => {
            let syscall_id = _id;
            let current_task_id = get_current_task_id();
            let ret = get_syscall_cnt(current_task_id, syscall_id) as isize;
            ret
        },
        _ => -1,
    }
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    if _start % PAGE_SIZE != 0 { // 如果虚拟地址没有按页对齐直接失败
        return -1;
    }
    if _prot & !0x7 != 0 { // _prot 其余位必须为 0
        return -1;
    }
    if _prot & 0x7 == 0 { // 这样的内存无意义
        return -1;
    }
    let num_pages = (_len + PAGE_SIZE - 1) / PAGE_SIZE; // page 数向上取整
    let flags = PTEFlags::from_bits(((_prot & 0x7) << 1) as u8).unwrap();
    for i in 0..num_pages {
        let vpn = VirtAddr::from(_start + i * PAGE_SIZE).floor();
        match map_page_for_current_task(vpn, flags) {
            MapStatus::Succ => {
                continue;
            },
            _ => {
                return -1;
            }
        };
    }
    return 0;
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    if _start % PAGE_SIZE != 0 { // 如果虚拟地址没有按页对齐直接失败
        return -1;
    }
    let num_pages = (_len + PAGE_SIZE - 1) / PAGE_SIZE; // page 数向上取整
    for i in 0..num_pages {
        let vpn = VirtAddr::from(_start + i * PAGE_SIZE).floor();
        match unmap_page_for_current_task(vpn) {
            MapStatus::Succ => {
                continue;
            },
            _ => {
                return -1;
            }
        }
    }
    return 0;
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
