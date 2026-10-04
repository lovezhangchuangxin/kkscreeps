# 核心经济链与 GCL 输入（2026-10-04）

本批承接 D3 后的核心玩法缺口，基于 `5673ec1` 开发。此前研究中的 container 搬运断链和 PlayerSpec.gcl 被忽略已经复核；既有升级累计逻辑不需要重写。当前机制见[游戏循环](../../architecture/game-loop.md#仓库资源与-gcl)。

## 行为与依据

- Generic 仓库按 `store`、`storeCapacity`、`storeCapacityResource` 字段参与 transfer/withdraw，不新增封闭的可存储类型名单。共享仓按全部资源合计库存钳制；分资源仓按资源槽钳制，空实验室矿物余量为总容量减已分配容量。
- 实验室装入首种矿物时设置该资源容量并将通用容量置 null；取空后清空该资源容量并恢复通用容量。保留其他库存、效果和对象字段。
- 放置及建造落成的空仓补原始 `store`。放置的共享仓为 `{}`，有 energy 容量槽的固定仓为 `{energy:0}`；建造落成按官方初始化为 `{energy:0}`。连续场景发现原先缺少该字段会被玩家 transfer 预检拒绝，即使容量字段完整；两入口一同修复，新增建造后首次转入的验证。
- 提取延用安全模式与 rampart 保护，并拒绝 nuker/powerBank、尚未到截止 tick 的 terminal 干扰效果。
- GCL 显式等级/进度转累计点数；默认等级为 `max(ownedRooms+1,2)`。输入不合法时旧分片保持有效；新增累计点数仍由现有 tick 候选更新并投影。

依据为本地官方 engine 的 `src/utils.js::capacityForResource`、`src/processor/intents/creeps/{transfer,withdraw}.js`，以及 screeps-ok 固定版本 `34a630efa42908c117c19c25d2350fa2387bd633` 的 `src/adapter.ts::gclPoints` 和 `adapters/vanilla/index.ts::createShard`。GCL 的 JS 安全整数上限是本项目输入边界；账户 power 输入仍未接入。

## 扩展边界登记

按 [ADR-014](../../decisions/ADR-014-contribution-plane.md#3-开发约束)登记例外：通用容量通过对象字段开放，但实验室矿物绑定、nuker/powerBank 禁止提取、terminal 干扰判定新增的内置特殊分支，以及 GCL 场景转换仍依赖默认规则常量。原因是当前贡献计划只完整装配 link 规则，本批须先修复默认玩法的真实资源流转；D4 应将特殊存储策略和账户成长常量接入具名规则入口。本批没有宣称完成 D4。

## 验证与评审

引擎新增行为测试覆盖混合资源共享容量、同轮竞争、事件方向、分资源仓、实验室换矿、特殊提取限制、干扰到期和敌方私有 rampart。真实玩家测试分别通过 JSON 与 KKPV 执行容器提取、升级、转回、GCL 累计和默认额度认领，晚期 RawMemory 写回失败后仓库库存与单位库存保留旧值，重试只提交一次搬运和升级，非法输入保留旧分片。

两种投影通道分别运行连续 40 tick 的真实玩家脚本：miner 持续采集并存入空容器，upgrader 从容器搬运并升级控制器。每次动作检查返回 OK，40 轮 Memory 计数精确，已消耗升级能量大于 10，源剩余能量加容器和单位库存及升级消耗恰好等于初始 3000；GCL 累计等于控制器新增进度。

提交前自审按官方实现复核了容量余量、分资源槽、矿物绑定、事件方向、干扰截止时间、GCL 等级回读和旧分片保护。连续场景揭露了空仓缺少原始 store；首次统一补空对象时完整兼容性门禁发现 9 条 tower 预检回归，因为 undefined 能量绕过资源比较，最终按容量元数据补固定仓的显式 energy:0，并加入两通道的 attack/heal/repair 缺能量测试。修复后没有待处理的阻断问题。

验证命令和结果：

- `cargo build --release -p kk-cli` 与适配器 build 成功，固定上游 pin 未变；后续使用绝对 `KKSCREEPS_BIN` 指向本次 release。
- `pnpm run check` 通过：格式、workspace Clippy、623 个 Rust 测试通过、11 个忽略、兼容适配器 Node 测试 14 个通过及文档链接检查。晚期搬运与升级同时回滚的增强断言又单独通过 `cargo test -p kk-runtime --test embed_tests`。
- 完整棘轮：1918 passed / 89 failed / 907 skipped，总数 2914，通过率 65.82%，地板上调为 1918。相对 D3 的 HEAD 报告新增 31 例通过，零转红；相对脚本规定的 HEAD~1 有 41 例转绿和 2 条原有豁免，无新增跳过或豁免。机器证据见[本批 JSON](economy-gcl-2026-10-04.json)与[入库套件报告](../../../compat/reports/kkscreeps.json)。

固定世界 bot-check 不执行规则结算，本批使用实际推进世界的确定性场景验证资源链；完整真实 bot 的经济场景入口仍是后续任务。
