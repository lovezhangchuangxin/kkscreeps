# remat 字段级复用（P1 后续）——ABBA 与 KKDIAG 证据

- 日期：2026-10-11；切片：脏行内未变字段的物化值跨拍保留（FLAG_FIELD_DIRTY=4）。
- 旧基线：`/tmp/kkscreeps-remat-old`（sha1 11ec15c9…，与 P2 首刀提交 2d5a872a 的 release 构建逐字节一致，经 stash 重建比对确认）。
- 新基线：`/tmp/kkscreeps-remat-new`（sha1 707a99ea…，工作区 remat 实现）。与最终提交源码的唯一差异是采集后按 Clippy 建议把 `rows % 8 != 0` 改写为 `!rows.is_multiple_of(8)`（dirty.rs，同义改写无语义变化）及测试文件格式修正，不影响上述读数。
- 协议：同款 50×6 成熟房夹具 /tmp/l1b-fixture.json，warmup=5、ticks=100、concurrency=4，每窗独立进程；2 个丢弃窗预热后按 O,N,N,O 采集。
- 负载注记：采集时段系统后台服务（mediaanalysisd、mds_stores）持续 ~75% 单核占用；ABBA 为同机交错比值口径，相邻对方向一致性（下）作为主要稳健性依据。

## ABBA 结果（abba-summary.json，new/old 均值比）

| 指标 | 比值 | 判定 |
| --- | --- | --- |
| wall P50 | **0.944** | −5.6%，相邻对 0.943 / old4 比 new3 慢 5.8%，方向一致 |
| run P50 | **0.861** | −13.9%，remat 主受益相 |
| settle P50 | 1.005 | 持平（decode 侧改动，符合定位） |
| CPU core-ms/tick | 0.929 | −7.1%，公平门通过 |
| RSS peak | 1.044 | +4.4%（约 +140MB）记录在案；对 xxs RSS 比值门（当前 0.609×）余量充足，来源未单独归因 |

逐窗 wall P50（ms）：old1 335.3 / new2 316.2 / new3 321.2 / old4 339.7。

## KKDIAG 组成对照（kkdiag-median-{old,new}.json，末拍 105、50 玩家中位）

| 分量 | old | new |
| --- | --- | --- |
| rows / dirty / objs | 3210 / 120 / 3210 | 3210 / 120 / 3210 |
| **remat** | **768** | **60**（−92%） |
| priv | 1.5 | 2.0（字段位图查表开销，亚毫秒级） |
| refresh / bwrap / brooms / btail / rawid | 1.0 / 1.0 / 0 / 0 / 0 | 同左（P2 成果保持） |

remat 从 768 降至 60：脏行（120）不再全量重物化变长字段，仅重物化真正变化的字段（中位 0.5 个/行）。

## 兼容性

ratchet-recheck.log：passed 2905 / failed 9 / skipped 0（floor 2905），相对基线转红 0 条——与 P2 提交态完全一致，束格式新增 FLAG_FIELD_DIRTY 位未引起任何玩家可见差异。

## 复现

```bash
KK_RUN_DIAG=1 <bin> compare-probe --fixture /tmp/l1b-fixture.json --warmup 5 \
  --ticks 100 --concurrency 4 --report /tmp/remat-diag-X.json
python3 /tmp/remat-diag-extract.py /tmp/remat-diag-X.json <detail> <median>
python3 /tmp/remat-abba.py   # 旧/新路径指向本文两个冻结二进制
```
