# settle 全局段压缩：commit 信任路径（免重复全扫）+ 旧房异步释放（现行实现）

## 机制

取证发现每拍全对象校验跑**两遍**：引擎 validate 段一遍（9.3ms）+
`commit_tick_rooms` 内 `validate_room_objects` 又一遍（19.1ms commit 段
的大头）。本片三刀：

1. **校验拆分**：`validate_room_objects` 拆为
   `validate_object_invariants`（ID/坐标/唯一/键名全扫——引擎 validate
   段同形执行）与 `validate_domain_state`（Generic 归一/storage/
   keeper·invader 记忆/效果悬空/扩展悬空——保留在提交路径）；组合入口
   供管理编辑/恢复调用方原语义不变。
2. **commit 信任路径**：`commit_tick_rooms_prevalidated`——调用方（引擎
   validate 段）已对**同一份候选**完成对象不变式全扫，提交时不再重复
   该扫；域状态校验与覆盖/地形/游标自检保留。契约：validate 与提交间
   候选不得变异（staged_body 两调用点间只做玩家侧收敛）。返回被替换
   旧房。
3. **旧房异步释放**：旧 300 房（16 万对象 dealloc）移出结算关键路径，
   后台线程释放与下一拍投影/执行窗重叠（RoomState 无 Drop 副作用且
   Send；释放速度远快于拍间隔，无积压）。

## 归因

- **分段**（KK_SETTLE_DIAG）：commit 19.1 → 10.5（同窗其他段热膨胀 40%
  下仍 −45%；凉态折算 ~7.5ms，**结构性省 ~11ms**）。
- **冻结 ABBA**（old=9446121f，凉态重跑）：**settle 相六窗 new 全部更低
  （79.6/85.5/122.5 vs 72.7/77.0/88.2），相邻对双确认，p50 比 0.720**；
  wall 相邻对受序列热化污染作废（同二进制 settle 序列内 +54%——见
  并行片同款热序形态）。CPU 中间窗 +4~5%（异步释放线程），对 xxs CPU
  比保持 0.80-0.84。
- **等价**：kk-world 106（新增 prevalidated 自检面：覆盖/地形/游标拒绝、
  旧房返回、悬空效果仍拒）+ 跨二进制 300 房终态逐字段全等 + 引擎/运行
  时全套 + 棘轮 2905/9/0 转红 0 + 完整无参数 check。

## 凉窗多窗标尺（并行片+本片 wall 兑现裁定，12 分钟冷却后 1 丢弃+3 测量）

| 窗 | kk | xxs | 比值 | CPU 比 | RSS 峰比 | kk 分相（proj/run/settle） |
|---|---|---|---|---|---|---|
| 丢弃 | 232.8 | 208.8 | 1.1147 | 0.832 | 0.642 | 42.9/105.7/**78.2** |
| m1 | 248.7 | 218.5 | 1.1383 | 0.837 | 0.631 | 46.5/111.8/81.1 |
| m2 | 254.0 | 231.1 | 1.0993 | 0.813 | 0.629 | 47.9/118.2/82.0 |
| m3 | 280.0 | 265.0 | 1.0565 | 0.800 | 0.643 | 55.5/130.3/90.4 |

**测量 3 窗中枢：1.0993（kk 254.0）**；等价签名与既往一致。

**wall 兑现裁定**：settle 相旅程 **111.6（串行）→ 95-97（并行）→ 78-82
（+本片）**，两片合计 −30ms 精确兑现；凉丢弃窗 kk 绝对 263.3（串行片
丢弃）→ **232.8**。比值未到 0.9（窗口内序列单调热化：kk+xxs 同胀、xxs
热敏更高致热窗比值反而更低——m3 1.0565，比值-热态反比关系再获印证，
中枢口径正确）。**距 0.9×（凉窗口径 ~188ms）仍差 ~45ms，run 相
（105.7 凉态）现为最大相**。

## 复现

```bash
cargo build --release -p kk-cli
KK_SETTLE_DIAG=1 ./target/release/kkscreeps compare-probe \
  --fixture /tmp/l1b-fixture.json --warmup 5 --ticks 100 --concurrency 4 \
  --report /tmp/cmt-report.json
python3 /tmp/cmt-abba.py   # 冻结对 cmt-old(9446121f) vs cmt-new
```
