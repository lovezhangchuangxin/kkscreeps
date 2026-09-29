# ADR-012 M2A 架构定型：沙箱运行时与玩家 API 层

- **日期**：2026-09-29（A6 收口时升格）
- **状态**：已接受（依据 [m2-playable.md](../plans/m2-playable.md) 头注的既定安排：M2 架构级结论定型时升格本 ADR）

## 背景

M2A（沙箱运行时 + 玩家 API 核心）经 A1–A6 六批落地后收口。方案期（2026-09-28）定下的
两条硬约束与六个关键决策（D1–D6）在实施中经受住了验证，本期将其中**架构级**的结论从
方案文档升格为 ADR——后续 M2B 扩面、B0 清偿、M3 多用户承载都建立在这些定型之上。
实施过程中的两项重大修正已由
[ADR-013](ADR-013-architecture-review-coordinator.md)（运行协调层 / seam 先行 / LIFO 纠偏 /
宿主面收口）独立裁决，本 ADR 不重复，只收敛其修正后的净形态。

## 决策

1. **两条硬约束是全部取舍的裁决器**：①玩家侧体验与官方一致
   （[兼容红线](../designs/compatibility.md)全文有效——官方行为即玩家体验的定义）；
   ②安全性对齐官方且玩家不可越狱（沙箱逃逸零容忍）。架构不照搬官方老架构：玩家可见层
   （game/ 语义）逐字对照官方，内部层（编码/传输/存储/调度）按性能与确定性自由选择。
2. **玩家 API 层放在 prelude JS 侧对照移植，Rust op 侧只做计算原语**（原 D2）。
   落地形态：`crates/kk-runner/src/prelude/30_game/` 按官方 game/ 文件结构组织（八文件 +
   _registry）；41 个类全局非可写安装 + 官方原型链拓扑（bot 原型补丁生态的根基）；对象是
   **持 id 的薄壳**——字段 getter 经 `rawData(id)` 回查当前 tick 投影（官方
   `defineGameObjectProperties` + `data` 闭包的同构），跨 tick 旧引用读新值、消亡抛官方同款
   错误。理由定型：官方 game/ 层本就整层跑在 isolate 内、宿主注入面极小；JS 细节语义
   （instanceof/lodash 谓词两用/`__packedPos`/Store 的 Proxy 下标）在 Rust 侧无可维持的对照
   锚点；M0 实测的层间形态漂移教训全部指向"玩家可见查询归 prelude 单源"。
3. **intent 链路官方同构四段**（原 D1）：沙箱内闭包收集器（`{list, set/push/pushByName/
   remove}`，玩家拿不到引用）→ API 方法"读本 tick 快照预检 → `intents.set` → 返回码"→
   endTick 一次性结构化带出（JSON 编码；二进制编码推迟随性能基线评估）→ 宿主侧清洗成
   IntentBatch 接结算。计费双轨（沙箱内 0.2/条即时显示、宿主按最终清单重算扣桶）；玩家
   loop 软异常时已 push 的 intents 照收、isolate 级硬中断整轮丢弃——两个口径都与官方一致。
4. **数据面分期，seam 先行**（原 D3 + ADR-013 决策 2 修正）：M2A 维持 world_json JSON 投影
   为数据源、消费面单独成层；M2B API 大扩面之前先落三件（世界注入来源接口化、地形按世界
   版本缓存、不可变房间版本句柄），新命名空间 getter 写在 seam 抽象之上；四段布局与零拷贝
   冻结仍由端到端测量决定。"每玩家每 tick 完整世界 JSON 仍是必经数据面"在 M2A 属**已登记的
   未兑现架构目标**（perf-probe 实测投影构建占比，见
   [A6 实测记录](../../compat/reports/perf-a6.md)），M2B 兑现。
5. **查询面契约投影 Rust 单源**（原 D5）：embed `getObject/findInRoom` 的 screeps-ok 契约
   形状（kind 族）在 world_json 出口统一，不引入 TS 转换层第三真相源；两层（prelude 玩家面
   / snapshot 契约面）一致性由显式对表测试锁定。
6. **验收口径定型**（原 D4/D6）：screeps-ok 通过率棘轮（地板只升不降 + 相对 HEAD~1 逐用例
   转红即红 + ratchet-allow 显式豁免同提交归因）贯穿全程；bot 三基线（tooangel/overmind
   2000t + wasm 10t）回归；2A 出口关卡 ≥40%（实际达成 48.25%，见下）。
7. **性能观测点与实测通道**（A6 交付）：EmbedEngine 内嵌四相打点（投影构建/玩家运行/结算/
   tick 墙钟，p50/p95/max），`kkscreeps perf-probe` 子命令驱动真实闭环（真玩家代码 × intent ×
   结算）出数——这是 performance-acceptance.md「阶段耗时」口径在 M2A 的落点；正式场景基线
   （场景 A/C × 3 实现 × ≥5 次独立运行）仍归 M2B/M5 基准批次。

## 实现现状（截至 A6 收口，2026-09-29）

- **A1–A5**：查询面契约投影、intent 通道、prelude 重组（30_game/）、薄壳 getter 化、
  per-viewer 装配、动作族全量（Store/spawn/资源/controller/移动杂项）、tombstone/ruin 接线、
  mineral/extractor、Room/RoomPosition/Memory/模块形态官方同形（A5 四片 +48）。
- **A6**：结构读取面真化（`Structure.prototype.isActive` 官方四门 + Chebyshev 距离计数；
  setActive 幽灵 API 摘除；place 通道 generic 结构 hits/hitsMax 缺省表；build 负 remaining
  官方算术）；tooangel 基线 300t→2000t 重跑零错；性能观测点埋设 + perf-probe + 首次小规模
  实测记录；棘轮地板 1358→1405（48.25%，2A 出口关卡 ≥40% 达成）。
- **移交**：多实例生命周期/运行协调/宿主面收口 → B0（ADR-013）；seam 三件套与投影宏化 →
  M2B 前置批；结构动作族（tower.attack 等）/寻路 FFI/RawMemory segments/市场 → 2B 批组。

## 三原则符合性

玩家体验一致（isActive/hits/build 逐字对照官方 file:line，含 ROAD-HITS 的官方 quirk 直通）/
玩家不可越狱（新执行路径全部加载期捕获 Math.abs/Math.max/Array.prototype.push/C 常量面板，
无新增可交换全局依赖）/ Rust 最优且不照搬官方（观测点是自有组件，perf-probe 为官方没有的
可重复验收工具）——逐项校验通过，详见 A6 批次三路评审记录。
