//! File and filesystem-related syscalls
use crate::fs::{open_file, OpenFlags, Stat, StatMode, find_inode_by_path, OSInode, insert_dir_entry, delete_dir_entry};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};
use crate::syscall::os_data_copy_to_user;
use easy_fs::{DiskInode, BLOCK_SZ};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

fn cal_inode_id(block_id: usize, block_offset: usize, inode_area_start_block: usize) -> usize {
    let size_of_disk_inode = core::mem::size_of::<DiskInode>();
    let disk_inodes_per_block = BLOCK_SZ / size_of_disk_inode;
    let inode_id = (block_id - inode_area_start_block as usize) * disk_inodes_per_block + block_offset / size_of_disk_inode;
    inode_id
}

/// YOUR JOB: Implement fstat.
pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    trace!(
        "kernel:pid[{}] sys_fstat",
        current_task().unwrap().pid.0
    );
    let task = current_task().unwrap();
    let task_inner = task.inner_exclusive_access();
    if _fd >= task_inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &task_inner.fd_table[_fd] {
        // 只有这里 clone 了 drop(task_inner) 才能成功
        let file = file.clone();
        drop(task_inner);
        if let Some(os_inode) = file.as_any().downcast_ref::<OSInode>() { // 运行时多态
            // 计算 inode_id
            let inode = {
                // 在这之前不 drop(task_inner) 的话会冲突
                let inner = os_inode.inner.exclusive_access();
                inner.inode.clone()
            };
            let block_id = inode.get_block_id() as usize;
            let block_offset = inode.get_block_offset() as usize;
            let inode_id = cal_inode_id(
                block_id, block_offset, 
                inode.fs.lock().inode_area_start_block as usize
            );
            let mut stat = Stat::new(0, inode_id as u64, StatMode::NULL, 1);
            inode.read_disk_inode(|disk_inode| {
                if disk_inode.is_dir() {
                    stat.mode = StatMode::DIR;
                } else if disk_inode.is_file() {
                    stat.mode = StatMode::FILE;
                }
                stat.nlink = disk_inode.nlink;
            });
            let size_of_stat = core::mem::size_of::<Stat>();
            os_data_copy_to_user(&stat as *const Stat as *const u8, _st as *const u8, size_of_stat);
            0
        } else {
            -1
        }
    } else {
        -1
    }
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let old_str = translated_str(token, _old_name); // alloc::string::String
    let new_str = translated_str(token, _new_name);
    if old_str == new_str { // 链接同名文件
        return -1;
    }
    // 借鉴 Inode create 的逻辑
    if let Some(inode) = find_inode_by_path(&old_str) {
        let inode = inode.clone();
        let block_id = inode.get_block_id() as usize;
        let block_offset = inode.get_block_offset() as usize;
        let inode_id = cal_inode_id(
            block_id, block_offset, 
            inode.fs.lock().inode_area_start_block as usize
        );
        inode.modify_disk_inode(|disk_inode| {
            // increase link count
            disk_inode.nlink += 1;
        });
        insert_dir_entry(&new_str, inode_id as u32);
        0
    } else { // 原文件不存在
        -1
    }
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_unlinkat",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let name_str = translated_str(token, _name);
    if let Some(inode) = find_inode_by_path(&name_str) {
        let inode = inode.clone();
        let block_id = inode.get_block_id() as usize;
        let block_offset = inode.get_block_offset() as usize;
        let inode_id = cal_inode_id(
            block_id, block_offset, 
            inode.fs.lock().inode_area_start_block as usize
        );
        let mut need_delete = false;
        inode.modify_disk_inode(|disk_inode| {
            // decrease link count
            disk_inode.nlink -= 1;
            if disk_inode.nlink == 0 {
                need_delete = true;
            }
        });
        delete_dir_entry(&name_str, inode_id as u32);
        if need_delete {
            inode.clear();
        }
        0
    } else { // 文件不存在
        -1
    }
}
