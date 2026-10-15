# DPDK Async Runtime & ICMP Ping Client

用 Rust 从零实现的单核 kernel-bypass 异步运行时，在其上跑 64 路并发 ICMP Ping 客户端（A），并提供手写 busy-poll 版本（B）作为对照，通过 **A 减 B** 量化异步抽象层的纳秒级开销。

## 架构

```
┌─────────────────────────────────────────────────────────┐
│                    async-ping (A)                        │
│  ping_session() ── spawn ──> Executor                    │
│       │                              │                   │
│       │ wait_reply().await           │ run_ready()       │
│       ▼                              ▼                   │
│  ┌─────────┐  poll_once()   ┌──────────────────┐         │
│  │ Reactor │ ──────────────>│ rx_burst + parse │         │
│  │         │ <──────────────│ wake session     │         │
│  └─────────┘                └──────────────────┘         │
│       │                                                   │
│       │ send_packet()                                     │
│       ▼                                                   │
│  ┌─────────────────────────────────────────────┐         │
│  │         DPDK Safe Wrapper (dpdk-runtime)     │         │
│  │  Eal / EthDev / Mempool / Mbuf               │         │
│  └─────────────────────────────────────────────┘         │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│                    raw-ping (B)                          │
│  64-session state machine (Idle/Waiting)                 │
│  loop { tx_burst(sessions ready) ; rx_burst ; parse }    │
└─────────────────────────────────────────────────────────┘
```

### 运行时组件 (`dpdk-runtime`)

| 模块 | 职责 |
|------|------|
| `rt/executor` | 单线程 executor，维护 ready queue，只轮询 `is_scheduled()` 的 task |
| `rt/task` | 封装 `Pin<Box<dyn Future>>` + 自定义 Waker |
| `rt/waker` | 手写 `RawWakerVTable`，`wake()` 时设置 `scheduled` 标志 |
| `rt/reactor` | poll-mode reactor：`rx_burst` → 解析 Eth/IPv4/ICMP → 匹配 session → 存 reply + wake |
| `rt/timer` | timer wheel，`Sleep::new()` 注册到期时间，`process_due()` 唤醒 |
| `dpdk/*` | DPDK FFI 安全封装：EAL、EthDev、Mempool、Mbuf |
| `net/*` | Ethernet/IPv4/ICMP/ARP 协议头解析与构建 |
| `stats/*` | TSC 时间戳 + `LatencyStats` 分位数统计 |

## runtime 提供了什么（回答 §1 核心问题）

§1 的核心问题是：**一帧 raw Ethernet 从 `rx_burst` 出来，64 个 session 的 future 在另一头被唤醒 —— 这中间 runtime 提供了什么？**

答案：把**同步的裸帧到达事件**翻译成**某个 async task 的 future 恢复执行**的整套机制。链路如下（全部手写，无 tokio / 无 epoll）：

1. **收帧 + 解析** —— [Reactor::poll_once()](dpdk-runtime/src/rt/reactor.rs) 调 `rx_burst` 拿到裸帧后，立即对**整批**记同一个 T2，再逐帧解析 Ethernet → IPv4 → ICMP；ARP 帧就地处理（应答 / 学 MAC）。
2. **按 `id` 找到 session，存 reply，唤醒** —— `handle_packet()`：ICMP echo reply 的 `id` 即 session_id，用它定位 `Session`，把 rx mbuf 所有权移入 `session.reply` 并记录 `reply_t2` / `reply_seq`，然后取出该 session 注册的 `Waker` 调 `wake()`。
3. **Waker 只置一个原子标志** —— [waker.rs](dpdk-runtime/src/rt/waker.rs)：`wake()` 把该 task 的 `scheduled` 标志置位。单核 poll 模式下不搬 future、不跨线程投递，O(1) 无锁。
4. **executor 只 poll 被调度过的 task** —— [Executor::run_ready()](dpdk-runtime/src/rt/executor.rs) 遍历 ready queue 时**跳过 `is_scheduled()==false` 的 task**（63 个睡着的 session 不被空转），命中的才 `poll()`，future 从 `wait_reply().await` 处恢复并拿到 reply。
5. **timer 支撑 `sleep().await`** —— [Sleep / Timer](dpdk-runtime/src/rt/timer.rs)：`sleep().await` 把 deadline 压进 timer 堆，`process_due()` 到期 `wake()`，task 恢复后走到下一个 `send()`（= 段③）。

