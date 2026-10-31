//! Process management syscalls
use crate::config::PAGE_SIZE;
use crate::mm::{MapPermission, PageTable, VirtAddr};
use crate::task::{
    change_program_brk, current_user_token, exit_current_and_run_next,
    get_current_syscall_count, suspend_current_and_run_next, with_current_memory_set,
};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// 从用户虚拟地址读一个字节。
/// 地址没有映射（或被映射成不可读）时返回 None，不panic。
fn user_read_u8(token: usize, addr: usize) -> Option<u8> {
    let page_table = PageTable::from_token(token);
    let va = VirtAddr::from(addr);
    let pte = page_table.translate(va.floor())?;
    if !pte.readable() {
        return None;
    }
    Some(pte.ppn().get_bytes_array()[va.page_offset()])
}

/// 往用户虚拟地址写一个字节。地址不可写时返回 false。
fn user_write_u8(token: usize, addr: usize, value: u8) -> bool {
    let page_table = PageTable::from_token(token);
    let va = VirtAddr::from(addr);
    match page_table.translate(va.floor()) {
        Some(pte) if pte.writable() => {
            pte.ppn().get_bytes_array()[va.page_offset()] = value;
            true
        }
        _ => false,
    }
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

/// get time with second and microsecond
///
/// `ts` 是用户虚拟地址，必须先经页表翻译才能写。
/// `TimeVal` 占 16 字节，正好可能横跨两个页（比如起始地址落在页尾附近），
/// 所以这里逐字节写，由 `user_write_u8` 自己处理跨页。
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
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

/// trace 系统调用。
///
/// 和 ch3 的区别在于现在有了地址空间：
/// - 读/写都要先经页表翻译，未映射或权限不符时返回 -1，而不是直接解引用
/// - `trace_request == 2` 的行为不变，仍然是查当前任务的系统调用计数
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    let token = current_user_token();
    match trace_request {
        0 => match user_read_u8(token, id) {
            Some(byte) => byte as isize,
            None => -1,
        },
        1 => {
            if user_write_u8(token, id, data as u8) {
                0
            } else {
                -1
            }
        }
        2 => get_current_syscall_count(id) as isize,
        _ => -1,
    }
}

/// 建立匿名内存映射。
///
/// `prot` 用的是 Linux 那套位定义（bit0 可读、bit1 可写、bit2 可执行），
/// 和内核 `MapPermission` 的位定义不是一回事，必须显式转换；
/// 另外别忘了加上 `MapPermission::U`，否则用户态访问会直接缺页。
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    // 只允许低三位，且不能一位都不选（这样的内存没有意义）
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

    // len 直接按页向上取整
    let len = (len + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    if start.checked_add(len).is_none() {
        return -1;
    }

    if with_current_memory_set(|ms| ms.mmap(VirtAddr::from(start), len, permission)) {
        0
    } else {
        -1
    }
}

/// 取消 [start, start + len) 的映射。
/// 区间里只要有一页没映射过就返回 -1，且不做任何修改。
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if start % PAGE_SIZE != 0 {
        return -1;
    }
    if start.checked_add(len).is_none() {
        return -1;
    }
    if with_current_memory_set(|ms| ms.munmap(VirtAddr::from(start), len)) {
        0
    } else {
        -1
    }
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
