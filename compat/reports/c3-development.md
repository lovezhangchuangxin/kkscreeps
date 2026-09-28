# C3 批次开发报告：存储选型基准骨架（kk-storage-bench）

- **批次**：M2 轨 C 批 3（[m2-playable.md](../../docs/plans/m2-playable.md) §10 轨 C）
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
| 场景夹具 | `src/fixtures.rs` | 三档规模世界（t1 单房 / t2 12 房 / t3 100 房）——房间内容经 `kk_world::place::normalize` 放置、值走 `kk_schema::room::encode_snapshot` 正式封套（与未来 kk-storage 落盘字节同源）；随机性全走 `kk_core::rng::SeededRng`；玩家记录 ~230B JSON + Memory blob（100KB 常态 + 每 5 人 1 条 1MB）|
| 基准编排 | `src/main.rs` | bin `kk-storage-bench`：场景矩阵（9 场景 × 引擎 × 档位）、读回字节严格对账、扫描条数断言、JSON + 人读摘要 |
| 夹具锁定测试 | `src/fixtures.rs` tests | 8 个单测：逐字节确定性、快照可解码+对象数形状、档位尺寸区间、blob 局部变更形态、键序 |
| 方法学文档 | `docs/research/notes/storage-bench.md` | 方法学、负载形态、初轮数字、选型判据草案（六维）、风险登记；已入 research README 索引 |
| 初轮证据 | `compat/reports/storage-bench/*.json` | 三档 macOS arm64 机器可读报告 |

版本钉在根 `Cargo.toml`：`redb = "4.3"`、`libmdbx = "0.9"`（workspace.dependencies 单点治理）。

## 依赖隔离方案与验证（工程约束清偿）

**选型：workspace 成员 + feature gate**（备选 workspace exclude 被否——会脱离
fmt/lints 门禁且需要独立 lockfile）。验证记录（本机 macOS arm64）：

- `cargo test --workspace`：全绿，**1:15 墙钟**（含本 crate 8 测试；无 redb/libmdbx 编译）；
- `cargo build --release -p kk-cli`：41.6s，构建图不含 redb/libmdbx（kk-cli 不依赖 bench crate）；
- `cargo fmt --all --check`：过（rustfmt 语法级，覆盖 feature-gated 代码）；
- `cargo clippy -p kk-storage-bench --all-targets` 四种 feature 组合（无/redb/libmdbx/both）：零警告零错误——CI 只跑默认组合，其余组合本批人工验证；
- libmdbx 降级路径：feature 不启用即完全跳过，基准二进制给出可行动错误信息；mdbx-sys（cc+bindgen）本机冷构建 13s 通过。

**已知残留摩擦**：Cargo.lock 增大（可选依赖全图锁定）；clippy 对 gated 代码
不在 CI 覆盖（本地验证，正式选型批若把引擎依赖转正则自然进 CI）。

## 基准初轮数字摘要（macOS arm64，rounds=5/warmup=1/read_ops=500）

t3（100 房 841.7KB 快照 + 100 玩家 23KB 记录 + 11.8MB blob）关键行【实测】：

| 场景 | redb durable | mdbx durable | redb relaxed | mdbx relaxed |
|---|---|---|---|---|
| rooms 检查点批量写 | 5.51 ms（153 MB/s） | 5.55 ms（152 MB/s） | 0.40 ms（2.1 GB/s） | 0.34 ms（2.4 GB/s） |
| rooms 逐房事务 p50 | 4.03 ms | 4.00 ms | 11.2 µs | 22.3 µs |
| rooms 点读 p50/p99 | 0.5/0.6 µs | 0.9/1.6 µs | 0.5/0.6 µs | 0.4/0.6 µs |
| blobs 批量写 | 30.65 ms（396 MB/s） | 25.11 ms（483 MB/s） | 2.77 ms | 4.29 ms |
| blobs 点读 p50/p99 | 0.5/0.5 µs | 2.2/56.0 µs | 0.5/0.6 µs | 2.5/28.6 µs |
| 数据文件（blobs 后） | 67.38 MB | 67.11 MB | 67.38 MB | 67.11 MB |

三条对 kk-storage 设计最有信息量的观察（详见方法学文档 §4）：

1. **durable 逐条提交两引擎都被 ~4ms APFS fsync 钉死**（235–306 op/s）——
   durable 语义下逐房提交不可行，检查点批量提交是唯一形态（比"选哪个引擎"
   更强的架构约束）；
2. durable 批量检查点两引擎打平（<5% 差异）；大 value 有区分度（mdbx 批量
   快 ~20%；redb 大值点读尾部更稳）；
3. 读路径（点读/扫描）全亚微秒～几十微秒且内存缓存命中——"内存为主 +
   定期检查点"形态下读路径不构成选型压力。

初轮过程还抓到一个基准方法学坑：同值重写被 libmdbx 优化为 no-op commit
（~30µs 假绿 durable 数字），已改为每轮 blob 头部 4KB 局部变更（更贴近真实
Memory 修改形态）并在文档登记。

## 风险与未尽事项

| 项 | 说明 | 处置 |
|---|---|---|
| 崩溃恢复语义未测 | snapshot 契约场景 4（检查点各阶段崩溃注入）是正式选型必测维度，骨架未含 | 2B 批 10 正式选型时补（判据草案已列） |
| relaxed 档语义不对齐 | redb None vs mdbx SafeNoSync 攒批狠度不同 | 文档明示只作量级参考，不作淘汰依据 |
| 单机 APFS 噪声 | t2 出现 mdbx durable p99 124ms 离群（fsync 风暴/文件增长）；无 root 清 page cache | 报中位+分位并标注；正式选型在专用基准机跑 |
| WriteMap 变体未测 | mdbx mmap 直写与 redb 缓冲 IO 的可比性 | 正式选型补一组 |
| fixture 形态逼近 ≠ 真实负载 | 房间尺寸按官方数据面参数化（250B–60KB 区间），真实分布在 2A 后校准 | 正式选型换真实负载夹具（2B 批 10） |
| Cargo.lock/根 Cargo.toml 合并摩擦 | 共享文件小冲突 | 主会话合入时手工处理（方案 §10 既定） |

## 提交

见分支 `m2/track-c` 上 "C3：" 前缀提交（本文件为末次提交的一部分）。
