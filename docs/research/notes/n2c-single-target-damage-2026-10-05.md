# N2c 首片：JS 单体伤害策略（2026-10-05）

基于 `25b427d`，开放近战/单体远程槽的 `ctx.combat.applyDamage` 及五个战斗槽的 `get`。作者可读取同房耐久、身体部件/强化，使用独立 JS 公式指定伤害；同一通用应用器维护生命值、死亡/库存掉落、结构摧毁/废墟和事件。现行契约见[JS 伤害策略](../../architecture/combat.md#单体伤害)，机器证据见[验收数据](n2c-single-target-damage-2026-10-05.json)。

## 实现与验证范围

- SDK 校验原始 ID 字符串和整数伤害；op 拒绝 NaN/Infinity/小数/越界，引擎再次校验表示域。原生 request 与策略 applyDamage 的意义分开：策略不重算部件功率，不做距离/安全模式/rampart 否决，也不追加原生反击。
- 战斗目标视图为纯数据脱离副本；执行者的 body/hitsMax 读面使用同源转换，修改读面不会写回权威候选。spawning 读面允许作者在应用前正常拒绝尚在孵化的目标。
- before/replace 应用与 native/request/cancel 共用 consumed。局部拒绝恢复额外目标的延迟队列，保留此前成功动作；摧毁恢复覆盖结构库存、废墟、空间索引、事件及 ID 游标。
- 同一次调用内的已结束处理器能力、越界和重复执行仍是不可吞的整链故障。跨调用旧 token 在访问候选之前拒绝，沿 N1b 既有契约可捕获后继续；分别加测，没有扩大已有对象能力的故障半径。
- 晚期 RawMemory 写回失败保持世界/state/Memory，使用缓存玩家结果重试，伤害与 state 恰一次。没有新增持久状态域，既有检查点保存 body/hits/spawning 和 namespace state 即可。
- 两个独立 JS 包 flat-damage（固定 45）和 scaled-damage（生命值/护甲公式）没有专用 Rust 分支。两个 replace 同时启用明确冲突；单选诊断通过。

## 验收结果

| 验证 | 结果 |
|---|---|
| Mod 处理链整组 | 139 通过；新增 10 个策略测试覆盖两通道、护甲/部件读面、远处且被 rampart 覆盖的目标、边界、拒绝、故障与缓存重试 |
| 真实 CLI | 两个新测试通过：三个场景各 JSON/KKPV，两个策略包各两通道的进程退出后冷恢复；比较完整 final 报告，排除实际 CPU 桶 |
| 完整 `pnpm run check` | Rust 864 通过、0 失败、12 个既有忽略（56 组）；JavaScript 14 通过；Clippy 无警告；格式、链接及 411 个源码文件规模通过 |
| Release 棘轮 | 2914 总用例、1918 通过、89 个既有失败、907 跳过；相对 `25b427d` 逐用例状态无变化；两项既有未命中豁免警告 |
| 真实 bot 固定桩环境 | TooAngel/Overmind/WASM 完成 2000/2000/10 轮，无终止、惩罚跳过或 missing API；关键指标与入库基线一致 |
| 干净暂存内容导出 | 只使用 Git 暂存文件导出，release 运行三个场景两通道共六组，全部通过；完整 final（排除 CPU 桶）和 PlanHash 两通道相等 |

策略场景目标/执行者最终 hits：flat 为 55/200，scaled 为 40/200，原生对照为 70/170。namespace state 记录 applied=1，冷恢复保持该状态。

Overmind 保留既有 1 个 console-error tick；将随机 harvest 名字归一后，错误栈与 `compat/reports/bot-overmind.json` 一致。本批没有将 bot-check 退出码视为零逻辑错误或完整游戏进展验收。性能没有新测量，不声称整体开销改善。

Release SHA-256：`a400f2b5574d71eb911aa9f431bf0062381ecd68d87419bd7ff5ae756fa567bb`。

## 后续范围

本批交付单体伤害计算/应用。原玩家预检策略替换、自定义治疗、群体/反击策略、移动仲裁、自有对象伤害 trait 仍须后续消费者和回滚验收；没有把它们标为完成。新世界规则包按显式内容选择启用，检查点仍核验实际包/配置/计划身份。
