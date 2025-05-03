//! Process management syscalls

use core::ptr;
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next, TASK_MANAGER},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);

    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// TODO: implement the syscall
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match _trace_request {
        0 => {
            let addr = _id as *const u8;
            let val = unsafe { ptr::read(addr) as isize };
            return val;
        },
        1 => {
            let addr = _id as *mut u8;
            let data = _data as u8; // 只需要低位 1 个字节
            unsafe { ptr::write(addr, data) };
            return 0;
        },
        2 => {
            let syscall_id = _id;
            let cnt = tm_mut_ref.syscall_cnt.get(syscall_id).copied();
            let mut ret: isize = 0;
            if let Some(s) = cnt {
                ret = s;
            }
            if (syscall_id == SYSCALL_TRACE) {
                ret += 1;
            }
            return ret;
        },
        _ => return -1,
    }
    return -1;
}
