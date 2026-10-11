# 信封字符串直构 + 逐代际片段缓存——project 相 −29%（现行实现）

## 机制

`World::kkpv_envelope_string_for_viewer`：逐代际缓存每房预序列化片段
（房键/名称/公共状态/事件段，`EnvelopeFragments`，generation 键控随一切
内容变更失效），逐玩家只拼接片段与逐玩家数值（time/gcl/user/
visibleRooms/能量两键）——免 Value 信封逐玩家重建（300 房 × ~5 分配）
与整树二次序列化。产物与 `to_string(kkpv_envelope_for_viewer(...))`
**逐字节一致**（键序/转义/数字格式同源复刻，字节锁回归
envelope_string_matches_value_serialization 覆盖可见/不可见、状态/无状态、
事件含病态条目、观察者、能量逐查看者、缓存复用与换代失效）。

配套：世界源 trait 分离 `kkpv_envelope_string_for_viewer` +
`kkpv_bundle_bytes_only_for_viewer`（束装配拆出信封，与合并路径同源同字
节）；运行时 market/power 逐玩家序列化后按顶层键序拼接（rooms 之后，与
Value 插入路径的 IndexMap 追加序一致）；`Value::String` 作为注入层哨兵
直用免整树序列化。直构路径内嵌分组共享（同组查看者束句柄复用，重叠视
野语义保持，组缓存探针断言仍生效）；分离接口不可用回退合并路径。

## ABBA（冻结 release，old=5e9abba5 sha1 4339d821…，new=aea12526…，
2 丢弃+O,N,N,O）

| 指标 | new/old | 判定 |
|---|---|---|
| **project P50** | **0.706（−29.4%）** | **相邻对双确认（0.748 / 1.487）**，48.4/60.7→36.2/40.8 |
| CPU core-ms/tick | 0.941 | 改善（Value churn 消失） |
| wall P50 | 0.951 | 均值向好但相邻对方向矛盾——本窗序列单调热漂（old1 255.9→old4 309.4，同二进制 21% 漂移），wall 归因作废，以 project 双对一致+标尺复核为准 |
| RSS 峰 | 1.022 | 噪声级 |

## 标尺（compare.py 同窗配对，丢弃+测量，机器清闲）

| 窗 | kk | xxs | 比值 | CPU 比 | RSS 峰比 |
|---|---|---|---|---|---|
| 丢弃 | 263.3 | 228.1 | 1.1543 | 0.755 | 0.689 |
| 测量 | **247.3** | **221.3** | **1.1177** | 0.708 | 0.650 |

旅程：1.397×（L1a 后）→ 1.155×（P2 后）→ 1.2213×（remat 后，凉窗）→
**1.1177×**；kk 绝对 401→343→266→247.3ms。分相 project 36.1 / run 97.7 /
settle 111.6（**settle 重回最大相**）。等价签名与既往一致（structures
517/518、move +0.339%），无新分歧；observed_goal RSS 峰值低于 xxs 维持。

## 验证链

kk-world 104（新增字节锁 envelope_string_matches_value_serialization）、
kk-runner 全套、kk-runtime embed 102（双通道等价套件锁定直构语义 + 分组
重叠场景探针保持）、mod_chain 495、sim 106、棘轮 2905/9/0 转红 0、完整
无参数 check（随提交执行）。

## 含义与下一片

0.9×（本窗口径 199.2ms）还差 −48ms。settle 111.6 为最大相（成本分散，
既往采样复核无单点杠杆）——**结构性相位重叠（P4 依赖调度/R03 底座）是
剩余主路径**；project 残余 36.1（片段拼接+可见集查表+装配+market/power）
与 run 97.7（V8 侧，玩家代码为主）次之。

## 复现

```bash
python3 /tmp/envstr-abba.py   # 冻结对 sfb-old vs envstr-new
python3 scripts/benchmark/compare.py --xxs-root <xxs> --node <node26> \
  --players 50 --runs 1 --warmup 5 --ticks 100 --concurrency 4 \
  --output /tmp/envstr-yard-{discard,measure}
```
