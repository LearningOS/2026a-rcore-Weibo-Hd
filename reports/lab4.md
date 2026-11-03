# 实验四　文件系统与 I/O（ch6）

姓名：王子谦
GitHub：Weibo-Hd
日期：2026-11-03

## 一、实现的功能

实现 `linkat`(37)、`unlinkat`(35)、`fstat`(80) 三个调用。为支持硬链接，easy-fs 的磁盘 inode 加了 `nlink`：建链接加一，unlink 减一，归零才回收数据块和 inode。目录项删除用末位覆盖。`fstat` 经 `File` trait 的 `stat` 方法返回 dev、ino、mode、nlink。另把上一章的 `spawn`、`get_time`、`mmap`、`munmap` 移植过来，保证前向兼容。

## 二、简答作业

### 2.1 root inode 的作用，以及它损坏后的后果

easy-fs 里没有单独的目录树结构，root inode 就是整棵树的入口。`EasyFileSystem::root_inode()` 用 `get_disk_inode_pos(0)` 算出 inode 区第 0 个 inode 的位置，把它包成一个 `Inode`。本实验只支持一级目录，所以 `open_file(name)` 实际上就是 `ROOT_INODE.find(name)`，`create`、`ls`、`link`、`unlink` 也都是以它为当前目录。

它本身是一个 Directory 类型的磁盘 inode：`size` 等于"目录项个数 × 32"，`direct` / `indirect1` / `indirect2` 指向存放目录项数组的数据块。换句话说，它既是目录本身，也是找到目录内容的唯一线索。

如果它的内容损坏，后果按损坏的字段分几种：

- `type_` 不再是 Directory：`create` 里的 `assert!(root_inode.is_dir())` 立刻 panic。
- `size` 被改大：那些从没写过的目录项会被当成有效条目，读出空名字和随机的 inode 号，`find` 有可能命中一个原本不属于这个目录的 inode；改小则是后面的文件凭空消失。
- `direct[]` 里的块号错乱：读出来的目录项其实是别的块的内容，文件名和 inode 号都是垃圾；写入时还会把真正的数据块覆盖掉。
- inode 号越界：`get_disk_inode_pos` 算出的偏移会落到 inode 区之外，把用户文件的数据当成 inode 来解释。

最直接的后果是整个文件系统不可用。内核启动时 `INITPROC` 就是从根目录读出来的（`open_file("ch6b_initproc")`），根 inode 一坏，这一步 `unwrap()` 直接 panic，第一个进程都起不来。

### 2.2 举出使用 pipe 的一个实际例子

最常见的用法是在 shell 里串命令：

```sh
cat access.log | wc -l
ls -l | grep ch6 | wc -l
```

第一个统计日志行数，第二个统计名字里带 ch6 的文件个数。`ps aux | grep nginx`、`git log --oneline | head -20`、`curl -s url | tar xz` 都是同一类。

机制上，shell 先调用 `pipe()` 拿到读写两个 fd，再 fork 出两个子进程，各自用 `dup2` 把一端接到自己的 stdout 或 stdin，然后 exec 成真正的程序。上游写满管道缓冲区就会阻塞，下游读空也会阻塞，两个程序的速率就这样被自动对齐，中间不需要落盘，数据也不经过 shell 本身。

顺带一个细节：`yes | head` 里 `head` 读够 10 行就退出，管道的读端全部关闭，`yes` 再写就会收到 SIGPIPE 被杀掉。管道不只是传数据，还顺带传递了"对面已经不要了"这个信息。

### 2.3 设计一个比 pipe 更易用的多进程通信机制

pipe 的两个限制很明确：它是匿名的，只能靠 fork 时的 fd 继承传给有亲缘关系的进程；而且一对一，N 个进程互相通信要建 N(N-1)/2 条管道。

我的方案是给内核加一类**有名字的消息通道**：

- 全局维护一张 `CHANNEL_TABLE: BTreeMap<String, Arc<Channel>>`；
- `channel_open(name, flags)` 返回一个 fd，指向通道的一个端点，名字不存在且带 CREATE 标志时新建；
- 每个 `Channel` 内部是一个存放定长消息的环形缓冲区（总容量设上限，比如 64 KiB），外加读、写两个等待队列和 `readers` / `writers` 两个引用计数；
- `send(fd, buf, len)` 在缓冲满时把自己挂到写等待队列上睡眠，写入后唤醒一个读者；`recv` 反过来。空/满的判断和挂队列必须在同一把锁内完成，否则会出现"刚判断为空、还没挂上去，对面就写完了"的丢唤醒；
- 最后一个 fd 关闭时把通道从全局表摘掉并释放缓冲页。进程退出时 `fd_table` 的析构会统一关闭，效果等同于 close；
- 通道记录创建者的 uid，`open` 时校验，避免任意进程往别人的通道里塞数据。

这样一来，两个进程只要约定一个名字就能通信，不需要亲缘关系，也可以多对多。共享内存比消息队列快，但要额外配信号量做同步，还得处理"进程崩溃时锁没释放"的问题；把同步收进内核、只暴露 send/recv，接口简单很多，适合课程这个规模。真需要传大块数据时，可以在通道之上再挂一层共享内存段，用通道传控制消息。

## 三、荣誉准则

