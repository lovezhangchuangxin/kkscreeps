# settle 相分段取证（KK_SETTLE_DIAG）与 R09 房间并行可行性裁定

## 动机

信封直构后标尺 1.1177×（kk 247.3 vs xxs 221.3），距 0.9× 差 −48ms；
settle 111.6ms 为最大相且既往采样成本分散。本片给 settle 装上分段
诊断（`KK_SETTLE_DIAG=1`，轻量 Instant 差分 + 共享候选原位计数），
用 B0 短窗取子相位构成与跨房耦合计数，裁定 R09 房间并行结算的可行
性与收益预算。

## B0 窗口读数（release，5+100，并发 4，106 拍中位）

| 分段 | p50 ms | 占比 | 并行性 |
|---|---|---|---|
| settle（规则执行） | 48.83 | 39% | 逐房独立 |
| commit（提交+旧房 drop） | 19.12 | 15% | 全局（逐槽替换） |
| clone（候选整树克隆） | 11.38 | 9% | 逐房独立 |
| validate（全对象扫） | 9.32 | 7% | 全局 |
| hook（预编码快照克隆） | 8.96 | 7% | 逐房独立 |
| prepare（玩家克隆+安全模式全扫） | 5.55 | 4% | 全局 |
| nukes/market/power | 14.34 | 11% | 全局 |
| interroom | 4.64 | 4% | 后置应用 |
| writeback/into_prepared | 0.50 | <1% | — |
| **合计** | **122.65** | | ≈ settle p50 126.2 ✓（交叉核对一致） |

## 耦合计数（每拍均值，300 房）

| 耦合项 | B0 频次 | 顺序敏感？ |
|---|---|---|
| id_allocs / id_restores | **0** | 是，但零触发 |
| claim_calls（GCL 门槛读候选） | **0** | 是，但零触发 |
| interroom_moves / portal_moves | **0** | 否（后置收集应用） |
| gcl_adds | 220.75 | 否（`+=` 交换律） |
| power_adds | 5.66 | 否（`+=` 交换律） |
| events | 1310.38 | 房序拼接，逐房键控（引擎外无消费者） |
| mod_states（房相写入） | 0（代码定性） | — |

## 代码定性（分类表要点）

- **随机**：生产路径 `MineralRng(room_name, obj_id, tick)` 房间派生，天然
  独立；`Injected` 注入序列仅诊断路径顺序敏感（保留串行回退）。
- **GCL/power**：`add_gcl`/`add_power` 为 i64 交换律累加，逐房增量并行
  归并即可。
- **claim_state**：controller 升级门读候选 `(rooms.len, gcl)`，同拍他房
  add_gcl 后 claim 的窗语义敏感；B0 零触发，通用路径需串行窗或预pass。
- **ID 分配**：单调 u64 游标，分配顺序决定对象 ID；B0 零触发，通用路径
  需按房预分区（块内单调、块间留洞不回退，确定性保持）。
- **fatal_error**：首错保留，并行时按房序归并首错即可。
- **跨房移动/portal**：本就是全部房间结算后的后置收集应用，与逐房并行
  正交。
- **mod namespace state**：房相零写入（grep 全库无调用点）。

## 裁定

**R09-lite 房间并行结算可行**：B0 稳态顺序敏感耦合为零，房间循环
（clone+settle+hook=69.2ms，55%）可四路并行至 ~17-20ms，**预算省
~50ms——恰好覆盖至 0.9× 的差距**（全局串行段 53.5ms 保留：commit
19.1/validate 9.3/prepare 5.6/nukes·market·power 14.3/interroom 4.6）。
通用语义需两处确定性构造：ID 预分区 + claim 串行窗（或意图预检串行
化）。附加发现：hook 9ms 是预编码快照的第二次整树克隆（并行后隐藏，
远期 R03 Arc 房可消除）；prepare 内安全模式持有人扫描为全对象扫
（160k 对象，可改索引）；validate 全量扫与 commit 19ms 各自成后续
小片。

## 复现

```bash
cargo build --release -p kk-cli
KK_SETTLE_DIAG=1 ./target/release/kkscreeps compare-probe \
  --fixture /tmp/l1b-fixture.json --warmup 5 --ticks 100 --concurrency 4 \
  --report /tmp/settle-diag-b0.json
# 报告 settle_diag 段：segments_p50_ms + counters_total
```

本窗为结构取证（分段比例+计数），非 wall 归因基准；分段和 122.65 与
settle 相 126.2 的差 ~3.5ms 为房循环脚手架（Plane 构造/迭代）。
