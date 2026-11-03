//! Process management syscalls
//!
use alloc::sync::Arc;

use crate::{
    config::PAGE_SIZE,
    fs::{open_file, OpenFlags},
    mm::{translated_refmut, translated_str, user_write_u8, MapPermission, VirtAddr},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    //trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    //trace!("kernel: sys_waitpid");
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// 获取当前时间，写进用户给的 `TimeVal`
///
/// `ts` 是用户虚拟地址，要先经页表翻译。`TimeVal` 占 16 字节，
/// 可能横跨两个页，所以逐字节写，由 `user_write_u8` 处理跨页。
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel:pid[{}] sys_get_time", current_task().unwrap().pid.0);
    let token = current_user_token();
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &time_val as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    for (offset, byte) in bytes.iter().enumerate() {
        if !user_write_u8(token, ts as usize + offset, *byte) {
            return -1;
        }
    }
    0
}

/// 建立匿名内存映射
///
/// `prot` 是 Linux 的位定义（bit0 可读、bit1 可写、bit2 可执行），
/// 转成内核的 `MapPermission` 时别忘了补 U 位，否则用户态一访问就缺页。
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel:pid[{}] sys_mmap", current_task().unwrap().pid.0);
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if prot & !0x7 != 0 || prot & 0x7 == 0 {
        return -1;
    }

    let mut permission = MapPermission::U;
    if prot & 0b001 != 0 {
        permission |= MapPermission::R;
    }
    if prot & 0b010 != 0 {
        permission |= MapPermission::W;
    }
    if prot & 0b100 != 0 {
        permission |= MapPermission::X;
    }

    let len = (len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    if start.checked_add(len).is_none() {
        return -1;
    }

    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if inner.memory_set.mmap(VirtAddr::from(start), len, permission) {
        0
    } else {
        -1
    }
}

/// 取消 [start, start + len) 的映射；区间内有未映射页则返回 -1 且不做修改
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_munmap", current_task().unwrap().pid.0);
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if start.checked_add(len).is_none() {
        return -1;
    }
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if inner.memory_set.munmap(VirtAddr::from(start), len) {
        0
    } else {
        -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// 创建一个执行指定程序的新进程，成功返回子进程 pid，失败返回 -1。
///
/// ch6 的程序都放在文件系统里，所以先从根目录读出 elf 内容，再建进程。
/// 和 fork 不同，spawn 不需要复制父进程的地址空间。
pub fn sys_spawn(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(app_inode) = open_file(path.as_str(), OpenFlags::RDONLY) {
        let all_data = app_inode.read_all();
        let current = current_task().unwrap();
        let new_task = current.spawn(all_data.as_slice());
        let new_pid = new_task.getpid();
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    -1
}
