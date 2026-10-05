# 源码组织与规模约束

主项目源码和测试按领域职责组织；单文件超过 1000 行时须拆分。行数是触发职责复审的门槛，不是架构质量的唯一指标。拆分应保留领域语义、注释、公开 API 和事务边界，不能通过压缩代码、删除必要注释或仅把连续代码搬到 `include!` 片段来过关。

`pnpm run check:source-size` 检查 `crates/compat/apps/packages/scripts` 下的 Rust、JavaScript、TypeScript 与 C/C++ 源码，包含 Git 跟踪和未忽略的新文件；已删除文件跳过。这项检查已纳入 `pnpm run check`。第三方固定版本 `crates/kk-runner/assets/lodash-3.10.1.js` 保留原文，例外在检查脚本中显式列出。`spikes/` 独立实验不属于主项目门禁；其中 inspector 实验的长文件保留为实验记录，不能照搬为生产模块。

## 生产模块导航

| 入口 | 子模块职责 |
|---|---|
| `kk-schema/src/kkpv.rs` | 公开值模型、限制和错误；`kkpv/wire.rs` 共享布局，`encode.rs` 编码与束校验，`view.rs` 单块校验与字段解码，`merge.rs` 同版本覆盖合并 |
| `kk-world/src/kkpv.rs` | 权威世界和 JSON 参考数据到块的转换；单元测试独立在 `kkpv/tests.rs` |
| `kk-engine/src/intent.rs` | 动作集合、执行优先级与输入清洗；测试按 JSON 收集、动作语义和市场输入分组 |
| `kk-engine/src/settle/mod.rs` | 世界引擎状态和控制入口；`working.rs` 房间候选及领域操作，`room_loop.rs` 房间循环，`install.rs` 冻结计划接线，`engine_tick.rs` 世界阶段与发布，`validation.rs` 发布前校验 |
| `kk-engine/src/settle/contrib.rs` | 槽、处理器和续行类型及运行入口；`contrib/assemble.rs` 冻结表装配，`execute.rs` 内建动作/tick 链执行与 Hosted 局部恢复（资源两端/事件、移动请求登记、战斗/建造族对象/延迟队列/链内增删） |
| `kk-engine/src/settle/resources.rs` | 资源领域读取与副本预检转账；`resources/generic.rs` 共享 Generic 仓库解析、native 容量与实验室槽写入 |
| `kk-mod-host/src/chain/resources.rs` | 资源 op 的同步能力访问、数字校验与故障来源 |
| `kk-mod-host/src/chain/movement.rs` | 移动请求 op 的方向校验与同步能力访问 |
| `kk-mod-host/src/chain/combat.rs` | 战斗改向/取消 op 的同步能力访问与故障来源 |
| `kk-runtime/src/embed/mod_chain/resources.rs` | 同房资源能力、首次触达日志与字段缓存失效；移动/战斗能力门控与 consumed 标记在 `mod_chain.rs` |
| `kk-engine/src/settle/market.rs` | 市场状态和结算入口、成交调度；`market/orders.rs` 订单生命周期，`terminal.rs` 发送/成交共用的库存、费用与交易写入 |
| `kk-runtime/src/run.rs` | 玩家执行参数、结果分类和未提交的指令/Memory/CPU 候选；由 crate 根导出，结算引擎不拥有这些类型 |
| `kk-runtime/src/embed.rs` | 分片状态、管理和诊断入口；`embed/setup.rs` 配置校验与创建/放置，`execution.rs` 玩家候选和失败重试，`projection.rs` KKPV 与市场视图，`mod_chain.rs` Mod 候选桥 |
| `kk-runner/src/sandbox.rs` | V8 实例生命周期、运行与写回；`sandbox/host.rs` 加载清单、宿主句柄与 Mod 玩家面安装，`inject.rs` 每轮 JSON/KKPV 与 CPU 窗口注入 |
| `kk-runner/src/prelude/30_game/game.js` | 世界注入、Game 装配和指令收集；`memory.js` 持有 RawMemory/segments 状态与写回，`market.js` 持有每轮市场视图缓存与指令 |

Rust 子模块通过父模块保留既有公开路径；跨子模块的内部操作使用受限可见性，不能因拆文件把内部候选暴露为新的外部写能力。KKPV 合并通过视图的公开读方法访问目录与元数据，保持视图内部字段私有。

JS 的 Memory、市场和 Game 分别持有闭包。加载清单先执行 Memory 和市场，再装配 Game；Game 在加载期捕获桥函数，Rust 捕获宿主句柄后删除临时 `__kkp`。tick 中不能重新查找这一全局。RawMemory 重建、CJS 重绑、Memory getter 安装的先后顺序保持不变，两个世界投影通道共享该流程。

## 测试组织

公共夹具留在测试族父模块，案例按规则、执行链、领域写入、安装门、故障/拒绝/停用等职责分组；Mod 链测试的共享工厂与放置/投影/ID 契约见 [mod_chain_tests 夹具契约](../../crates/kk-runtime/tests/mod_chain_tests/README.md)。结构拆分本身保持原测试注册数量；后续功能测试在对应子模块显式注册。`sandbox_tests` 与 `embed_engine_suite` 的串行场景仍由原入口按原顺序调用，子模块不新增独立 `#[test]`，避免改变 V8 生命周期和进程级计数的约束。

接近 1000 行时应提前检查职责：新增规则、协议形态或故障恢复场景应放到对应模块；只有实际形成新领域边界时才增加模块。阶段记录见[结构治理验收](../research/notes/source-structure-2026-10-05.md)。
