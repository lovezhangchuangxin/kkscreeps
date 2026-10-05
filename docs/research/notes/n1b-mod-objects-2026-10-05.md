# N1b：同房 Mod 对象操作交付（2026-10-05）

基于 3ff2471，公开自有对象动作/tick 的 get/list/create/update/remove，接通同房 Mod 候选、字段默认值/校验与位置索引。nursery 独立 JS 原型自主创建火花、更新年龄并到期删除，player.js 与真实玩家脚本检查类、FIND/LOOK 和动态 ID。宿主没有按官方包 ID 专门实现玩法；当前 root 同时加载 clockwork/nursery，不是内容预设。

现行 API 及限制见[对象能力](../../architecture/mod-objects.md)，机器汇总见[JSON](n1b-mod-objects-2026-10-05.json)。完整 pnpm run check 通过：741 个 Rust 测试通过、11 个忽略；适配器 14 项通过。文档与 1000 行源码门禁通过。

## 评审修复与验收面

正常拒绝扩大到整条自有链的对象字段首触、插入/移除、索引格首触与 ID 游标，namespace state 仍成功后移交。复审修复同格对象顺序回退；新生、被删对象的本轮资格与失效引用均有定向验证。索引日志共用于既有 Hosted 战斗/建造链，其默认 Native 直调不记录日志。

sim 的原生 world 诊断投影不含 Mod 对象，新增 initial/final.mod_objects 管理列表，包含自主创建且没有初始 alias 的对象。测试比较原生世界、Mod 对象全集、namespace state 和玩家 Memory，避免只比较原生投影漏掉动态对象。

定向测试：runtime 对象操作 9 项、engine 日志 1 项、CLI sim 全部 9 项。覆盖创建默认值、分页、detach、同房/类型/绑定权限、自删、动作出生同轮/tick 出生下轮、后续已删跳过、捕获错误及 getter 嵌套、日志上限、正常拒绝恢复此前成功动作、双房故障和停用后缓存重试、晚期 segments 写回故障、ID 重试一致、停用保留对象/state、双投影玩家 API。既有 tick/资源/移动/战斗/建造与全部工作区测试由完整门禁验证。

## 持续场景

2003 轮的双房双玩家场景请求每房周期生成一个火花，最终应 born=802、expired=800，两个存活火花 age=2，两个发射器 emitted=401。每名玩家运行 2003 次，最后读取的火花 age=1（玩家读取结算前状态），没有玩家指令推动这套玩法。

[JSON 原始报告](n1b-mod-objects-json-2026-10-05.json)和[KKPV 原始报告](n1b-mod-objects-kkpv-2026-10-05.json)各完成 2003 轮，4 条断言通过、玩家错误为零，完整 Mod 对象、state、玩家 Memory 和计划身份一致。报告记录场景/模块/包配置/CLI SHA-256、实际 PlanHash、错误和已提交状态。run_wall_ms 只作诊断，没有受控 native/JS 或 xxscreeps 性能对照。

## 兼容性门禁

重新构建 release CLI，复用已准备的固定上游 34a630efa42908c117c19c25d2350fa2387bd633，显式重建适配器后运行 ratchet：2914 项中 1918 通过、89 失败、907 跳过，达到 floor 1918；相对父提交及本批次起点逐项状态变化均为零，未新增豁免。原有两条未命中豁免警告保持，本批次不调整基线或放松门槛。报告见[当前兼容性报告](../../../compat/reports/kkscreeps.json)。

## 未完成范围

当前能力只读 Mod 对象、写当前绑定 namespace 的自有类型、作用于同房自有槽。原生对象通用写面、跨房访问、生产库存/建造 trait、preTick/房间/世界生命周期、内容预设、检查点/迁移/持久回放仍待后续工作。共享可信 VM 与整分片故障半径保持；对象条数门槛不是 Rust 堆的硬上限。

下一片 N1c 公开 handler 依赖并补组合诊断，随后读取冻结有效规则及显式内容选择。历史 N1a 报告保留其原包清单和实现限制，不追改为当前能力。
