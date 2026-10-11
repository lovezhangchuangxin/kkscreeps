# 私有束按可见集分组共享——视野重叠场景的共享装配（B0 中性，如实入档）

## 背景与用户裁定

初版为"全视野门槛"（查看者须见全部已注册房），B0 标尺复测发现夹具**每房
单一主人**（50 玩家 × 6 自有房 + observer 观察），无人满足全视野——快路径
不生效，首版 ABBA 中性（wall 0.971、相邻对 0.957/1.015 方向矛盾=噪声）。
一度裁定撤回；**用户纠正**：真实场景（打架互驻军、探索、联盟、观战）视野
重叠是常态，机制应保留并推广，且须构造视野重叠测试——本片据此改型。

## 机制（现行实现）

- **分组键**：查看者完整可见房集合（自有 ∪ 观察，`VisibilityIndex` 新增
  `owned_rooms` 反转表与 `visible_rooms_of`）的确定性序列化（房名升序
  `\\u{0}` 连接）。
- **世界资格**：`World::kkpv_grouping_eligible` = 共享块映射覆盖全部已注册房
  且空块全部可共享（即世界无归属旗/Mod 对象/扩展字段/归属工地——这些内容
  使块字节依赖查看者裁切）。资格不成立时逐查看者装配，行为不变。
- **共享流**（kk-runtime execution）：资格成立时，组内首个查看者逐查看者
  装配后把束字节经 `SharedKkpvBundle::from_vec` 发布为组句柄（RunJob 私有束
  新增 `KkpvPrivateBundle::Shared` 变体，工作者挂载与公开束同机制零拷贝），
  后续同组查看者直接命中共享句柄，仅构建逐玩家信封（user/gcl/market/power
  逐玩家不变）。同组束字节仅差**无人消费的 audience 改写位**——JS 解码路
  径不读该字段（kkpv.js 全文无 audience 校验）。
- **代际纪律**：组缓存按 RunEpoch 键控，**代际更替整体清空**——束字节携带
  块头 tick，跨代际复用会被解码端"块头 tick 与信封不一致"拒绝（该缺陷由
  economy_gcl 回归抓住：资格跨代际保持为真时旧实现只换 epoch 标记未清映射，
  陈旧束注入导致表达式 Null）。

## 测试（视野重叠场景，用户点名构造）

- kk-world `grouped_bundles_share_identical_bytes_for_overlapping_viewers`：
  **互驻军**（两玩家在同样两房各有对象）与**探索**（observer 补足对方房视野）
  同组；同组查看者束 audience 归零后**逐字节一致**；异组（可见集不同）束不同；
  归属旗/归属工地使世界资格失效（块依赖查看者裁切/己方工地候选）。
- embed_tests `grouped_bundle_overlap_impl`（引擎端到端）：双玩家互驻军
  （对方房各有己方 link——road 等无主结构不授予视野，夹具须用可占领结构）
  → 组缓存恰 1 条共享束（探针 `kkpv_grouped_cache_for_test`）、两玩家双房
  结构读数一致；归属旗出现后资格失效清空回退，读数仍一致。

## B0 定位（如实）

B0 夹具每玩家可见集互异（自有 6 房 + 观察 6 房，50 组不同键），分组共享
**不生效**；开销为每代际一次资格探测（共享映射覆盖检查）+ 每玩家一次
可见集键构建，微秒级。**本片对 B0 达标无贡献**——B0 的 project 相主靶
是信封逐玩家 Value 构建（主线程采样：信封占逐玩家投影 ~82%，另片处理）。
价值主张为真实负载：视野重叠/观战/联盟场景的同组玩家共享装配。

## ABBA 与棘轮

- 终版冻结 ABBA（old=5e9abba5 release sha1 4339d821…，new=本片
  1fcf6775…，2 丢弃+O,N,N,O）：**wall 0.991 / project 1.010 / CPU 0.984 /
  RSS 峰 1.038，相邻对 wall 1.000、1.019**——B0 如预期中性（不生效态的
  资格探测+键构建开销在噪声带内），逐窗 wall 311.2/311.4/309.6/315.6。
- 棘轮（ratchet.log）：2905/9/0，相对基线转红 0 条。
- 回归：kk-world 103、kk-runtime 全目标（embed 102 含新场景、mod_chain
  495）、kk-runner 全部、kk-cli sim 106 全绿；完整无参数 `pnpm run check`
  随本片提交前执行。

## 二进制差异注记

ABBA/棘轮所用二进制（1fcf6775…）与最终提交源码的差异为采集后的门禁
修正：Cmd::RunJob 装箱平抑变体尺寸差（每次派发一次堆分配，微秒级）、
doc 注释位置、tests.rs 按职责拆出 grouping_tests.rs——均无语义变化，
不影响上述读数。

## 复现

```bash
cargo test -p kk-world grouped_bundles
cargo test -p kk-runtime --test embed_tests   # grouped_bundle_overlap_impl
KKSCREEPS_BIN=<bin> node compat/scripts/ratchet.mjs
python3 /tmp/grouped-abba.py                  # 冻结对：sfb-old vs grouped-new
```
