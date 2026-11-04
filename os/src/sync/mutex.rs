//! Mutex (spin-like and blocking(sleep))

use super::UPSafeCell;
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
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

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self);
    /// Unlock the mutex
    fn unlock(&self);
    /// 死锁检测用：返回当前持有该锁的线程 tid，未加锁时为 None
    fn owner(&self) -> Option<usize>;
    /// 死锁检测用：返回正在等待该锁的线程 tid 列表
    fn waiters(&self) -> Vec<usize>;
    /// 死锁检测用：该锁是否已被占用
    fn is_locked(&self) -> bool;
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    inner: UPSafeCell<MutexSpinInner>,
}

struct MutexSpinInner {
    locked: bool,
    owner: Option<usize>,
    /// 正在自旋等待的线程，只为死锁检测服务
    waiters: Vec<usize>,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new() -> Self {
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexSpinInner {
                    locked: false,
                    owner: None,
                    waiters: Vec::new(),
                })
            },
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) {
        trace!("kernel: MutexSpin::lock");
        let tid = current_tid();
        loop {
            let mut inner = self.inner.exclusive_access();
            if inner.locked {
                inner.waiters.push(tid);
                drop(inner);
                suspend_current_and_run_next();
                let mut inner = self.inner.exclusive_access();
                if let Some(pos) = inner.waiters.iter().position(|&t| t == tid) {
                    inner.waiters.remove(pos);
                }
                continue;
            } else {
                inner.locked = true;
                inner.owner = Some(tid);
                return;
            }
        }
    }

    fn unlock(&self) {
        trace!("kernel: MutexSpin::unlock");
        let mut inner = self.inner.exclusive_access();
        inner.locked = false;
        inner.owner = None;
    }

    fn owner(&self) -> Option<usize> {
        self.inner.exclusive_access().owner
    }

    fn waiters(&self) -> Vec<usize> {
        self.inner.exclusive_access().waiters.clone()
    }

    fn is_locked(&self) -> bool {
        self.inner.exclusive_access().locked
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
    /// 持有锁的线程 tid，未加锁时为 None
    owner: Option<usize>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new() -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                    owner: None,
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) {
        trace!("kernel: MutexBlocking::lock");
        let tid = current_tid();
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
            // 被 unlock 唤醒时锁已经移交过来了，把持有者改成自己
            let mut mutex_inner = self.inner.exclusive_access();
            mutex_inner.owner = Some(tid);
        } else {
            mutex_inner.locked = true;
            mutex_inner.owner = Some(tid);
        }
    }

    /// unlock the blocking mutex
    fn unlock(&self) {
        trace!("kernel: MutexBlocking::unlock");
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
            mutex_inner.owner = None;
        }
    }

    fn owner(&self) -> Option<usize> {
        self.inner.exclusive_access().owner
    }

    fn waiters(&self) -> Vec<usize> {
        let inner = self.inner.exclusive_access();
        let mut v = Vec::new();
        for task in inner.wait_queue.iter() {
            if let Some(res) = task.inner_exclusive_access().res.as_ref() {
                v.push(res.tid);
            }
        }
        v
    }

    fn is_locked(&self) -> bool {
        self.inner.exclusive_access().locked
    }
}
