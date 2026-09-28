# 存储选型基准：方法学、负载形态与初轮数字（M2-C3 骨架）

> **状态：基准骨架 + 初轮本机数字。非正式选型**——正式选型待 2A 落地后用
> 真实负载（真玩家代码 × intent 通道 × API 面完整度）重跑后定
> （[ADR-008](../../decisions/ADR-008-runtime-model.md) 第 7 条；
> [m2 方案](../../plans/m2-playable.md) D4：存储基准被 2A 阻塞，C3 只备方法学与负载）。
> 结论按调研报告惯例标注置信级别：**【实测】**（本文命令可复现）、
> **【文档】**（依赖官方文档）、**【推断】**。

## 1. 工程形态与依赖隔离

基准 crate：`crates/kk-storage-bench`（workspace 成员）。候选引擎依赖全部走
**非默认 feature**（`default = []`）：

- `--features redb` → redb 4.3（纯 Rust，MIT OR Apache-2.0）；
- `--features libmdbx` → libmdbx 0.9 绑定（mdbx-sys 14.4 携 libmdbx C++ 源码
  经 cc + bindgen 构建；libmdbx 本体 OpenLDAP 许可；rust 绑定 Apache-2.0）。

**隔离方案选型：workspace 成员 + feature gate（而非 workspace exclude 独立
crate）**，理由：

1. CI 的 `cargo test --workspace` / `cargo clippy --workspace --all-targets`
   按默认 feature 编译本 crate——**零外部存储依赖**，实测全量测试 1:15
   （含本 crate 8 个夹具测试），对 CI 时长无感；
2. `cargo fmt --all` 覆盖所有代码（rustfmt 按语法工作，不受 feature 影响），
   格式门禁不漏；
3. 版本钉在根 `Cargo.toml [workspace.dependencies]`（治理单点），Cargo.lock
   锁定全图（含未启用的可选依赖——版本漂移在 lockfile diff 可见）；
4. 对比 workspace exclude（spikes/ 路线）：那会脱离 fmt/lints 门禁并需要独立
   lockfile，与"spikes 是证据冻结的已完成使命工程"语义冲突。

**降级路径**：libmdbx 的 C++ 构建在任一平台失败时，不启用该 feature 即可
——workspace 其余构建完全不受影响，基准二进制在无引擎 feature 时打印可用
引擎清单并以非零码退出（可行动的错误信息）。本机（macOS arm64 + Xcode CLT）
mdbx-sys 冷构建约 13s 通过【实测】。

**复现**（在仓库根）：

```text
cargo run -p kk-storage-bench --release --features redb,libmdbx -- \
  --tier t3 --rounds 5 --json target/storage-bench/report-t3.json
```

初轮证据 JSON 随报告入库：`compat/reports/storage-bench/`。

## 2. 方法学

### 2.1 访问模式（对齐 kk-storage 的未来形态）

[storage.md](../../architecture/storage.md) 定的形态是"按房间名分键、按数据
类型分表、定期检查点批量写"。基准场景矩阵：

| 场景 | 形态 | 计时口径 |
|---|---|---|
| rooms_checkpoint_bulk_write | 一次检查点 = 一个大事务全量重写房间快照（值同长不同内容） | 每轮（开事务→commit）毫秒 |
| rooms_per_room_txn_write | 逐房独立事务（对照批量提交的写放大） | 每对提交的微秒 |
| rooms_point_read | 按房间名点读单房（kk-storage 读路径） | 每读微秒；**每键独立只读事务**（两引擎同口径） |
| rooms_scan_all / prefix | 全表/前缀顺序扫描（启动恢复、按象限加载） | 每轮毫秒 + 条数断言 |
| users_checkpoint_bulk_write | 检查点重写全部玩家小记录（~230B JSON） | 每轮毫秒 |
| users_point_read | 按玩家名点读 | 每读微秒 |
| blobs_bulk_write | 一个事务写全部玩家 Memory blob | 每轮毫秒 |
| blobs_per_key_txn_write | 每 blob 独立事务（逐玩家提交） | 每对微秒 |
| blobs_point_read | 大 value 点读（百 KB～MB） | 每读微秒 |

读路径正确性自验：点读按抽样键严格对账读回字节数（缺键即报错），扫描断言
命中条数——基准数字不会来自错误结果。

### 2.2 持久化档位映射（重要：语义不完全对齐）

| 档位 | redb | libmdbx | 备注 |
|---|---|---|---|
| durable | `Durability::Immediate`（commit 返回即落盘） | `SyncMode::Durable`（commit 即 fsync） | 语义基本对齐 |
| relaxed | `Durability::None`（不主动写盘，**下一次** Immediate commit 才冲刷） | `SyncMode::SafeNoSync`（异步冲刷、保库完整性、可能丢最近提交） | **语义不对齐**：redb None 攒得更狠，跨引擎只作量级参考 |

