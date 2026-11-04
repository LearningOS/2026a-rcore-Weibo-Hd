//! 基于银行家算法的死锁检测
//!
//! 进程可以在运行时打开这个功能。开启后，`mutex_lock` 和 `semaphore_down`
//! 在真正阻塞之前先做一次安全性检查：把"当前线程还要申请这个资源"计入需求，
//! 再看存不存在一个安全序列能让所有线程都跑完。不存在就说明这次申请会把
//! 进程带进死锁，于是直接拒绝，返回 `-0xDEAD`。
//!
//! 资源模型的对应关系：
//! - 每个信号量是一类资源，`count` 就是可用数量，一个线程可以持有多份；
//! - 每个互斥量也是一类资源，但容量固定为 1。

use crate::sync::{Mutex, Semaphore};
use crate::task::TaskControlBlock;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

/// 本次申请针对的资源
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resource {
    /// 信号量，参数是它在进程信号量表里的下标
    Semaphore(usize),
    /// 互斥量，参数是它在进程互斥量表里的下标
    Mutex(usize),
}

/// 取一个线程的 tid。线程已经退出（`res` 被回收）时返回 None。
fn task_tid(task: &Arc<TaskControlBlock>) -> Option<usize> {
    task.inner_exclusive_access().res.as_ref().map(|r| r.tid)
}

/// 判断线程 `tid` 再申请一次 `req` 之后，进程是否仍处于安全状态。
///
/// 返回 false 表示找不到安全序列，这次申请应当被拒绝。
pub(crate) fn request_is_safe(
    thread_num: usize,
    sem_list: &[Option<Arc<Semaphore>>],
    mutex_list: &[Option<Arc<dyn Mutex>>],
    req: Resource,
    tid: usize,
) -> bool {
    // 资源统一编号：先信号量，后互斥量
    let sems: Vec<&Arc<Semaphore>> = sem_list.iter().flatten().collect();
    let mtxs: Vec<&Arc<dyn Mutex>> = mutex_list.iter().flatten().collect();
    let m = sems.len() + mtxs.len();
    if m == 0 || thread_num == 0 {
        return true;
    }

    let mut avail = vec![0usize; m];
    let mut alloc = vec![vec![0usize; m]; thread_num];
    let mut need = vec![vec![0usize; m]; thread_num];

    // 信号量：count 是剩余可用份数，alloc 记着每个线程拿了多少份
    for (j, sem) in sems.iter().enumerate() {
        let inner = sem.inner.exclusive_access();
        avail[j] = if inner.count > 0 {
            inner.count as usize
        } else {
            0
        };
        for (i, v) in inner.alloc.iter().enumerate() {
            if i < thread_num {
                alloc[i][j] = *v as usize;
            }
        }
        for task in inner.wait_queue.iter() {
            if let Some(t) = task_tid(task) {
                if t < thread_num {
                    need[t][j] = 1;
                }
            }
        }
    }
    // 互斥量：容量为 1，占用时可用数量就是 0
    for (k, mtx) in mtxs.iter().enumerate() {
        let j = sems.len() + k;
        avail[j] = if mtx.is_locked() { 0 } else { 1 };
        if let Some(owner) = mtx.owner() {
            if owner < thread_num {
                alloc[owner][j] = 1;
            }
        }
        for t in mtx.waiters() {
            if t < thread_num {
                need[t][j] = 1;
            }
        }
    }

    // 找到本次申请的编号
    let req_index = match req {
        Resource::Semaphore(id) => sem_list[..id].iter().flatten().count(),
        Resource::Mutex(id) => sems.len() + mutex_list[..id].iter().flatten().count(),
    };
    if req_index >= m {
        return true;
    }
    // 还有余量就是立刻拿到，不可能因为这次申请而死锁
    if avail[req_index] > 0 {
        return true;
    }
    // 否则把"我还需要它"记进 Need，再判断有没有安全序列
    if tid < thread_num {
        need[tid][req_index] = 1;
    }

    // 银行家算法
    let mut work = avail;
    let mut finish = vec![false; thread_num];
    loop {
        let mut progressed = false;
        for i in 0..thread_num {
            if finish[i] {
                continue;
            }
            if (0..m).all(|j| need[i][j] <= work[j]) {
                for j in 0..m {
                    work[j] += alloc[i][j];
                }
                finish[i] = true;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    finish.iter().all(|&f| f)
}
