# 散页块目录（scattered pages）证伪——解码散访代价压倒装配节省

## 动机与实现（完整实现后实测证伪，补丁存档）

按用户方向实施（"不要因为活多放弃合适方案"）：每代际拼装一份全房
full+empty 块目录（u32 房数 + 每房 16 字节偏移槽 + 全部块，世界房序），
任意可见集的查看者共用同一共享句柄，沙箱端按信封 visibleRooms 逐房选块
（`decodeCatalogResident`，与逐查看者束共用块游走核心，块序完全同构）。
实现全绿：目录布局/资格证伪单测、三玩家混合可见集引擎端到端（含**双形
态等价锁**——同世界目录形态与逐查看者装配读数逐玩家一致）、全量回归。
完整补丁见 implementation.patch（1336 行，已回退）。

## 实测（B0 夹具 50×6，短窗两轮复现 + KKDIAG）

| 指标 | 基线（HEAD） | 目录形态 | 判定 |
|---|---|---|---|
| project P50 | ~62 | 66.7 / 65.4 | **无收益**（装配本就 ~6ms/拍总量） |
| run P50 | ~120 | 220.9 / 232.0 | **+100ms 净回退** |
| wall P50 | ~310 | 443.4 / 451.3 | 净回退 ~+135ms |
| KKDIAG priv/玩家 | ~2.0ms | **6.0ms** | 解码段 3× 劣化（选块正确：rows/dirty/remat 与基线一致） |

机器全程清闲（仅 WindowServer ~23%），两轮读数稳定复现。

## 机理（为什么输）

1. **装配不是大头**：B0 每玩家仅见自有 6 房（6×534 对象=3204≈rows 3210
   ——该数字两次误导归因，本次实锤为"每人 6 个 534 对象房"），逐玩家
   束装配 ≈ 6 个大块 memcpy + 294 个空块 ≈ **~0.1ms/玩家、~6ms/拍总量**。
2. **解码散访是实打实的新增成本**：目录缓冲 ~50MB（全部房 full 块），
   每玩家在其中的 6 个大块 + 294 空块**散布访问**（cache 局部性丢失），
   每拍新建 backing store 的**缺页**，逐槽房名 utf8 读取——合计
   **+4ms/玩家**（×50=+200 core-ms）。
3. 胜负条件"装配节省 > 解码散访"在 B0 形态（6/300 可见）下不可能成立；
   即使全互见形态，装配 memcpy（~1MB≈0.1ms）仍远低于散访代价——
   **该方向被支配，非夹具特例**。

## 对 B0 主线的含义（采样归因的最终修正）

project 相 62ms 的构成：**信封构建 ~50ms（82%，逐玩家 300 房 Value 构建
+ 事件 from_str）** + 装配 ~6ms + market/power 等 ~5ms。散页证伪后，
投影侧唯一有分量的杠杆是**信封逐代际条目模板缓存/字符串直构**——
已登记为下一片。

## 复现

```bash
git apply implementation.patch && cargo build --release -p kk-cli
KK_RUN_DIAG=1 target/release/kkscreeps compare-probe --fixture /tmp/l1b-fixture.json \
  --warmup 5 --ticks 100 --concurrency 4 --report /tmp/catalog.json
```