redb 4.x 的公共 Durability 只剩 None/Immediate（Eventual 已移除【文档】），
两引擎的"relaxed"不可能完全对齐——这个差异本身进入选型判据（§5）。
基准末尾统一补一次显式落盘（redb 空 Immediate 事务 / mdbx `sync()`）再测
文件体积，保证体积数字可比。

### 2.3 跑法

固定 seed 夹具（确定性见 §3）→ 每场景 warmup 1 轮（不计时）+ 计时 5 轮 →
每轮聚合值取中位、每操作样本合并出 p50/p95/p99（就近取秩分位）。机器可读
JSON + 人读摘要双输出。默认参数 `--rounds 5 --warmup 1 --read-ops 500`。

对齐 [performance-acceptance.md](../../architecture/performance-acceptance.md)
"预热后采集、独立多轮、报中位与分位、不只挑最好一次"的口径；微基准规模下
把"预热 500 tick"等比缩成 warmup 轮。**已知局限**：本机无 root 无法清
page cache，读写都偏乐观；APFS 的 fsync 行为与 Linux ext4 不同，durable 档
跨平台不可比；单机单次运行有噪声（见 §4 的离群点标注）。

## 3. 负载形态（场景夹具）

三档规模（对齐 performance 场景表：t1 ≈ 场景 B 单房热点、t3 ≈ 场景 A/C 的
100 房）：

| 档 | 房间 | 玩家 | 房间构成（成熟度轮转） | 快照总量（实测） |
|---|---|---|---|---|
| t1 | 1 | 4 | 1 满配房 | 16.6KB（单房 16.6KB） |
| t2 | 12 | 20 | 满配/中期/新落地各 4 | 99.9KB（min 3.0KB / 中位 5.4KB / max 16.6KB） |
| t3 | 100 | 100 | 34/33/33 | 841.7KB（同上分布） |

夹具复用面（不重复造轮子）：

- 房间内容经 `kk_world::place::normalize` 放置（embed 通道同一归一化入口），
  再走 `RoomState::encode_snapshot` + `kk_schema::room::encode_snapshot`
  （magic+version 正式封套）——**基准读写的字节与未来 kk-storage 落盘的房间
  快照同源**，且随 schema 演化自动跟进；
- 成熟度三档对照官方数据面：满配房 = rcl8 + 3 spawn + 60 extension + storage
  + 150 路 + 180 墙 + 20 creep（半数 50 部件满身）+ mineral/tombstone 透传；
  新落地房 = rcl1 + spawn + 2 creep。对象数 428 / 55 / 8（mature/mid/young）；
- 随机性全走 `kk_core::rng::SeededRng`（结算同款 PCG32），任意档位逐字节
  可复现（单测锁定）；
- 玩家记录 ~230B 小 JSON；Memory blob 每 5 名玩家 1 条 1MB（至多 2 条）、
  其余 100KB——百 KB 常态 + MB 级尾部。

**blob 局部变更（初轮实测教训）**：每轮重写同一份 blob 字节时，libmdbx 把
同值提交优化成 ~30µs 的 no-op commit（无脏页即不落盘），durable 档出现假绿
数字【实测】。改为每轮重写 blob 头部 4KB（轮次相关字节）——既贴合"玩家每
tick 只改 Memory 一小部分"的真实形态，也消除 no-op 假象。

## 4. 初轮数字（macOS arm64 本机，2026-09-29）

环境：Apple Silicon（aarch64）、macOS 15、APFS；参数 rounds=5 / warmup=1 /
read_ops=500；两引擎跑同一目录树上各自的独立数据目录。完整数字见
`compat/reports/storage-bench/*.json`（机器可读，含每场景 p95/min/max 与
文件体积）。

t3（100 房/100 玩家）关键数字【实测】：

| 场景 | redb durable | mdbx durable | redb relaxed | mdbx relaxed |
|---|---|---|---|---|
| rooms 检查点批量写 | 5.51 ms/轮（153 MB/s） | 5.55 ms/轮（152 MB/s） | 0.40 ms（2.1 GB/s） | 0.34 ms（2.4 GB/s） |
| rooms 逐房事务 p50 | 4.03 ms | 4.00 ms | 11.2 µs | 22.3 µs |
| rooms 点读 p50 / p99 | 0.5 µs / 0.6 µs | 0.9 µs / 1.6 µs | 0.5 µs / 0.6 µs | 0.4 µs / 0.6 µs |
| rooms 全表扫描 | <0.05 ms | 0.04 ms | <0.03 ms | 0.02 ms |
| users 检查点批量写 | 3.79 ms | 4.70 ms | 0.04 ms | 0.04 ms |
| blobs 批量写（11.8MB/轮，头部 4KB 变更） | 30.65 ms（396 MB/s） | 25.11 ms（483 MB/s） | 2.77 ms（4.4 GB/s） | 4.29 ms（2.8 GB/s） |
| blobs 逐条事务 p50 | 4.03 ms | 3.99 ms | 27.7 µs | 30.0 µs |
| blobs 点读 p50 / p99 | 0.5 µs / 0.5 µs | 2.2 µs / 56.0 µs | 0.5 µs / 0.6 µs | 2.5 µs / 28.6 µs |
| 数据文件体积（blobs 写后） | 67.38 MB | 67.11 MB | 67.38 MB | 67.11 MB |

