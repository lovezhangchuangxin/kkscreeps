# ADR-015 JS 组合处理链与 Rust 候选访问

- **日期**：2026-10-03
- **状态**：已接受并实现 link/对象意图执行片
- **前置**：[ADR-014](ADR-014-contribution-plane.md)

## 问题

默认规则在 Rust，贡献处理器与闭包在可信 JS VM。原尝试用 Rust NextAction 捕获内层链，JS next 经重入 op 执行它；但外层 invoke 持有整个 ModRealm 的可变借用，内层再派发 JS 会借用冲突。改为 try_borrow_mut 只能安全报错，不能支持嵌套。静态续行包含槽级 after，还会导致 after 提前且重复执行。

既有实验表明 op scope 可以安全回调 JS，所以“所有宿主驱动或重入都不可能”不成立。但当前功能主要是在同一 VM 中组合多个 JS 函数，为此反复跨过 Rust 边界并不带来候选所有权优势。

## 决定

保留默认 Native 路径；有 JS 贡献的槽一次进入 JS，由 JS 根据启动时安装的冻结链完成 before/wrap/center/after。Rust 引擎提供不依赖 V8 的整槽 SlotExecutor 接口。native 中心作为普通 op 的领域步骤执行，候选与原意图参数始终在 Rust。去除 NextAction、op_mod_next、invoke_wrap 与静态整槽/内层链导出。

保留拥有房间的 Working 与 RoomCtx。候选能力可在本次调用期间进入 OpState，必须在返回或错误后清除，不能跨调用/跨 tick 留存。不同 namespace 的 state 从权威候选读取；next 前 flush，next 后刷新上下文，避免外层快照覆盖内层写入。

同步以权威 JSON 树为准：独立且同类型的旧容器保留身份，共享容器只在首次遇到的兼容路径复用，其余路径拆开。只复用自有数据属性，避免通过继承键修改原型。ctx/target 的本地属性不参与候选写回，也不因同步被删除；具体遍历顺序与旧引用边界见现行执行文档。

after 在整条 wrap 成功返回后执行一次。取消中心属于成功；异常、重复 next 或内层候选写入错误属于故障。宿主记录故障，不依赖 JS 作者传播异常，Rust 处理链也遵守该规则。普通的已处理业务错误与 next/宿主故障分别对待。

## 取舍

| 方案 | 对当前项目的影响 |
|---|---|
| Rust 闭包续行并重新借 realm | 无法完成当前嵌套形态；继续补工厂不能消除借用冲突 |
| 用 op scope 回调 JS、Rust 继续驱动 | 技术可行，但执行器必须贯穿 VM 调用上下文；每层跨边界，增加候选同步与宿主适配复杂度 |
| JS 组合，Rust 执行领域步骤 | 组合留在函数所属 VM，引擎保持独立；需要显式处理 flush/刷新、能力寿命、错误留存与 native 的领域接口 |

选择第三种。它针对当前以 JS 贡献为主的扩展链；不要求把默认规则移到 JS，也不要求在每个无 Mod 对象上进入 VM。后续 native 需要生命周期 JS 时优先拆分步骤，独立回调接口仍可明确使用 reentrant，不把局部选择推广为全项目禁用重入。

## 验收与边界

真实引擎验证两层 JS wrap 与 link native、JS replace、自有 process；验证候选读写顺序、保存的字段引用、跨 namespace 状态、取消、after 时点、捕获 next 错误、Promise/thenable、能力清理与失败重试。默认配置通过 workspace、兼容性棘轮与 bot 门禁；性能另做同负载对照。

当前只接线 link 与对象域扩展意图，不表示所有规则/生命周期槽已开放。模块全局回滚、Mod 资源预算、故障停用、持久化等仍有设计与实现缺口。现行接口及限制见 [Mod 处理链执行](../architecture/mod-execution.md)。
