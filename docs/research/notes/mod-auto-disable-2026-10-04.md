# Mod 连续故障计数与自动停用（2026-10-04）

本片在 [显式停用与回退](mod-disable-execution-2026-10-04.md) 上增加可配置自动策略。默认关闭，保持现有失败即回滚与显式重试行为；Rust 宿主可通过 `RegisterOptions.fault_disable_threshold = NonZeroU32::new(3)` 启用三次阈值。自动策略属于 runtime，不改变冻结计划身份或直接 ModRealm 的行为。

## 当前契约

每个实际引擎结算失败至多记一次首个 Mod 故障。handler/control/context/write_conversion/write 且绑定存在、namespace 对应启用包时可归责；read/native/capability/resource/chain、缺失或未知绑定及非 Mod 引擎故障排除。不解析异常文本。这是可信共享 VM 的运营策略，SDK 来源不构成不可伪造身份，不能用于恶意包隔离。

只保留当前连续序列（来源 Mod ID、饱和 u32 次数）。同 Mod 的相邻实际失败累计，另一 Mod 从 1 开始，成功或不可归责失败清除。序列跨 VM 重建保留；重建失败、结算前测试拒绝和丢弃缓存本身既不计数也不清除。不提供累计历史次数或多个来源交替失败的停用策略。正常拒绝不记故障；整轮成功也中断序列。

达到阈值时仍向调用方返回本次失败；预算结束、候选已被引擎丢弃后停用原来源及依赖闭包，销毁 realm，明确报告新停用 ID 和次数。下一次调用重建并结算缓存玩家输出，不能把失败候选直接发布或重新执行玩家。没有在单个 API 调用内添加自动重试循环。

`mod_fault_streak()` 提供序列，`last_mod_auto_disable()` 保留最近决定与触发故障证据。决定跨重建、成功及其他故障保留；teardown/新分片清空。故障报错明确附带自动停用说明，原槽错误保留；与下一轮会清除的普通故障诊断分开。不建立无限事件队列，尚无 CLI 命令/通知、持久输入事件、执行策略指纹或进程隔离。

## 验证

真实 JSON/KKPV 用例覆盖：阈值 1/2、达到阈值前世界/Memory/CPU 不发布、失败重建与结算前拒绝不重复计数、停用依赖闭包、native 与自有 process/槽丢弃回退、缓存玩家只执行一次、费用在成功发布时保留、成功和非 Mod 失败中断序列、另一 Mod 来源不继承计数、决定跨成功/新故障留存、teardown 清空、默认关闭仍记录诊断。

普通 control/context/转换/字段校验故障分别验证。资源测试即使阈值为 1 仍走恢复门、不记可归责序列；覆盖 CPU、堆外内存与整轮累计额度。内部策略矩阵用有效绑定验证 read/native/capability/resource/chain 排除，以及缺绑定/未知绑定不能靠文本归责。宿主注册测试核对阈值不会改变 PlanHash/贡献/绑定身份。

命令、源码与二进制哈希及兼容结果见 [验证记录](mod-auto-disable-validation-2026-10-04.json)。本片不运行性能采样，前一批无 Mod/空 Mod 与尾延迟差异仍未定位。现行机制见 [Mod 执行架构](../../architecture/mod-execution.md)。