（t3 之外的档位数字量级一致，见 JSON：t2 的 mdbx durable 逐房事务 p99 出现
124 ms 离群——APFS fsync 风暴/文件增长事件，属单机噪声，中位 7.2ms 仍与其他
档一致【实测+推断】。）

### 初轮观察（不是结论）

1. **fsync 主导 durable 逐条提交**：两引擎逐房/逐 blob 的 durable 提交都被
   ~4ms 的 APFS fsync 钉死（235–306 op/s），引擎差异被 IO 抹平。**任何
   durable 语义下逐房提交都不可行，检查点批量提交是唯一形态**——这对
   kk-storage 的写路径设计是比"选哪个引擎"更强的约束【推断】；
2. **durable 批量检查点两引擎打平**（t3 房间 5.51 vs 5.55 ms；差异 <5%）：
   小值全量重写场景 fsync 计时占大头，引擎内算不上区分度【实测】；
3. **大 value 有区分度**：blobs 批量写 mdbx 快 ~20%（483 vs 396 MB/s，
   durable），且 blob 点读形态分化（redb p50 0.5µs / p99 0.5µs vs mdbx
   2.2µs / p99 56µs）——redb 大值读延迟更稳，mdbx 的 p99 尾部（页面间接层）
   值得在真实负载下复核【实测】；
4. **relaxed 档**：redb 逐条提交更快（11 vs 22µs）、mdbx 批量略快；但两边
   relaxed 语义不对齐（§2.2），只作量级参考【实测】；
5. **文件体积**：两者最终体积几乎相同（67.4 vs 67.1 MB）；mdbx 按几何
   预分配（数据未到 67MB 时文件也已是 67MB），redb 随数据增长。小数据集下
   mdbx 常驻 1.05MB vs redb 0.23MB 起【实测】；
6. **点读/扫描全部亚微秒到几十微秒级、内存缓存命中**：对"每 2 分钟一次
   检查点 + 常驻内存"的 kk-storage 形态，**读路径不构成选型压力**，写路径
   与崩溃语义才是主战场【推断】。

## 5. 选型判据草案（正式选型时的评分面）

| 维度 | 口径 | 初轮印象（非结论） |
|---|---|---|
| 写吞吐 / 检查点停顿 | durable 批量检查点的 MB/s 与毫秒；真实负载（场景 C）复测 | 打平，小值场景无区分度；大 value mdbx 快 ~20% |
| 读延迟分位 | 点读 p50/p99（大 value 的尾部） | redb 大值读尾部更稳 |
| 文件体积 | 最终体积 + 常驻体积（预分配差异） | 最终体积相当；小库常驻 mdbx 偏大 |
| 崩溃恢复语义 | 崩溃注入测试（检查点各写入阶段，snapshot 契约场景 4）；丢提交窗口 vs 库损坏边界 | **未测**——mdbx SafeNoSync 明确保完整性、redb None 需后续 Immediate 补刷，语义面 mdbx 表述更清晰【文档】 |
| 依赖健康度 | 维护活跃度/许可/供应链 | redb 纯 Rust、活跃（2026-09 仍发版）；libmdbx 本体活跃、rust 绑定 0.9（2026-09 发版）但生态面窄、带 C++ 供应链【文档】 |
| 构建复杂度 | CI/跨平台/工具链 | redb 零原生依赖；mdbx 需 cc+bindgen（本机 13s 过，CI 未接入——按 C3 隔离方案 CI 不碰它） |

正式选型流程（2B 批 10）：2A 落地后把本骨架的夹具换成真实负载形态（真实
房间快照尺寸分布 + 真实 Memory blob 变更率 + 真实检查点频率），补崩溃注入
维度，出对比报告 → 新 ADR。

## 6. 风险与局限登记

- 单机 APFS + 无 root 清缓存：绝对值偏乐观，durable 档与 Linux ext4 不可比；
- relaxed 档两引擎语义不对齐（§2.2），不能作淘汰依据；
- 夹具是"形态逼近"不是"真实负载"：房间快照尺寸分布按官方数据面参数化
  （xxscreeps 公布的 250B–60KB 区间），真实分布在 2A 后校准；
- blob 4KB 头部变更假设 Memory 修改的局部性；若真实玩家 Memory 每 tick
  全量重写（如 memhack 类 bot），大 value 写差距会被放大——真实负载复测项；
- mdbx WriteMap（mmap 直写）变体未测——与 redb 缓冲 IO 的可比性问题，
  正式选型时补一组。
