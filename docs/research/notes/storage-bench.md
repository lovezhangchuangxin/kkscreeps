# 存储选型基准：方法学、负载形态与本机数字（M2-C3 骨架）

> **状态：基准骨架 + 本机数字（初轮 + 评审清偿后复跑）。非正式选型**——
> 正式选型待 2A 落地后用
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
   按默认 feature 编译本 crate——**零外部存储依赖**，实测全量测试温缓存
   ~40s（含本 crate 8 个单测：fixtures 5 + engine/harness 3；t3 逐字节双
   构建锁 +~8s 墙钟），对 CI 时长无感；
2. `cargo fmt --all` 覆盖所有代码（rustfmt 按语法工作，不受 feature 影响），
   格式门禁不漏；
3. 版本钉在根 `Cargo.toml [workspace.dependencies]`（治理单点），Cargo.lock
   锁定全图（含未启用的可选依赖——版本漂移在 lockfile diff 可见）；
4. 对比 workspace exclude（spikes/ 路线）：那会脱离 fmt/lints 门禁并需要独立
   lockfile，与"spikes 是证据冻结的已完成使命工程"语义冲突。

**扩展 CI 时的纪律**：若未来把引擎 feature 纳入 CI，用显式
`--features redb,libmdbx`（或子集）——**禁 `--all-features`**：workspace 级
`--all-features` 会把 cc/bindgen 拖进的 mdbx C++ 构建带进每次 CI。

**降级路径**：libmdbx 的 C++ 构建在任一平台失败时，不启用该 feature 即可
——workspace 其余构建完全不受影响，基准二进制在无引擎 feature 时打印可用
引擎清单并以非零码退出（可行动的错误信息）。本机（macOS arm64 + Xcode CLT）
mdbx-sys 冷构建约 13s 通过【实测】。

**复现**（在仓库根）：

```text
cargo run -p kk-storage-bench --release --features redb,libmdbx -- \
  --tier t3 --rounds 5 --json target/storage-bench/report-t3.json
```

证据 JSON 随报告入库：`compat/reports/storage-bench/`（2026-09-29 复跑
覆盖初轮文件——复跑动机见 §3 的 warmup 同值污染修复）。

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
| users_checkpoint_bulk_write | 检查点重写全部玩家小记录（~280B JSON） | 每轮毫秒 |
| users_point_read | 按玩家名点读 | 每读微秒 |
| blobs_bulk_write | 一个事务写全部玩家 Memory blob | 每轮毫秒 |
| blobs_per_key_txn_write | 每 blob 独立事务（逐玩家提交） | 每对微秒 |
| blobs_point_read | 大 value 点读（百 KB～MB） | 每读微秒 |

读路径正确性自验：点读按抽样键对账读回**字节总数**（合计口径，非逐字节；
缺键在引擎内即报错），扫描**每轮**断言命中条数——基准数字不会来自错误结果。

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

固定 seed 夹具（确定性见 §3）→ 每场景 warmup 1 轮（不计时，**数据集与
计时轮不相交**——warmup 轮号从 `rounds` 起排，避免 warmup 与计时首轮同值
重写被引擎判为无脏页 no-op commit，见 §3 的 blob 教训）+ 计时 5 轮 →
每轮聚合值取中位、每操作样本合并出 p50/p95/p99（就近取秩分位）；操作吞吐
（op/s）**按 p50 派生而非 mean**——逐条提交的样本带 fsync 风暴重尾，mean
会被单次离群拉偏。机器可读 JSON + 人读摘要双输出。默认参数
`--rounds 5 --warmup 1 --read-ops 500`。