1. 在完成本次实验的过程（含此前学习的过程）中，我曾分别与 **以下各位** 就（与本次实验相关的）以下方面做过交流，还在代码中对应的位置以注释形式记录了具体的交流对象及内容：

   > DeepSeek（AI 助手）。交流内容主要是 `nlink` 字段怎么塞进 128 字节的 inode、`unlink` 的目录项删除与块回收顺序、`fstat` 如何绕开 `Arc<dyn File>` 无法向下转型的问题，以及本地测例超时的排查思路。相关结论我都自己跑过测例验证后再写进代码和本文档。

2. 此外，我也参考了 **以下资料**，还在代码中对应的位置以注释形式记录了具体的参考来源及内容：

   > - rCore-Tutorial-Guide 实验指导书第六章、第七章：<https://learningos.cn/rCore-Tutorial-Guide/>
   > - rCore-Tutorial-Book 第三版第六、七章：<https://rcore-os.cn/rCore-Tutorial-Book-v3/>
   > - 作业仓库 README 与 `.github/workflows/build.yml`
   > - rCore-Tutorial-Checker 的 `check/ch6.py`（对照检查项确认实现范围）
   > - easy-fs 源码中 `DiskInode`、`Inode`、`Bitmap` 的既有实现（确认布局约束与回收接口）

3. 我独立完成了本次实验除以上方面之外的所有工作，包括代码与文档。我清楚地知道，从以上方面获得的信息在一定程度上降低了实验难度，可能会影响起评分。

4. 我从未使用过他人的代码，不管是原封不动地复制，还是经过了某些等价转换。我未曾也不会向他人（含此后各届同学）复制或公开我的实验代码，我有义务妥善保管好它们。我提交至本实验的评测系统的代码，均无意于破坏或妨碍任何计算机系统的正常运转。我清楚地知道，以上情况均为本课程纪律所禁止，若违反，对应的实验成绩将按“-100”分计。

## 四、其他

### 4.1 128 字节是个硬约束

`DiskInode` 是 `#[repr(C)]` 的，`BLOCK_SZ` 是 512，一块正好放 4 个 inode，所有偏移都建立在这个大小上。原来的字段是：

```
4(size) + 28×4(direct) + 4(indirect1) + 4(indirect2) + 1(type_) = 125 → 对齐到 128
```

尾部本来就有 3 字节填充。加 `nlink` 最省事的写法是直接追加一个 `u32`，但那样结构体变成 132 字节，一块只放得下 3 个 inode，`INODES_PER_BLOCK` 和每个 inode 的偏移全变，镜像布局和内核代码必须严格同步改，一旦漏改一处，读出来的 inode 就是错位的。

我选了另一条路：把直接块从 28 个减到 27 个，腾出 4 字节，正好塞下 `nlink`，总大小还是 128。代价是单文件直接寻址的容量从 14336 字节降到 13824 字节，超过这个长度就要多走一层间接索引，功能上没有区别。

顺带算了一下测例的实际规模：`ch6_file3` 每个文件写 50 次、每次 2900 字节，共 145000 字节，需要 284 个数据块。直接块 27 个加一级间接 128 个一共只能覆盖 79360 字节，剩下的 65640 字节走二级间接，也就是 129 个数据块、2 个二级索引块。这块路径能跑通，说明 `increase_size` / `get_block_id` 在多级索引上的配合是对的。

### 4.2 又一次栽在 missing_docs 上

上一章为了在 syscall 层用 `PageTable::from_token`，把 `PageTable` 用 `pub use` 导了出去，结果连带暴露了没写文档的 bitflags 常量，内核编译失败。这次换成 `ROOT_INODE`：源码里看它只是一个 `lazy_static!` 的静态变量，但宏展开后会生成一个**同名结构体**，一旦被 `pub use` 出去就成了公开 API，同样需要文档。

最后按 `open_file` 的样子改成两个普通函数：

```rust
pub fn link_file(old_name: &str, new_name: &str) -> bool
pub fn unlink_file(name: &str) -> bool
```

规律算是记下了：`pub use` 不只是换个可见性，它会把内部类型一起拖进文档检查的范围。

### 4.3 本地"卡住"不等于死锁

本地跑检查器时，`ch6_file3` 打印到 `test iteration 5` 就没有下文了，30 秒后被 `timeout` 杀掉，日志里既没有 panic 也没有内核报错。第一反应是死锁——毕竟整个内核一声不吭地停住，很像在自旋锁上转圈。

但把同一份代码推上去，CI 上 `Test passed: 31/31`，一个测例都没漏。差别在磁盘：检查器给 ch6 的 QEMU 只留了 30 秒（`overwrite/Makefile-ch6` 里的 `timeout --foreground 30s`），而本地的仓库在 `/mnt/f`，也就是 WSL 挂载的 Windows 盘，QEMU 对 `fs.img` 的每一次块读写都要穿过这层文件系统。`ch6_file3` 十个来回要写和删十份 141 KiB 的文件，光是这些块 I/O 就吃掉了全部时间预算。

结论是这章的检查器对 I/O 速度敏感，本地判 pass/fail 不太可靠，得看 CI 的结果。以后再遇到"跑到一半不动了、又没有报错"，除了怀疑锁，还得先看一眼是不是被超时掐掉的。
