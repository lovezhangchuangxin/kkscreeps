# D6 link 动作正常拒绝（2026-10-05）

本批基于 `5178540`，承接 D4 容量规则后的内建多对象拒绝边界。现行契约见[Mod 执行](../../architecture/mod-execution.md#正常动作拒绝)，此前仅自有动作的拒绝机制见[原始记录](mod-intent-decline-2026-10-04.md)。

## 本批变化

允许 `builtin:link.transfer` 的 JS before/wrap/replace/after 返回 `api.decline(code)`。引擎在 Hosted 整链开始前保存至多两份源/目标对象原始记录及事件长度，拒绝后恢复对象并截去本次新增事件；自转移去重。runtime 将所有参与 Mod namespace 的 state 暂存到正常完成，拒绝时丢弃。开始前的候选含此前成功动作，故共享接收方已有入账、事件和 namespace 状态均保留。

没有新增处理槽或资源写能力；当前 JS 仍只能修改源 link 的 cooldown，native 只改源/目标库存、冷却和新增事件，不分配 ID、不改位置、玩家、市场或工作队列。无扩展的 Native 路径不保存对象快照、不进入 JS。扩大写面前必须重新审计局部边界；不能将当前实现当通用多对象事务。

正常拒绝继续其他动作和每轮 tick，发布玩家 Memory/CPU 候选并保留执行费用，不重建健康 realm。JS 全局变量不回滚，持久玩法状态须用 ctx.state。拒绝不追改玩家 API 已返回的 OK，也不新增玩家事件。异常与候选访问故障仍优先，整轮失败并重建 realm；link tick、其他内建资源动作和纯 Rust callback 的正常拒绝不属于本片。

## 验证范围

新增 5 个真实 runtime 测试，均进入 `mod_chain_tests`：

- before、wrap 进入前、replace、native 后 wrap、after 拒绝，撤销已执行的 before/wrap/after 写入、native 库存/冷却/事件和两个 namespace；剩余 after 不执行。
- 三个传输共享接近满仓的目标，中间拒绝：第一笔扣 25/入 24，第三笔按候选余量扣 16/入 15，目标最终 799；中间资源、冷却与事件无残留，两个 namespace 只累计两次成功动作。Mod 全局调用次数为 3，证明健康 realm 未被重建。
- 外层捕获拒绝后又抛异常，或先捕获非法库存字段写入再拒绝，都使整轮候选失败，此前成功动作也不发布。
- 晚期 RawMemory 写回注入失败，世界和完整 PlayerState、Mod state 保持上一提交；重建后缓存重试提交同一轮，玩家代码只运行一次，成功与拒绝动作均不重复入账。
- wrap 不调用 next 仍属 Completed，字段与 state 正常提交且 after 执行，保持原取消语义。

普通传输使用真实玩家 API，分别覆盖 JSON/KKPV。玩家 API 禁止 link 自转移，额外的自指恢复用 `settle_external` 构造可信意图覆盖 native 分支，不能把它解释为玩家可合法调用自转移。现有 link tick 拒绝负面测试也扩至两通道。

## 完整门禁与复审

最终 release 经 `cargo build --release -p kk-cli` 重建，使用仓库脚本准备固定的 screeps-ok 版本 `34a630efa42908c117c19c25d2350fa2387bd633`，适配器显式 build。完整门禁使用绝对路径选择本次二进制：

```bash
KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check
KKSCREEPS_BIN="$PWD/target/release/kkscreeps" node compat/scripts/ratchet.mjs
```

workspace check 通过格式、Clippy、641 个 Rust 测试（11 个忽略）、JavaScript workspace 测试（兼容适配器 14 个通过）和文档链接检查。Mod 链测试 41 个全部通过，包括本批新增 5 个测试。完整棘轮保持 1918 passed / 89 failed / 907 skipped，总数 2914、地板 1918；与当前 HEAD 和脚本规定的 HEAD~1 都零逐用例状态变化，无新增跳过或豁免。原有两个未命中转红的豁免警告保持不变。

汇总与 release 指纹见[机器记录](d6-link-decline-2026-10-05.json)，完整结果见[套件报告](../../../compat/reports/kkscreeps.json)。没有新增正式性能测量；Hosted 链保存对象记录、暂存 namespace 会增加成本，不能据 Native 路径不保存快照声称整体零开销。

提交前复核 native 的全部写面、权威 ID/参数、宿主字段门禁、同目标竞争、self 去重、事件截断、namespace 暂存、异常优先级、每轮 tick 继续执行及晚期失败重试。新增拒绝资格复用现有具名 link.transfer 槽，不新增封闭玩法分派。测试夹具曾触发玩家的超容量和自转移预检，修正为可接受的普通转入与显式可信自指输入，避免把未进入结算误当作回滚验证。最终完整门禁通过，没有剩余阻断项。