对齐 [performance-acceptance.md](../../architecture/performance-acceptance.md)
"预热后采集、独立多轮、报中位与分位、不只挑最好一次"的口径；微基准规模下
把"预热 500 tick"等比缩成 warmup 轮。**已知局限**：本机无 root 无法清
page cache，读写都偏乐观；APFS 的 fsync 行为与 Linux ext4 不同，durable 档
跨平台不可比；单机单次运行有噪声（见 §4 的两次运行漂移单元格标注）。

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
  新落地房 = rcl1 + spawn + 2 creep。对象数 **429 / 98 / 14**（mature/mid/
  young；mature = controller 1 + source 2 + spawn 3 + extension 60 +
  storage 1 + 路 150 + 墙 180 + creep 20 + 旗 3 + 工地 2 + 掉落资源 5 +
  透传桶 2——早版文档的 428/55/8 漏计了路/旗/工地/透传等非结构对象）；
- 随机性全走 `kk_core::rng::SeededRng`（结算同款 PCG32），任意档位逐字节
  可复现（单测锁定，含 t3 双构建逐字节比对）；
- 玩家记录 ~280B 小 JSON；Memory blob 每 5 名玩家 1 条 1MB（至多 2 条）、
  其余 100KB——百 KB 常态 + MB 级尾部。

**blob 局部变更（初轮实测教训）**：每轮重写同一份 blob 字节时，libmdbx 把
同值提交优化成 ~30µs 的 no-op commit（无脏页即不落盘），durable 档出现假绿
数字【实测】。改为每轮重写 blob 头部 4KB（轮次相关字节）——既贴合"玩家每
tick 只改 Memory 一小部分"的真实形态，也消除 no-op 假象。评审清偿追补的
同源修复：warmup 轮号从 `rounds` 起排、与计时轮数据集不相交——修复前
warmup 与计时首轮同值，首轮计时样本同样被 no-op 污染（初轮 JSON 里 durable
场景 min_ms 低至 0.03–1.7ms 的签名即由此而来，复跑后消失）。

## 4. 数字（macOS arm64 本机，2026-09-29 复跑——评审清偿后口径）

环境：Apple Silicon（aarch64）、macOS 15、APFS；参数 rounds=5 / warmup=1 /
read_ops=500，warmup 数据集与计时轮不相交（§2.3）、操作吞吐按 p50 派生；
两引擎跑同一目录树上各自的独立数据目录。完整数字见
`compat/reports/storage-bench/*.json`（机器可读，含每场景 p95/min/max 与
文件体积；本次复跑覆盖初轮 JSON——复跑动机是 warmup 同值污染修复，见 §3）。

t3（100 房/100 玩家）关键数字【实测】：

| 场景 | redb durable | mdbx durable | redb relaxed | mdbx relaxed |
|---|---|---|---|---|
| rooms 检查点批量写 | 5.19 ms/轮（162 MB/s） | 4.85 ms/轮（173 MB/s） | 0.45 ms（1.9 GB/s） | 0.57 ms（1.5 GB/s） |
| rooms 逐房事务 p50 | 4.00 ms | 3.09 ms | 11.0 µs | 24.2 µs |
| rooms 点读 p50 / p99 | 0.7 µs / 0.8 µs | 0.5 µs / 0.8 µs | 0.5 µs / 0.5 µs | 0.4 µs / 0.7 µs |
| rooms 全表扫描 | 0.006 ms | 0.031 ms | 0.004 ms | 0.021 ms |
| users 检查点批量写 | 3.80 ms | 3.73 ms | 0.042 ms | 0.052 ms |
| blobs 批量写（12.1MB/轮，头部 4KB 变更） | 16.85 ms（720 MB/s） | 9.54 ms（1271 MB/s） | 3.56 ms（3.4 GB/s） | 4.15 ms（2.9 GB/s） |
| blobs 逐条事务 p50 | 3.99 ms | 3.91 ms | 22.0 µs | 29.4 µs |
| blobs 点读 p50 / p99 | 0.7 µs / 0.8 µs | 2.8 µs / 52.4 µs | 0.5 µs / 0.5 µs | 2.1 µs / 24.7 µs |
| 数据文件体积（blobs 写后） | 67.38 MB | 67.11 MB | 67.38 MB | 67.11 MB |

（t1/t2 数字量级一致，见 JSON。两次运行的已知漂移单元格【实测+推断】：

