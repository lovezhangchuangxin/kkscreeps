# C3 批次开发报告：存储选型基准骨架（kk-storage-bench）

- **批次**：M2 轨 C 批 3（[m2-playable.md](../../docs/plans/archive/m2-playable.md) §10 轨 C）
- **日期**：2026-09-29
- **分支**：`m2/track-c`（基于 main=74c16d5；不合入、不 push，合入门禁归主会话）
- **定位重申**：本批只交付**基准骨架与场景夹具备料**。非正式选型——正式选型
  待 2A 落地后以真实负载跑（ADR-008 第 7 条）。

## 实现清单

| 交付 | 位置 | 说明 |
|---|---|---|
| 基准 crate | `crates/kk-storage-bench/` | workspace 成员；redb/libmdbx 全部走非默认 feature（`default = []`）——CI 的 workspace 构建零存储依赖 |
| 统一操作面 | `src/engine.rs` | `StorageEngine` trait（批量写/逐键事务写/点读/前缀扫描/flush/文件体积）+ `DurabilityProfile` 映射表 + 前缀上界工具（带单测） |
| redb 实现 | `src/redb_engine.rs` | feature `redb`；字节表 ×3（rooms/users/blobs），durability 按 Immediate/None |
| libmdbx 实现 | `src/mdbx_engine.rs` | feature `libmdbx`；目录形态（mdbx.dat/mdbx.lck）、NoWriteMap、SyncMode Durable/SafeNoSync、几何上限 4GiB、命名表 open 时一次建好 |
| 计时与统计 | `src/harness.rs` | 就近取秩分位 p50/p95/p99、跨轮中位；机器可读 JSON 报告模型（serde） |
| 场景夹具 | `src/fixtures.rs` | 三档规模世界（t1 单房 / t2 12 房 / t3 100 房）——房间内容经 `kk_world::place::normalize` 放置、值走 `kk_schema::room::encode_snapshot` 正式封套（与未来 kk-storage 落盘字节同源）；随机性全走 `kk_core::rng::SeededRng`；玩家记录 ~280B JSON + Memory blob（100KB 常态 + 每 5 人 1 条 1MB）|
| 基准编排 | `src/main.rs` | bin `kk-storage-bench`：场景矩阵（10 场景 × 引擎 × 档位）、点读按字节总数对账（合计口径，非逐字节）、扫描每轮条数断言、JSON + 人读摘要 |
| 夹具锁定测试 | `src/fixtures.rs` tests | 8 个单测（fixtures 5 + engine/harness 3）：fixtures——逐字节确定性（含 t3 双构建）、快照可解码+对象数形状、档位尺寸区间、玩家记录/blob 尺寸形态、键序；engine——前缀上界；harness——分位统计两组 |
| 方法学文档 | `docs/research/notes/storage-bench.md` | 方法学、负载形态、初轮数字、选型判据草案（六维）、风险登记；已入 research README 索引 |
| 初轮证据 | `compat/reports/storage-bench/*.json` | 三档 macOS arm64 机器可读报告 |

版本钉在根 `Cargo.toml`：`redb = "4.3"`、`libmdbx = "0.9"`（workspace.dependencies 单点治理）。

## 依赖隔离方案与验证（工程约束清偿）

**选型：workspace 成员 + feature gate**（备选 workspace exclude 被否——会脱离
fmt/lints 门禁且需要独立 lockfile）。验证记录（本机 macOS arm64）：

- `cargo test --workspace`：全绿，清偿后温缓存 ~40s 墙钟（含本 crate 8 个
  单测 fixtures 5 + engine/harness 3，t3 双构建确定性锁 +~8s；初轮冷缓存
  1:15；无 redb/libmdbx 编译）；
- `cargo build --release -p kk-cli`：41.6s，构建图不含 redb/libmdbx（kk-cli 不依赖 bench crate）；
- `cargo fmt --all --check`：过（rustfmt 语法级，覆盖 feature-gated 代码）；
- `cargo clippy -p kk-storage-bench --all-targets` 四种 feature 组合（无/redb/libmdbx/both）：零警告零错误——CI 只跑默认组合，其余组合本批人工验证；
- libmdbx 降级路径：feature 不启用即完全跳过，基准二进制给出可行动错误信息；mdbx-sys（cc+bindgen）本机冷构建 13s 通过。

**已知残留摩擦**：Cargo.lock 增大（可选依赖全图锁定）；clippy 对 gated 代码
不在 CI 覆盖（本地验证，正式选型批若把引擎依赖转正则自然进 CI）。

## 基准数字摘要（macOS arm64，rounds=5/warmup=1/read_ops=500）

t3（100 房 841.7KB 快照 + 100 玩家 27.8KB 记录 + 12.1MB blob）关键行
【实测，2026-09-29 复跑；评审清偿后口径：warmup 数据集与计时轮不相交、
op/s 按 p50 派生——初轮 JSON 已被覆盖，方法学见 storage-bench.md §2/§3】：

