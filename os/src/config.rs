//! Constants in the kernel

#[allow(unused)]

/// user app's stack size
pub const USER_STACK_SIZE: usize = 4096;
/// kernel stack size
pub const KERNEL_STACK_SIZE: usize = 4096 * 2;
/// kernel heap size
pub const KERNEL_HEAP_SIZE: usize = 0x20000;
/// the max number of apps
pub const MAX_APP_NUM: usize = 16;
/// 系统调用计数表的大小，需覆盖用到的最大 syscall id（ch3 只用到 410，
/// 留出余量以便后续章节复用）
pub const MAX_SYSCALL_NUM: usize = 512;
/// base_addr(changed) of app
pub const APP_BASE_ADDRESS: usize = 0x80400000;
/// size limit of app
pub const APP_SIZE_LIMIT: usize = 0x20000;

/// clock frequency
pub const CLOCK_FREQ: usize = 12500000;
/// the physical memory end
pub const MEMORY_END: usize = 0x88000000;
