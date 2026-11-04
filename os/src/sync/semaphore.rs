//! Semaphore

use crate::sync::UPSafeCell;
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc, vec::Vec};

/// 当前线程的 tid
fn current_tid() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid
}

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
    /// 每个线程当前持有本信号量的份数，下标是 tid，死锁检测时用来填 Allocation
    pub alloc: Vec<u32>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                    alloc: Vec::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) {
        trace!("kernel: Semaphore::up");
        let tid = current_tid();
        let mut inner = self.inner.exclusive_access();
        // 释放的线程把自己名下的那一份减掉
        if let Some(v) = inner.alloc.get_mut(tid) {
            if *v > 0 {
                *v -= 1;
            }
        }
        inner.count += 1;
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                wakeup_task(task);
            }
        }
    }

    /// down operation of semaphore
    pub fn down(&self) {
        trace!("kernel: Semaphore::down");
        let tid = current_tid();
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
            // 被唤醒说明拿到了一份
            let mut inner = self.inner.exclusive_access();
            Self::add_alloc(&mut inner.alloc, tid);
        } else {
            Self::add_alloc(&mut inner.alloc, tid);
        }
    }

    /// 记一笔"线程 tid 又拿走了一份"
    fn add_alloc(alloc: &mut Vec<u32>, tid: usize) {
        if alloc.len() <= tid {
            alloc.resize(tid + 1, 0);
        }
        alloc[tid] += 1;
    }
}