- **t2 的 mdbx durable 逐房事务**：初轮该场景**整体抬升**——中位约其他档
  1.8×（7.2ms）、p95 40.9ms、redb 同轮干净；本次复跑该单元格干净（p50
  4.0ms、p99 4.4ms，与其他档一致）。单次运行无法归因，正式选型须复跑该
  单元格；
- **blobs durable 批量写绝对值**：初轮 30.6/25.1ms → 复跑 16.8/9.5ms，
  两引擎同向整体漂移——单机 page cache/盘态噪声，方向性结论（mdbx 占优）
  不变；正式选型在专用基准机多轮取样。）

### 观察（不是结论）

1. **fsync 主导 durable 逐条提交**：两引擎逐房/逐 blob 的 durable 提交都被
   ~4ms 的 APFS fsync 钉死（p50 口径 t3 250–324 op/s，三档全量 249–324——
   几乎全部落在 ~250 op/s 的地板上，仅 t3 mdbx 逐房 324 略穿），引擎差异被
   IO 抹平。**任何 durable 语义下逐房提交都不可行，检查点批量提交是唯一
   形态**——这对 kk-storage 的写路径设计是比"选哪个引擎"更强的约束【推断】；
2. **durable 批量检查点两引擎接近打平**（t3 房间 5.19 vs 4.85 ms，差异
   ~7%——单次运行噪声量级内）：小值全量重写场景 fsync 计时占大头，引擎内
   算不上区分度【实测】；
3. **大 value 有区分度**：blobs 批量写 durable mdbx 占优（初轮快 ~20%、
   复跑 9.54 vs 16.85 ms 快 ~43%——绝对值跨运行漂移，方向一致），且 blob
   点读形态分化（redb p50 0.7µs / p99 0.8µs vs mdbx 2.8µs / p99 52.4µs）
   ——redb 大值读延迟更稳，mdbx 的 p99 尾部（页面间接层）值得在真实负载
   下复核【实测】；
4. **relaxed 档**：redb 逐条提交更快（11 vs 24µs）、批量也更快（3.56 vs
   4.15ms）；但两边 relaxed 语义不对齐（§2.2），只作量级参考【实测】；
5. **文件体积**：两者最终体积几乎相同（67.4 vs 67.1 MB）；mdbx 按几何
   预分配（数据未到 67MB 时文件也已是 67MB），redb 随数据增长。小数据集下
   mdbx 常驻 1.05MB vs redb 0.23MB 起【实测】；
6. **点读/扫描全部亚微秒到几十微秒级、内存缓存命中**：对"每 2 分钟一次
   检查点 + 常驻内存"的 kk-storage 形态，**读路径不构成选型压力**，写路径
   与崩溃语义才是主战场【推断】。

## 5. 选型判据草案（正式选型时的评分面）

| 维度 | 口径 | 初轮印象（非结论） |
|---|---|---|
| 写吞吐 / 检查点停顿 | durable 批量检查点的 MB/s 与毫秒；真实负载（场景 C）复测 | 小值场景接近打平（~7%，噪声量级）；大 value mdbx 批量写占优（快 20–43%，绝对值跨运行漂移） |
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
  正式选型时补一组；
- **删除/压缩面未测**：`StorageEngine` trait 无 delete/compact 操作——
  kk-storage 的对象消亡（creep 死亡、工地完成、资源捡拾）与空间回收
  （墓碑/压缩停顿）未进基准；正式选型须补删除混合负载与压缩行为一组；
- **混合读写并发未测**：基准是单线程单写者形态（与"检查点写 + 常驻内存
  读"的 kk-storage 架构可辩护地对应），但写事务进行中的并发读（MVCC 快照
  排队/写读互斥代价）未测——登记为正式选型补测项；
- **增量检查点未测**：storage.md 宣称主写路径"只写变化部分"，而基准的
  rooms/users 场景只测全量重写（blobs 头部 4KB 局部变更部分弥补了该形态）
  ——真实检查点的脏房间占比/脏字节比例待 2A 后校准后再测。
