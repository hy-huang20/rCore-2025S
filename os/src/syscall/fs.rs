//! File and filesystem-related syscalls
use crate::fs::{open_file, OpenFlags, Stat, StatMode, find_inode, OSInode};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};
use crate::syscall::os_data_copy_to_user;
use easy_fs::{DIRENT_SZ, DirEntry, DiskInode, BLOCK_SZ};
use core::{str, slice};

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
    println!("here 1");
    if let Some(file) = &task_inner.fd_table[_fd] {
        // 只有这里 clone 了 drop(task_inner) 才能成功
        let file = file.clone();
        drop(task_inner);
        println!("here 2");
        if let Some(os_inode) = file.as_any().downcast_ref::<OSInode>() {
            println!("here 3");
            // 计算 inode_id
            let inode = {
                // 在这之前不 drop(task_inner) 的话会冲突
                let inner = os_inode.inner.exclusive_access();
                inner.inode.clone()
            };
            let block_id = inode.get_block_id() as usize;
            let block_offset = inode.get_block_offset() as usize;
            let fs = inode.fs.lock();
            let size_of_disk_inode = core::mem::size_of::<DiskInode>();
            let disk_inodes_per_block = BLOCK_SZ / size_of_disk_inode;
            let inode_id = (block_id - fs.inode_area_start_block as usize) * disk_inodes_per_block + block_offset / size_of_disk_inode;
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

fn ptr_const_u8_to_ref_str<'a>(ptr: *const u8) -> &'a str {
    let mut len = 0;
    while unsafe { *ptr.add(len) } != 0 { // 已知 path 以 \0 结尾
        len += 1;
    }
    str::from_utf8(unsafe {
        slice::from_raw_parts(ptr, len)
    }).unwrap()
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_linkat",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let old_str = translated_str(token, _old_name);
    let new_str = translated_str(token, _new_name);
    if old_str == new_str { // 链接同名文件
        return -1;
    }    
    // 借鉴 Inode create 的逻辑
    if let Some(inode) = find_inode(_old_name) {
        let inode_id: u32 = inode.get_block_id();
        let mut fs = inode.fs.lock();
        inode.modify_disk_inode(|disk_inode| {
            // increase link count
            disk_inode.nlink += 1;
            // append file in the dirent
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            inode.increase_size(new_size as u32, disk_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(ptr_const_u8_to_ref_str(_new_name), inode_id);
            disk_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &inode.block_device,
            );
        });
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
    -1
}
