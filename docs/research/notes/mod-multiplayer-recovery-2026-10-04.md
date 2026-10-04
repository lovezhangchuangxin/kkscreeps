# 双玩家与 Mod 的恢复闭环（2026-10-04）

本片在 `765b942` 之后补充组合验收；生产实现没有变更。此前自动停用与 Mod VM 恢复已有单玩家测试，双房间玩家提交已有独立测试；这里验证它们与真实 player.js 类方法、双通道投影组合时仍遵守统一发布和缓存重试纪律。

## 场景与测试矩阵

[集成测试](../../../crates/kk-runtime/tests/mod_multiplayer_tests.rs)在两个 RCL 6 房间各放置一个 Mod counter、一对 link、对方的 scout；scout 授予跨房视野。两名玩家使用同一份 player.js 安装的 Counter 类、getter、increment 方法与 FIND/LOOK 常量，发出自己 counter 的动作、另一玩家 counter 的未授权动作以及 builtin link 转移；同时产生 Memory、segment、CPU、通知与 console 候选。

gameplay 包保留健康的自有动作中心和 builtin wrap，累计成功动作计数及模块缓存调用数；faulty 包在第二房间 25 energy 的 link 转移完成 native 中心后抛异常。这样异常发生前两房都可产生真实候选写入，而权威状态仍必须保持旧版本。包目录在创建后删除，恢复读取 runtime 启动快照。

| 测试 | 组合 | 判据 |
|---|---|---|
| 自动停用后的缓存重试 | JSON/KKPV × 玩家并发上限 1/2，共四种；玩家派发顺序 p2/p1 | 两次结算异常都不发布；达到阈值后只停用 faulty；恢复失败不丢候选；只请求 p1 的重试仍发布两名玩家的缓存结果 |
| 晚期写回失败 | JSON/KKPV × RawMemory/Segments 写回点，共四种；并发上限 2 | 处理器成功后注入的写回失败回滚两房世界、Mod 状态和两名玩家状态；空请求重试整个缓存轮 |

每个自动停用组合还与一个干净参考臂对照：相同包和初态，先显式停用 faulty，串行按 p1/p2 执行一次，随后销毁参考 VM。故障臂成功恢复后的全部已放置对象与控制器、gameplay 状态和玩家表达式结果均与参考一致。不同 VM 的实际 CPU 用量不做数值相等比较。

## 锁定的行为

- **统一回滚**：失败后逐一比较两房全部已放置对象和控制器、两名玩家完整 PlayerState、Mod 持久状态、world tick 与 generation；Memory、segments、活动段请求、CPU 桶和通知均无提前发布，console 也无提前交付。
- **缓存轮完整性**：自动停用前后以及一次 VM 重建失败后，pending 仍包含两名玩家；最终 p1 子集请求和另一组空请求都返回两人的首轮结果。重试脚本故意抛异常，首轮全局执行计数保持 1，不能以重新执行玩家得到相同意图蒙混通过。
- **恰一次生效**：成功轮只有一次 tick/generation 增量；两名玩家 Memory 中 executions=1，各自 segment 与通知只提交一份，console 只交付一条；下一新轮仍读到全局计数 1，没有重复 console 或通知。
- **实际游戏结果**：counter 分别增加 2/3；对方 counter 的玩家 API 调用虽然返回 OK，但结算侧拒绝未授权写入。link source 分别为 90/75，target 为 9/24，保留 native 损耗与健康 wrap；Mod 计数为 increments=2、transfers=2、calls=4，重建丢弃失败 VM 的模块缓存。
- **下一轮投影**：JSON 与 KKPV 都能重新读取已提交 counter 值，FIND/LOOK 对象与直接 getter 引用一致；secret（owner 可见性）仅归属者可读，server-only 字段双方不可见。停用 faulty 不移除 gameplay 的玩家 API。
- **费用候选**：测试显式设玩家回充 0.25 ms、初始桶 100 ms，正费用按 ceil 后必使余额下降，避免封顶掩盖缺失计费；失败期余额完全不变，成功后才变化。没有用耗时区间判断费用，也没有据此测量性能。

## 验证与初次夹具失败

Debug 定向测试两项通过，遍历上述八个故障组合与四个干净参考臂。最初两次失败均来自新夹具：两个 wrap 重用了同 namespace 的处理器 ID，注册按契约拒绝；随后管理查询的控制器 ID 误取 `_id`，实际为 `id`。改为唯一 ID 与正确管理字段后通过，未改生产装载器或运行期行为。初次日志保留，最终命令和摘要随本片验证清单登记。

Release 定向两项也通过；全量检查的 Rust 595 项通过（10 项忽略）、JavaScript 19 项通过，没有跳过真实引擎冒烟测试。首次全量命令的最后一步发现新笔记尚未落盘的链接，代码/测试部分已通过；补齐文档后执行最终全量检查。固定版本 screeps-ok 棘轮为 1854 passed / 153 failed / 907 skipped，转红 0、转绿 0；已知失败不能写成全套通过。

三组 vanilla bot 的原始报告如下；这些是固定世界的沙箱加载/运行检查，玩家意图不送入世界结算，也没有安装本片 Mod 包，不能替代上面的闭环测试：

| 夹具 | 完成 / 请求 tick | 硬中断 / 惩罚跳过 | 含 console-error 的 tick |
|---|---:|---:|---:|
| [TooAngel](mod-multiplayer-tooangel-2026-10-04.json) | 2000 / 2000 | 0 / 0 | 0 |
| [Overmind](mod-multiplayer-overmind-2026-10-04.json) | 2000 / 2000 | 0 / 0 | 1 |
| [WASM](mod-multiplayer-wasm-2026-10-04.json) | 10 / 10 | 0 / 0 | 0 |

三组均无记录到的 missing API，bot-check 的判定通过，但其退出码不检查玩家逻辑错误。Overmind 首轮实例化 colony 的错误栈在替换 harvest 对象 ID 后，与[预算复审基线](mod-budget-review-validation-2026-10-03.json)全部一致，仍为一个 console-error tick；TooAngel 在普通 log 中捕获的 `offsetPosition.lookFor` 异常及折叠计数也与该基线一致。没有把两者视为零错误运行。

本片命令、源码/二进制/日志/报告摘要、初次失败和最终检查登记在[验证清单](mod-multiplayer-recovery-validation-2026-10-04.json)。

## 范围与后续

这证明了有限组合中玩家执行、Mod 结算、自动停用及写回恢复的接线；没有新增调度器自动重试，仍由调用方再次请求。也没有实现逐 Mod 资源隔离、持久化、输入日志、进程崩溃恢复或网络服务。性能历史差异仍按[平衡采集记录](mod-native-balanced-2026-10-04.md)保持未归因状态，本片不新增性能结论。

后续应扩展带真实 Mod 包的多 tick 工作负载，再量化玩家投影/执行与 Mod 结算在同一闭环中的成本；合成 counter/link 场景不能代表真实 bot 或完整 C 批性能矩阵完成。
