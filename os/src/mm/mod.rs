//! Memory management implementation
//!
//! SV39 page-based virtual-memory architecture for RV64 systems, and
//! everything about memory management, like frame allocator, page table,
//! map area and memory set, is implemented here.
//!
//! Every task or process has a memory_set to control its virtual memory.

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

use address::VPNRange;
pub use address::{PhysAddr, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
pub use frame_allocator::{frame_alloc, frame_dealloc, FrameTracker};
pub use memory_set::remap_test;
pub use memory_set::{kernel_token, MapPermission, MemorySet, KERNEL_SPACE};
use page_table::PTEFlags;
pub use page_table::{
    translated_byte_buffer, translated_ref, translated_refmut, translated_str, PageTable,
    PageTableEntry, UserBuffer, UserBufferIterator,
};

/// 往用户虚拟地址写一个字节，地址未映射或不可写时返回 false。
///
/// syscall 层拿到的都是用户虚拟地址，不能直接解引用；
/// 这里统一做地址翻译和权限检查。
pub fn user_write_u8(token: usize, addr: usize, value: u8) -> bool {
    let page_table = PageTable::from_token(token);
    let va = VirtAddr::from(addr);
    match page_table.translate(va.floor()) {
        // translate 会返回「存在但无效」的叶子表项，必须再看 valid 位；
        // 同时要求 U 位，否则超出 Sv39 范围的地址会因索引截断撞上 trampoline 的页表项
        Some(pte) if pte.is_valid() && pte.is_user() && pte.writable() => {
            pte.ppn().get_bytes_array()[va.page_offset()] = value;
            true
        }
        _ => false,
    }
}

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.exclusive_access().activate();
}
