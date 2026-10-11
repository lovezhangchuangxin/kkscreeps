# validate 段压缩：唯一性集乘法散列 + Mod 检查融进不变式遍历（现行实现）

## 机制

macOS `sample` 函数级采样定位 validate 段（10.4-12.2ms 热口径）构成：
①不变式扫的跨房唯一性 `HashSet<ObjId>` 用默认 SipHash 逐键哈希 + 空集
增量扩容（rehash 反复搬迁），约占一半；②Mod 对象独立扫是第二趟全对象
判别枚举（B0 无 Mod 也要走），约占四分之一。两刀：

1. **乘法散列 + 预容量**：`IdHasher`（Fx 式 rotate-xor-multiply）+ 按
   对象总量 `with_capacity_and_hasher`——免 SipHash 与增量扩容。
2. **Mod 检查融合**：单对象检查拆为 `check_mod_object` 助手，融进不变
   式的同一趟全对象循环；独立入口 `validate_mod_object_iter` 保留给
   检查点/管理路径（两个死包装删除）。建筑房扫已有空集早退无需动。

## 归因（冻结 ABBA，old=df170906，2 丢弃+O,N,N,O）

| 段 | old（三窗） | new（三窗） | 判定 |
|---|---|---|---|
| **validate** | 10.41/12.13/12.22 | **3.01/3.19/3.20** | **−7.4~−9.0ms，全窗一致** |
| settle 相 wall | 58.0/67.2/68.2 | **53.4/59.4/60.9** | **三对全部同向，−4.6~−7.8ms** |
| wall | 热污染（run 106→151、CPU 104k→134k 单调爬升） | 同 | 本轮不作判 |

本轮序列热化严重（棘轮余温未被状态门拦住——**教训：状态门需含重活后
的沉降间歇，pmset 无节流≠无热惯性**）；validate/settle 段为窗内口径，
不受影响。

## 等价

kk-engine 全套、kk-runtime 全套（mod_chain 495 覆盖融合后的 Mod 检查）、
300 房终态跨二进制逐字段全等、棘轮转红 0、完整无参数 check（随提交）。

## 复现

```bash
cargo build --release -p kk-cli   # old=df170906 冻结副本 /tmp/kk-bin-maxid
KK_SETTLE_DIAG=1 <bin> compare-probe --fixture /tmp/l1b-fixture.json \
  --warmup 5 --ticks 100 --concurrency 4 --report /tmp/valfuse-abba-<tag>.json
# ABBA 驱动 /tmp/valfuse-abba.py；sample 函数级归因见台账 2026-10-12 记录
```
