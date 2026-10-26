//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, get_current_syscall_count, suspend_current_and_run_next},
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

/// 追踪系统调用。
///
/// 根据 `trace_request` 分为三种功能：
/// - `0`：把 `id` 当作 `*const u8`，读取该地址的一个字节并返回；忽略 `data`
/// - `1`：把 `id` 当作 `*mut u8`，把 `data` 的最低字节写入该地址，返回 0
/// - `2`：查询当前任务调用编号为 `id` 的系统调用的次数并返回
/// - 其他值：返回 -1
///
/// ch3 还没有地址空间隔离，所以这里不做指针合法性检查
/// （这是实验文档明确允许的，ch4 引入虚存后才需要补）。
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => unsafe { (id as *const u8).read_volatile() as isize },
        1 => {
            unsafe { (id as *mut u8).write_volatile(data as u8) };
            0
        }
        2 => get_current_syscall_count(id) as isize,
        _ => -1,
    }
}