主循环 [block_on](dpdk-runtime/src/rt/executor.rs) 把三者缝在一起：`poll 主 future → timer.process_due() → reactor.poll_once() → run_ready()`。这就是 **poll-mode 与 async 的融合**——没有 epoll / io_uring，网卡轮询是唯一的 I/O 事件源。

**一句话**：runtime 让每个 session 写成直白的 `loop { send().await; wait_reply().await; sleep().await }`，而不是退化成一个巨大的 `match` + callback 状态机；它为此收的税，就体现在段②里"解析 → wake → ready queue → poll → future 恢复"这条链路上（约 220 ns，p50，即下文 A − B 的差）。

## 测量口径

进程内耗时 = **段①**（发送）+ **段②**（接收），用 TSC 计时：

| 时间点 | 含义 |
|--------|------|
| T0 | 构建 echo request 之前 |
| T1 | `tx_burst` 返回之后 |
| T2 | `rx_burst` 返回之后（reactor 中记录） |
| T3 | session 从 `wait_reply().await` 恢复执行 |

- **段①** = T1 − T0（包构建 + mbuf alloc + copy + tx_burst）
- **段②** = T3 − T2（包解析 + session 匹配 + wake + executor 调度 + future poll）
- **in-process** = 段① + 段②

## 构建与运行

### 一键 setup

```bash
# 从零开始：安装依赖、编译 DPDK、配置大页、绑定网卡、编译项目
sudo DPDK_PCI_BDF=0000:28:00.0 ./scripts/setup.sh
```

### 一键 run

```bash
# 默认 delay_us=500, duration_sec=600, sessions=64, payload=64
./scripts/run.sh

# 自定义参数
DELAY_US=500 DURATION_SEC=600 SESSIONS=64 PAYLOAD=64 ./scripts/run.sh
```

### 手动构建

```bash
export PKG_CONFIG_PATH=/usr/local/lib64/pkgconfig
cargo build --release -p dpdk-runtime -p async-ping -p raw-ping
```

### 手动运行

**async-ping (A)** — 异步运行时版本，64 路并发：

```bash
sudo env LD_LIBRARY_PATH=/usr/local/lib64 \
  ./target/release/async-ping \
  --delay-us 500 --duration-sec 600 --sessions 64 --payload 64 \
  --target-ip 10.202.8.15 --gateway-mac 06:ff:fd:b6:f0:cd --local-ip 10.202.15.212
```

**raw-ping (B)** — busy-poll 手写状态机版本：

```bash
sudo env LD_LIBRARY_PATH=/usr/local/lib64 \
  ./target/release/raw-ping \
  --delay-us 500 --duration-sec 600 --sessions 64 --payload 64 \
  --target-ip 10.202.8.15 --gateway-mac 06:ff:fd:b6:f0:cd --local-ip 10.202.15.212
```

命令行参数：

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--delay-us N` | 500 | 两次发送间隔（µs），同时约束聚合速率 |
| `--duration-sec T` | 60 | 运行 T 秒后自动停止并输出统计 |
| `--sessions` | 64 | 并发 session 数 |
| `--payload` | 64 | ICMP payload 长度（字节） |
| `--target-ip` | 10.202.8.15 | 对端 IP |
| `--gateway-mac` | 06:ff:fd:b6:f0:cd | 下一跳 MAC（对端或网关） |
| `--local-ip` | 10.202.15.212 | DPDK 网卡的 IP（用于 ARP 应答） |

> `--target-ip` / `--gateway-mac` / `--local-ip` 有默认值（AWS 环境），可省略。

## 运维事项

### 环境依赖（`scripts/setup.sh` 已一键覆盖）

| 项 | 配置 |
|----|------|
| DPDK | 22.11.4，源码编译装到 `/usr/local`（`-Dlibdir=lib64`）|
| Rust | stable（rustup）|
| 大页 | 1024 × 2MB，挂载到 `/mnt/huge`（hugetlbfs）|
| 网卡驱动 | `vfio-pci`（AWS 无 IOMMU，需 `enable_unsafe_noiommu_mode=1`）|
| DPDK 网卡 | `0000:28:00.0` 绑定到 vfio-pci |

关键环境变量：编译用 `PKG_CONFIG_PATH=/usr/local/lib64/pkgconfig`，运行用 `LD_LIBRARY_PATH=/usr/local/lib64`。

### 日常操作

```bash
# 一次性：装依赖 + 编 DPDK + 配大页 + 绑网卡 + 编项目
sudo DPDK_PCI_BDF=0000:28:00.0 ./scripts/setup.sh

