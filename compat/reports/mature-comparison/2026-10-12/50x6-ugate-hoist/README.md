# commit 段转义门提升：`\u` 子扫每串一次（现行实现）

## 机制

`domain_validate_markers_present` 原以六个 `literal_gate` 串联，每个都
`contains(字面量) || contains("\\u")`——同一 data 串的 `\u` 子扫重复 6
遍（每 Generic 对象最多 12 趟子串扫）。提升为转义门每串一次 + 六字面量
顺序短路（7 趟），布尔结果逐位等价。

## 归因（冻结 ABBA，old=fc5d602c，2 丢弃+O,N,N,O）

| 段 | old（三窗） | new（三窗） | 判定 |
|---|---|---|---|
| commit | 8.07/8.45/9.35 | **7.49/7.68/7.91** | **三对全部同向，−0.6~−1.4ms** |

量级小于乐观估计：标记门里"命中后的完整 JSON 解析"才是 commit 的大头，
纯门扫只 ~1.5-2ms；彻底压缩需脏对象跟踪（未排期）。wall 本轮序列热化
（cpu 103k→120k）不作判。

## 等价

kk-world 全套、300 房终态跨二进制全等、棘轮转红 0、完整无参数 check。

## 复现

```bash
# old=fc5d602c 冻结副本 /tmp/kk-bin-current；ABBA 驱动 /tmp/ugate-abba.py
KK_SETTLE_DIAG=1 <bin> compare-probe --fixture /tmp/l1b-fixture.json \
  --warmup 5 --ticks 100 --concurrency 4 --report /tmp/ugate-abba-<tag>.json
```
