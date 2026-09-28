# ADR-011 引擎骨架定型：typed world 为唯一世界后端 + 官方结算序子集 + 棘轮验收

- **日期**：2026-09-28
- **状态**：已接受（方案 docs/plans/m1-engine-skeleton.md 经用户确认，八步实施收官；
  本文将该方案的架构级结论升格为 ADR，实现现状同步回写架构文档）

## 背景

M1（引擎骨架）要交付 kk-core / kk-schema / 单房间结算 / 通过率棘轮，并把 M0 的哑世界
（JSON Value 存取、tick 只计数）换成有真实规则的世界。方案阶段的六项关键决策（D1~D6）
在实施中全部落地并经逐提交三方评审验证，本文将其定为架构决策，供后续里程碑（M2 真沙箱
接入、M2B 存储）作为既定事实引用。

## 决策

1. **typed world 是唯一世界后端**（方案 D1）：`kk_world::World`（BTreeMap<ObjId, _>，
   id 全局单调分配）同时服务 embed 通道（screeps-ok 适配器/sim）与引擎结算；
   M0 的 DumbWorld 已删除。世界投影 JSON（`world_json`）保持 M0 契约形状——prelude
   消费面零改动是硬约束。放置接受面 = `place::normalize` 归一化表（初态对照官方，
   spec 私有字段折算不泄入；结算范围外类型落 Generic 透传桶保持 FIND 可见性）。
2. **结算 = 官方 processor.js 阶段序的忠实子集 + 三处有意差异**（方案 D4）：对象迭代序
   用 id 升序（= 创建序，天然确定；官方为 DB 插入序）、bulk 写入改为工作副本直接变更
   （内存模型）、结算失败即整房回退（官方打日志继续）。每房间 clone→settle→写回
   （深评轮已升级为全房两阶段提交——全部克隆结算成功后统一写回，杜绝部分失败时
   同 time 重放结算，见"后果与遗留"），`World::advance_ticks` 是 player_ticks
   唯一维护点。
3. **intent 通道 = "世界视图进、intent 批出" 的 Sandbox trait**（方案 D5）：M1 只有
   ScriptedSandbox 假实现（固定脚本回放），embed 路径 intents 恒空——玩家 intent 收集
   通道是 M2-2A。intent 层（sanitize 白名单、同对象冲突压制图、幂等覆盖）按官方全图
   建表，M2 补执行器零改动。
4. **kk-schema = 声明宏生成的二进制快照格式**（方案 D2）：`kk_schema!` 宏展开出 typed
   struct + encode/decode + 编译期布局常量，M1 只实现所需子集；版本升级函数链，旧版本
   读入即升级。world → schema → core 的依赖方向合规。
5. **常量不手抄**（方案 D3）：`crates/kk-core/src/constants.rs` 由官方 constants.json
   全量 dump 生成（scripts/gen-constants.mjs），消费按需取子集。
6. **通过率棘轮 = 地板文件 + CI job**（方案 D6）：`compat/reports/ratchet.json` 是唯一
   真相源（floor 只升不降，行为变化收尾随全量重跑上调并同步文档）；CI 第四 job 全量跑
   screeps-ok 套件（vitest 退出码不作判定——预期失败池数千；报告 mtime 新鲜度守卫防
   陈旧假绿；红时与上一入库基线逐用例 diff 列转红名单）。

## 与既有 ADR 的关系

- ADR-008 第 5/6/7/8 条（Sandbox trait 假实现、地形官方算法推后、存储推后、嵌入式 API）
  的 M1 落地形态由本文细化；嵌入式 API 的方法集与地位不变（ADR-010）。
- ADR-004（房间快照 + 索引 + 分桶）：typed world 的 `RoomIndex` 与 BTreeMap 对象表是其
  实现层；M2B 存储落盘时快照格式走 kk-schema。

## 验收证据

- 基线 237 → 268（步 5 embed 切真实结算 +28 全转绿零转红；步 6 仪表批次 +3；逐用例
  归因见 compat/reports/m0-code-review.md 追记）；
- bot：tooangel 房间管线完整执行（M1 期实跑 300 tick；2000 tick 证据系 M0 期报告，
  下里程碑重跑）、overmind 推进到 RoomIntel 全量执行（2000 tick 零中断、缺失 API 清零），
  bot-check 退出码机器可判定（0/1/2）；wasm-bot 10 tick 零中断；
- 门禁：fmt/clippy 0 警告、cargo test 全绿（LIFO 零违规）、pnpm run check 全套绿。

## 后果与遗留

- 每步提交前三方并行 subagent 评审（官方语义对照/契约投影保真/Rust 正确性与并发）成为
  M1 起的常设纪律；
- 灰区按 compatibility.md 灰区表持续登记（步 3/4/5 各一批：迭代序/玩家序/失败回退/
  投影形态等）；
- **M1 深度评审轮（收官后，2026-09-28）**：5 路 subagent 并行深评修复 4 high + 13 medium + 3 low
  ——其中两处架构级修正回写本文：决策 2 的"每房间 clone→settle→写回"已升级为
  **两阶段提交**（全房克隆结算成功后统一写回，杜绝后房失败时先房按同 time 重放结算）；
  决策 4 的"版本升级函数链"由承诺变为 `UPGRADE_CHAIN` 挂载点占位（v1 起步空链）；
  详见台账追记；
- M2 输入已登记：intent 收集通道与真沙箱、独立 runExpr 通道计费口径、tombstone 变体、
  多实例 LIFO、旧对象引用失效语义。