| 场景 | redb durable | mdbx durable | redb relaxed | mdbx relaxed |
|---|---|---|---|---|
| rooms 检查点批量写 | 5.19 ms（162 MB/s） | 4.85 ms（173 MB/s） | 0.45 ms（1.9 GB/s） | 0.57 ms（1.5 GB/s） |
| rooms 逐房事务 p50 | 4.00 ms | 3.09 ms | 11.0 µs | 24.2 µs |
| rooms 点读 p50/p99 | 0.7/0.8 µs | 0.5/0.8 µs | 0.5/0.5 µs | 0.4/0.7 µs |
| blobs 批量写 | 16.85 ms（720 MB/s） | 9.54 ms（1271 MB/s） | 3.56 ms（3.4 GB/s） | 4.15 ms（2.9 GB/s） |
| blobs 点读 p50/p99 | 0.7/0.8 µs | 2.8/52.4 µs | 0.5/0.5 µs | 2.1/24.7 µs |
| 数据文件（blobs 后） | 67.38 MB | 67.11 MB | 67.38 MB | 67.11 MB |

三条对 kk-storage 设计最有信息量的观察（详见方法学文档 §4）：

1. **durable 逐条提交两引擎都被 ~4ms APFS fsync 钉死**（p50 口径 t3
   250–324 op/s、三档全量 249–324——几乎全钉在 ~250 op/s 地板上）——
   durable 语义下逐房提交不可行，检查点批量提交是唯一形态（比"选哪个引擎"
   更强的架构约束）；
2. durable 批量检查点两引擎接近打平（t3 房间 5.19 vs 4.85 ms，~7%——单次
   运行噪声量级）；大 value 有区分度（mdbx 批量写 durable 占优：初轮快
   ~20%、复跑 9.54 vs 16.85 ms 快 ~43%，绝对值跨运行漂移但方向一致；
   redb 大值点读尾部更稳）；
3. 读路径（点读/扫描）全亚微秒～几十微秒且内存缓存命中——"内存为主 +
   定期检查点"形态下读路径不构成选型压力。

初轮过程还抓到一个基准方法学坑：同值重写被 libmdbx 优化为 no-op commit
（~30µs 假绿 durable 数字），已改为每轮 blob 头部 4KB 局部变更（更贴近真实
Memory 修改形态）并在文档登记。评审清偿追补同源修复：warmup 轮号从
`rounds` 起排、与计时轮数据集不相交——修复前 warmup 与计时首轮同值，初轮
JSON 的 durable 场景 min_ms 低至 0.03–1.7ms 的 no-op 签名即由此而来，复跑
后全部回到 fsync 地板（≥2.8ms）。

## 评审清偿（三路评审，2026-09-29）

- warmup 与计时轮数据集不相交（main.rs）；op/s 由 mean 改 p50 派生
  （mean 被 fsync 重尾拉偏）；
- 复跑三档基准覆盖 `compat/reports/storage-bench/*.json`：durable min_ms
  去 no-op 污染（旧 0.03–1.7ms → 新 ≥2.8ms），中位量级稳定；t2 的 mdbx
  durable 逐房单元格初轮整体抬升（中位 1.8×/p95 40.9ms）、复跑干净
  （p50 4.0ms/p99 4.4ms）——单次运行无法归因，正式选型须复跑该单元格；
- 扫描条数断言改每轮校验；点读对账措辞改"字节总数（合计口径）"；
  deterministic_bytes 补 t3 逐字节双构建锁（+~16s debug 单测/套件墙钟 +~8s）；
- 方法学文档：对象数 429/98/14（附构成拆分）、玩家记录 ~280B、blob
  12.1MB、10 场景、单测归属 fixtures 5 + engine/harness 3、fsync 地板
  op/s 区间改 p50 口径改述、§6 风险登记补删除/压缩面、混合读写并发、
  增量检查点三行、§1 补"扩展 CI 禁 --all-features"纪律。

## 风险与未尽事项

| 项 | 说明 | 处置 |
|---|---|---|
| 崩溃恢复语义未测 | snapshot 契约场景 4（检查点各阶段崩溃注入）是正式选型必测维度，骨架未含 | 2B 批 10 正式选型时补（判据草案已列） |
| relaxed 档语义不对齐 | redb None vs mdbx SafeNoSync 攒批狠度不同 | 文档明示只作量级参考，不作淘汰依据 |
| 单机 APFS 噪声 | t2 的 mdbx durable 逐房单元格初轮**整体抬升**（中位约其他档 1.8×、p95 40.9ms、redb 同轮干净），复跑干净（p50 4.0ms/p99 4.4ms）；blobs durable 绝对值两次运行同向漂移；无 root 清 page cache | 单次运行无法归因，正式选型须复跑该单元格并在专用基准机多轮取样；报中位+分位并标注 |
| WriteMap 变体未测 | mdbx mmap 直写与 redb 缓冲 IO 的可比性 | 正式选型补一组 |
| fixture 形态逼近 ≠ 真实负载 | 房间尺寸按官方数据面参数化（250B–60KB 区间），真实分布在 2A 后校准 | 正式选型换真实负载夹具（2B 批 10） |
| Cargo.lock/根 Cargo.toml 合并摩擦 | 共享文件小冲突 | 主会话合入时手工处理（方案 §10 既定） |

## 提交

见分支 `m2/track-c` 上 "C3：" 前缀提交（本文件为末次提交的一部分）。
