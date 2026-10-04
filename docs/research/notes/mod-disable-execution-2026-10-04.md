# Mod 显式停用与回退（2026-10-04）

本片提供 Rust 宿主调用边界上的显式停用机制，解决可信 Mod 的确定性结算故障在重建后不断复现的问题。自动阈值与故障计数留给下一片，不据当前来源记录直接自动归责。

## 机制与取舍

停用集合存在 ModVm 生命周期容器中，新增停用先销毁 realm，再从原包快照重建。完整注册、冻结及 PlanHash/绑定清单核验仍执行；只筛选结算链。manifest 传递依赖者整体停用，handler 显式依赖也提升到 Mod 级。JS 注册 SDK 当前不导出 handler depends_on，已有 manifest 依赖是运行期的实际入口。

before/wrap/after 移除并保持其他钩子顺序；replace 被停用后从原槽声明找回中心，不从冻结 dispatch 推测。builtin 原始中心重新取得 native 能力；启用的自有槽回退原 process。自有槽所属 Mod 已停用时整个槽丢弃，不运行挂在该槽的健康钩子。未知槽仍报错，不能因 namespace 停用而放宽清单验证。

不删除对象/schema/namespace，不回退有效规则，不重装玩家 API；玩家仍可发出停用动作，动作丢弃而 Memory/CPU 候选正常提交。没有新增逐意图费用或拒绝事件。分片内单调停用，重新启用须重建分片。原故障诊断保留到下一次实际结算，重复停用不重建健康 realm。

注册函数仍执行，因此此机制不处理注册异常、注册超限或非确定性计划漂移，也不隔离恶意包。只有结算派发受掩码约束，诊断 dispatch 和普通包间 JS 调用不受约束。掩码目前是内存 Rust API，尚无 CLI 协议、执行策略指纹或持久化输入事件；同 PlanHash 只说明目录/有效规则一致。

## 验证

专项覆盖 JSON/KKPV：失败候选未发布、停用故障 replace 后 native 回退、健康 wrap 执行、缓存玩家不重跑及 Memory 提交、重建失败保留故障和停用集合、重复/未知停用、四类钩子移除、传递依赖停用、对象/state/schema 保留、自有 process 回退与已停用绑定能力拒绝。宿主测试另核对 PlanHash/可执行绑定全集不变、inactive 槽零候选访问、未知槽拒绝、profile 关闭/开启下 native 回退能力寿命。

最终命令、测试与兼容结果见 [验证记录](mod-disable-execution-validation-2026-10-04.json)。本片未运行性能采样，不能关闭前一片登记的无 Mod/空 Mod 与尾延迟差异，也不提供新的性能通过结论。当前机制以 [Mod 执行架构](../../architecture/mod-execution.md) 为准。
