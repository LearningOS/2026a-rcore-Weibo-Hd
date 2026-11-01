# 实验一　多道程序与分时多任务（ch3）

姓名：王子谦
GitHub：Weibo-Hd
日期：2026-10-26

## 一、实现的功能

这一章要在 ch3 的框架上补一个 `sys_trace` 系统调用，编号 410。它有三种用法：读用户地址上的一个字节、往用户地址写一个字节、查某个系统调用被调用过多少次。

统计功能我在 TCB 里加了一个数组当计数器，下标就是 syscall id。计数放在 `syscall()` 分发之前做，因为实验要求「本次调用也计入统计」；要是放在 dispatch 后面，查 trace 自己的次数时会永远少一次。

读写功能因为 ch3 还没有地址空间，直接用裸指针访问，没有做地址合法性检查，这部分留到 ch4 再补。

## 二、简答作业

### 2.1 用户态执行特权操作会发生什么

实验用的固件是仓库自带的 RustSBI v0.4.1（jump 模式），加载在 `0x80000000`，内核入口 `0x80200000`。这一点可以在 `bootloader/rustsbi-qemu.toml` 里查到。

用 `make run BASE=2` 跑全套 13 个测例（3 个 `ch2b_bad_*` 加 10 个正常测例）。ch3 是分时轮转，几个应用交替执行，所以这些「杀进程」的消息会夹在 `power_3`、`yield` 之类的正常输出中间，不是连续出现的。

| 测例 | 干了什么 | 内核实际输出 |
| --- | --- | --- |
| `ch2b_bad_address` | 往 `0x0` 写一个字节 | `[kernel] PageFault in application, bad addr = 0x0, bad instruction = 0x804003a4, kernel killed it.` |
| `ch2b_bad_instructions` | 在 U 态执行 `sret` | `[kernel] IllegalInstruction in application, kernel killed it.` |
| `ch2b_bad_register` | 在 U 态执行 `csrr sstatus` | `[kernel] IllegalInstruction in application, kernel killed it.` |

后两个的输出一模一样，内核没有打印是哪个应用，所以从日志里分不出哪条对哪条；但一共出现两次，和两个测例的数量对得上。

`sret` 和 `csrr sstatus` 那两个都是非法指令异常。CSR 和 `sret` 都是 S 态才允许碰的东西，U 态执行会被硬件直接拦下，`scause` 是 IllegalInstruction，内核把任务杀掉，接着跑下一个。

第三个要说清楚：它**不是缺页**。ch3 根本没开页表（`satp` 为 0，裸机地址模式），不可能产生缺页异常。真正发生的是访问 `0x0` 这个物理地址时被更底层拦下了，属于 Store Access Fault。至于具体是 PMP 还是总线拒绝的，内核这一层看不出来——启动日志里 RustSBI 打印的 PMP 表显示低地址段是 RWX，所以不一定赖 PMP。麻烦的地方在于 `trap_handler` 把 `StoreFault` 和 `StorePageFault` 归到了同一个分支，打印出来一律叫 `PageFault in application`，名字和实际异常类型对不上，不过处理方式（杀进程、切下一个任务）是对的。

三个测例都没有打印出它们自己 `panic!` 里的 `FAIL: T.T`，说明异常确实在进入用户代码的 panic 之前就被内核接管了。

### 2.2 trap.S 里两个函数在做什么

**(1) 刚进 `__restore` 时 `sp` 是什么，它有哪两种用法**

`sp` 指向内核栈上那个 34×8 字节的 `TrapContext` 的起始地址，也就是 `__alltraps` 刚分配出来的那块区域。

两种用法：

- 处理完 trap 直接返回。`trap_handler` 返回后往下走就是 `__restore`，此时 `sp` 就是刚才压好的上下文，恢复完 `sret` 回到用户程序被打断的地方。
- 任务切换之后恢复。`__switch` 切到另一个任务时，会把那个任务 TCB 里存的 `task_cx` 恢复出来，它的 `ra` 指向 `__restore`、`sp` 指向该任务内核栈上的 `TrapContext`。于是从 `__restore` 开始执行，进入的是那个任务的用户态。