# 一键跑 A + B（默认 500µs / 600s / 64 session / 64B）
./scripts/run.sh
```

### 换机器 / 换网卡要动的地方

- **网卡 BDF 因机而异**，用环境变量传入：`DPDK_PCI_BDF=0000:XX:00.0`。查找：`lspci -nn | grep -i ethernet` 或 `dpdk-devbind.py --status`。
- **把网卡还给内核**（临时恢复 SSH / 系统栈）：`sudo dpdk-devbind.py --bind=ena 0000:28:00.0`。
- 对端 IP / 下一跳 MAC / 本地 IP 有默认值，可用 CLI 覆盖（见上一节）。

### 已知现象（不是故障）

- **退出时 ENA 驱动残留数十个 mbuf**（`mempool final avail_count` 略低于初值，实测 53–62）：这是驱动内部开销，不随运行时长 / 包数增长；`raw-ping` 因 drain 更彻底会精确回到初值。
- `EAL: Using IOMMU type 8 (No-IOMMU)` 是 AWS 无 IOMMU 环境的正常输出。
- **必须 `sudo` 运行**（大页 + vfio + 网卡），机器提供 passwordless sudo。

## 性能结果

测试环境：AWS c8a.xlarge（AMD EPYC 9R45，4 物理核无超线程），DPDK 22.11.4，ENA 网卡，64 路并发，10 分钟连续运行。

> 完整延迟报告与原始运行日志见 [`results/`](results/)（[`latency-report.md`](results/latency-report.md) + A/B/C 运行日志）。

### 硬门槛验证

| 指标 | async-ping (A) | raw-ping (B) |
|------|----------------|--------------|
| 发送包数 | 59,348,548 | 58,905,018 |
| 接收包数 | 59,348,548 | 58,905,016 |
| 丢包率 | 0.0000% | 0.0000% |
| 崩溃 | 无 | 无 |
| mbuf 泄漏 | 无（59 个 ENA 驱动内部残留） | 无（精确回到初值） |

### 进程内耗时对比（10 分钟）

| 指标 | async-ping (A) | raw-ping (B) | A − B |
|------|----------------|--------------|-------|
| **in-process p50** | 400 ns | 80 ns | **+320 ns** |
| in-process p90 | 530 ns | 290 ns | +240 ns |
| **in-process p99** | 1419 ns | 600 ns | **+819 ns** |
| in-process p99.9 | 2870 ns | 2470 ns | +400 ns |
| in-process p99.99 | 4010 ns | 2900 ns | +1110 ns |

### 分段耗时（p50）

| 段 | async-ping (A) | raw-ping (B) | 差值 |
|----|----------------|--------------|------|
| 段① 发送 (T1−T0) | 170 ns | 70 ns | +100 ns |
| 段② 接收 (T3−T2) | 230 ns | 10 ns | **+220 ns** |

异步抽象开销主要集中在**段②**（接收分发路径）：解析 → session 匹配 → `wake()` → ready queue → executor 出队 → `poll()` → future 恢复，约 **220 ns**（p50）。

### 端到端延迟参考

| 版本 | e2e p50 | 说明 |
|------|---------|------|
| async-ping (A) | ~152 µs | DPDK 网卡 (10.202.15.212) |
| raw-ping (B) | ~152 µs | DPDK 网卡 (10.202.15.212) |
| 系统 ping (C) | ~97 µs | 内核网卡 (10.202.15.8)，网络路径不同 |

## 关键技术点

1. **Checksum 字节序陷阱**：`ipv4_checksum()` / `icmp_checksum()` 返回主机序 `u16`，写入 `#[repr(C, packed)]` 结构体字段时在小端机器上会字节反转，必须 `.to_be()`。
2. **RFC 1624 增量 checksum**：固定 payload 下 IPv4 头逐字节常量，startup 构建一次模板包；运行时只 patch `seq` + `ident` + TSC，用 `HC' = ~(~HC + ~m + m')` 增量更新 ICMP checksum。模板中被修改的字段必须初始为 0。
3. **Executor 优化**：`run_ready()` 只轮询 `is_scheduled()` 为 true 的 task，避免每轮轮询全部 64 个 task。
4. **T2 精确测量**：reactor 在 `rx_burst` 返回后立即记录 T2 并存入 session，session 恢复执行时取该 T2，避免将 executor 调度延迟混入段②之外。
5. **poll-mode reactor**：无 epoll/io_uring，`rx_burst` 直接轮询网卡，零系统调用。
6. **timer wheel**：基于 TSC 的高精度定时器，`Sleep::new()` 注册到期 cycle，`process_due()` 唤醒。
7. **ARP 应答**：收到问自己 IP 的 ARP request 就答（约 20 行），对端内核才能回包给 DPDK 网卡。
8. **mbuf 跨 await 所有权**：session 收到 reply 后持有 rx mbuf 直到 `sleep().await` 结束，验证 runtime 对跨 await 资源所有权的处理。

