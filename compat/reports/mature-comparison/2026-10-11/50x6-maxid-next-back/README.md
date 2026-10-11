# max_obj_id 末键修复：O(全对象) 换代重建 → 每房 O(log n)（现行实现）

## 机制

`World::max_obj_id` 的 max_id_cache 按换代惰性重建，旧实现平铺扫描全部
房间全部对象键（`flat_map(keys()).max()`，B0 ~9-16 万键的 BTreeMap 全树
指针追逐）。BTreeMap 最大键即末键：`keys().next_back()` 每房 O(log n)，
300 房合计微秒级。ObjId 为单一 u64 派生 Ord，末键即最大 ID，结果逐位
一致。

## 取证（validate 段内部构成，env 门控跳过探针，不入库）

- 四趟子扫（不变式/Mod/建筑/扩展）合计仅 ~1.5ms——"validate 9.3 可索
  引化"的旧归因不成立。
- 真凶部分定位：`next_available_obj_id` → `max_obj_id` 全树重建 ~2ms，
  **由 prepare 段支付**（prepare 先触达、validate 命中温缓存）。
- validate 段剩余 ~7ms（凉口径）构成仍未解释（各可见部分合计 <3ms），
  需更深剖检——留队列。

## 归因（冻结 ABBA，old=f0036d82 二进制，2 丢弃+O,N,N,O；本轮序列热污染重，wall 不可判）

| 段 | old（三窗） | new（三窗） | 判定 |
|---|---|---|---|
| **prepare** | 5.01/7.13/5.30 | **3.51/4.43/3.56** | **三对全部同向，−1.5~−2.7ms（整拍净省 ~2ms）** |
| validate | 11.09/15.58/12.39 | 11.99/15.45/13.12 | 不动（旧本就命中温缓存） |
| wall | 220-325 摆动 | 同幅摆动 | 本轮不作判（热污染） |

## 等价

kk-world 106（含 max 缓存增量维护面）、kk-engine 全套、300 房终态跨二
进制逐字段全等、棘轮转红 0。

## 复现

```bash
cargo build --release -p kk-cli   # old=f0036d82 冻结副本 /tmp/kk-bin-new-clonepipe
KK_SETTLE_DIAG=1 <bin> compare-probe --fixture /tmp/l1b-fixture.json \
  --warmup 5 --ticks 100 --concurrency 4 --report /tmp/maxid-abba-<tag>.json
# ABBA 驱动 /tmp/maxid-abba.py；validate 子扫探针见台账 2026-10-11 记录
```