**(2) L43–L48 为什么单独处理那三个寄存器**

```riscv
ld t0, 32*8(sp)   # sstatus
ld t1, 33*8(sp)   # sepc
ld t2, 2*8(sp)    # 用户 sp
csrw sstatus, t0
csrw sepc, t1
csrw sscratch, t2
```

- `sstatus`：`sret` 的行为全靠它。SPP 位决定返回后是 U 态还是 S 态，SPIE 位决定返回后要不要重新开中断。不恢复它，回不到用户态。
- `sepc`：返回地址。`sret` 会把 PC 设成它，也就是用户程序被打断的那条指令。
- `sscratch`：存用户栈指针。恢复它是为了下一次 trap 时，`__alltraps` 开头那条 `csrrw` 还能正常换栈。

**(3) L50–L56 为什么跳过 x2 和 x4**

x2 是 `sp`。这时候 `sp` 正被当成 `TrapContext` 的基址用，循环里把它覆盖掉就没法继续往下取了。用户栈指针另外存在 `sscratch` 和第 2 个槽位里，最后靠一条 `csrrw` 换回来。

x4 是 `tp`，线程指针。用户程序不用它，内核这边可能拿它指向 hart 相关的数据，存了也没意义，所以跳过。

**(4) L60 `csrrw sp, sscratch, sp` 之后**

- `sp`：变成用户栈指针，回到用户栈上了。
- `sscratch`：变成内核栈顶（前面 `addi sp, sp, 34*8` 已经把 `TrapContext` 释放了）。

**(5) 状态切换发生在哪条指令，为什么之后就进了用户态**

在最后的 `sret`。

`sret` 干三件事：按 `sstatus.SPP` 选返回的特权级，把 `sepc` 装进 PC，把 `SPIE` 复制回 `SIE`。前面 L43–L48 已经把 SPP 恢复成陷入前的值（从 U 态来就是 0），所以 `sret` 一执行，CPU 就从 S 态回到 U 态，从用户程序被打断处接着跑。

**(6) L13 `csrrw sp, sscratch, sp` 之后**

这是 `__alltraps` 开头那条（在文件靠前的位置，L13）：

- `sp`：内核栈指针，指向当前任务内核栈的栈顶，紧接着就在这上面分配 `TrapContext`。
- `sscratch`：用户栈指针，先寄存在这里，后面写进 `TrapContext` 的 x2 槽位。

**(7) 从 U 态进 S 态是哪条指令**

用户程序自己执行的 `ecall`。它产生 `Environment call from U-mode` 异常，CPU 跳到 `stvec` 指向的 `__alltraps`，从这里开始就是内核代码了。

## 三、荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   > DeepSeek（AI 助手）。交流内容主要是开发环境的搭建与网络问题排查、`sys_trace` 计数时机的选择、以及三个 `ch2b_bad_*` 测例的输出分析。相关结论我都自己跑过一遍再写进代码和本文档。

2. 此外，我也参考了 **以下资料**，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   > - rCore-Tutorial-Guide 实验指导书第三章：<https://learningos.cn/rCore-Tutorial-Guide/>
   > - rCore-Tutorial-Book 第三版（原理部分）：<https://rcore-os.cn/rCore-Tutorial-Book-v3/>
   > - 作业仓库 README 与 `.github/workflows/build.yml`
   > - rCore-Tutorial-Checker 的 `check/ch3.py`（对照检查项确认实现范围）
   > - `bootloader/rustsbi-qemu.toml`（确认固件版本）

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

## 四、其他

这章代码量不大，真正要留意的是「本次调用也计入统计」这句话。计数如果写在 `match` 后面，`count_syscall(SYSCALL_TRACE)` 就会永远差 1，`assert_eq!(2, ...)` 过不去。放到 `syscall()` 入口处，逻辑最干净，以后新加的系统调用也能自动统计到。

另外 ch3 的 `sys_trace` 直接解引用用户传进来的裸指针，看着能用，但其实是把内核地址空间完全暴露给用户程序了。等 ch4 上了页表，这里必须补地址检查，不然就是一个现成的提权口子。