## `unsafe` 边界（`grep -rn unsafe dpdk-runtime/src`）

`unsafe` 块共 66 处，全部集中在 DPDK FFI 封装层和 runtime 基础设施中，业务逻辑（协议解析、session 管理、统计）全部 safe：

| 文件 | `unsafe` 数 | 用途 |
|------|-------------|------|
| `dpdk/ethdev.rs` | 17 | `rte_eth_*` FFI 调用 |
| `dpdk/mbuf.rs` | 15 | `rte_pktmbuf_*` FFI 调用 |
| `rt/waker.rs` | 7 | `RawWakerVTable` 实现 |
| `dpdk/mempool.rs` | 6 | `rte_mempool_*` FFI 调用 |
| `rt/reactor.rs` | 5 | raw 指针解引用 + `Mbuf::from_raw` |
| `dpdk/eal.rs` | 3 | `rte_eal_init` / `rte_eal_cleanup` |
| `net/ipv4.rs` | 3 | packed struct 字段读写 |
| `net/icmp.rs` | 3 | packed struct 字段读写 |
| `net/ethernet.rs` | 2 | packed struct 字段读写 |
| `net/arp.rs` | 2 | packed struct 字段读写 |
| `stats/tsc.rs` | 2 | `_rdtsc()` 内联汇编 |
| `rt/executor.rs` | 1 | `Pin::new_unchecked` |

## 换一块网卡要改哪几行

代码本身**几乎不需要改**——DPDK EAL 会自动探测绑定到 `vfio-pci` 的网卡，`-p 0x1` 端口掩码用第一个可用端口。本地 MAC 通过 `rte_eth_macaddr_get` 从硬件读取。

需要改的只有**环境参数**（默认值在 `main.rs`，也可通过 CLI 覆盖）：

1. **绑定新网卡到 vfio-pci**（不改代码）：
   ```bash
   sudo dpdk-devbind.py --bind=vfio-pci 0000:XX:00.0
   ```
2. **对端 IP / MAC**（网络变了才需要）：
   - `async-ping/src/main.rs` 第 34-35 行：`target_ip_str`、`gateway_mac_str`
   - `raw-ping/src/main.rs` 第 34-35 行：同上
   - 或运行时传 `--target-ip` / `--gateway-mac`
3. **本地 IP**（用于 ARP 应答，需和新网卡同网段）：
   - `async-ping/src/main.rs` 第 36 行：`local_ip_str`
   - `raw-ping/src/main.rs` 第 36 行：同上
   - 或运行时传 `--local-ip`

> 网卡型号改变（如 ENA → ixgbe）**不需要改代码**，DPDK PMD 自动适配。
