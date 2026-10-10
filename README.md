# kkscreeps

用 Rust 重写 Screeps 服务端。Screeps 是由玩家编写 JavaScript 控制单位的游戏：服务器按 tick 推进世界，玩家代码读取世界、下达指令，服务器随后结算采集、建造、移动和战斗等行为。

项目以默认玩法的玩家 API 和游戏行为兼容为目标，同时探索降低运行开销和改善本地开发体验的方法。兼容性与性能目标需要分别通过行为测试和端到端基准验证，目前尚未提供完整游戏服务器。

## 当前能力

- 已有内存世界、游戏规则结算、V8 玩家沙箱，以及玩家执行、候选提交和失败重试的协调层。
- `kkscreeps embed` 提供标准输入/输出的逐行 JSON 调用服务；`bot-check` 检查真实 bot 的运行情况；`perf-probe` 采集固定负载下的性能数据，`mod-perf-probe` 对比密集对象的纯结算与 Mod 处理链成本。 50 玩家×6 个满功能成熟房的本地 xxscreeps 对照见[基准使用说明](scripts/benchmark/README.md)。
- `kkscreeps sim run` 加载 JSON 场景、玩家模块及显式 JS Mod 包，推进真实结算并检查已提交状态；支持多玩家、JSON/KKPV 投影、Mod state 断言和内容/计划身份报告，见[本地场景模拟](docs/architecture/local-simulation.md)。
- `kkscreeps mod services` 在独立后台 VM 验证真实包的 start/request/stop 与有限只读异步探针，见[后台服务](docs/architecture/background-services.md)；耐久 outbox/storage provider 与网络监听仍待后续。
- `kkscreeps mod inspect` 注册真实 JS 包并输出处理器依赖、冻结/活动链、规则来源和停用影响预览，见[组合诊断](docs/architecture/mod-composition.md)。
- 独立 JS 包可声明[新资源](docs/architecture/mod-resource-catalog.md)和[可建造生产建筑](docs/architecture/mod-buildings.md)，并在近战/单体远程槽中提供[自定义伤害策略](docs/architecture/combat.md#单体伤害)，在近程/远程治疗槽中提供[自定义治疗](docs/architecture/combat.md#治疗)，在群体远程槽中提供[批量伤害策略](docs/architecture/combat.md#群体伤害)，在三个攻击槽中配置[反击策略](docs/architecture/combat.md#反击)；其余领域按[完整玩法计划](docs/plans/mod-complete-gameplay.md)继续开放。
- 显式完整状态保存与重启续跑见[基础检查点](docs/architecture/checkpoints.md)，`sim run` 支持 `--save-checkpoint` / `--load-checkpoint`、周期保存 `--checkpoint-every` 与目录恢复（[周期保存与恢复选择](docs/architecture/checkpoints.md#周期保存与恢复选择g26a)）。
- `serve`、`test`、`import` 仍为预留命令，调用时返回退出码 2。HTTP/WebSocket 服务、数据库持久化和前端应用尚未实现。
- `kk-pathfinder` 的原生寻路已接入玩家沙箱：`PathFinder.search`/`Room.findPath`/`moveTo` 使用真实地形和代价矩阵，支持同步 roomCallback/costCallback、嵌套搜索、异常传播及预算中断。
- 玩家可在容器等 Generic 仓库间真实搬运资源；分片创建应用 GCL 等级与进度，升级累计和认领扩张通过同一候选提交机制生效。机制见[游戏循环](docs/architecture/game-loop.md#仓库资源与-gcl)。

兼容性测试的结果见 [最新入库报告](compat/reports/kkscreeps.json)，通过数下限与用例总数由 [ratchet.json](compat/reports/ratchet.json) 管理。启用某项适配器能力表示允许运行对应测试，不表示该组全部通过。具体运行方法和回归判定见 [兼容性验收说明](compat/README.md)。

## 本地构建与运行

Rust 工具链由 [rust-toolchain.toml](rust-toolchain.toml) 固定；Node 和 pnpm 的要求见 [package.json](package.json)。在仓库根目录运行：

```bash
pnpm install
cargo build -p kk-cli
target/debug/kkscreeps --help
```

`embed` 的消息格式见 [总体架构](docs/architecture/overall.md)；适配器与真实 bot 的运行步骤见 [compat/README.md](compat/README.md)。

## 阅读入口

完整导航见 [文档目录](docs/README.md)。

| 要了解的问题 | 文档 |
|---|---|
| 当前模块职责、调用链与未实现的边界 | [总体架构](docs/architecture/overall.md) |
| 玩家执行、世界结算和失败重试 | [游戏循环](docs/architecture/game-loop.md)、[tick 契约](docs/architecture/tick-contract.md) |
| 兼容性要求、已知差异和验收方法 | [兼容性设计](docs/designs/compatibility.md)、[验收工具](compat/README.md) |
| 技术选择及其依据 | [架构决策记录](docs/decisions/README.md)、[研究证据](docs/research/README.md) |
| 未来范围与验收目标 | [路线图](docs/roadmap.md)、[功能设计](docs/designs/README.md)、[前端架构](docs/architecture/frontend.md) |
| 维护注释与理解术语 | [注释规范](docs/contributing/comment-style.md)、[术语词典](docs/glossary.md) |

开发与贡献流程（完整命令、测试门禁、提交规范、仓库布局）见 [AGENTS.md](AGENTS.md)。
