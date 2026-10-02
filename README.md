# kkscreeps

用 Rust 重写 Screeps 服务端。Screeps 是由玩家编写 JavaScript 控制单位的游戏：服务器按 tick 推进世界，玩家代码读取世界、下达指令，服务器随后结算采集、建造、移动和战斗等行为。

项目以默认玩法的玩家 API 和游戏行为兼容为目标，同时探索降低运行开销和改善本地开发体验的方法。兼容性与性能目标需要分别通过行为测试和端到端基准验证，目前尚未提供完整游戏服务器。

## 当前能力

- 已有内存世界、游戏规则结算、V8 玩家沙箱，以及玩家执行、候选提交和失败重试的协调层。
- `kkscreeps embed` 提供标准输入/输出的逐行 JSON 调用服务；`bot-check` 检查真实 bot 的运行情况；`perf-probe` 采集固定负载下的性能数据。
- `serve`、`sim`、`test`、`import` 仍为预留命令，调用时返回退出码 2。HTTP/WebSocket 服务、数据库持久化和前端应用尚未实现。
- `kk-pathfinder` 已提供独立的原生寻路接口，但玩家沙箱中的 `PathFinder.search` 尚未接入它，目前使用忽略障碍的直线近似。

兼容性测试的结果见 [最新入库报告](compat/reports/kkscreeps.json)，通过数下限与用例总数由 [ratchet.json](compat/reports/ratchet.json) 管理。启用某项适配器能力表示允许运行对应测试，不表示该组全部通过。具体运行方法和回归判定见 [兼容性验收说明](compat/README.md)。

## 本地构建与检查

Rust 工具链由 [rust-toolchain.toml](rust-toolchain.toml) 固定；Node 和 pnpm 的要求见 [package.json](package.json)。在仓库根目录运行：

```bash
pnpm install
cargo build -p kk-cli
pnpm run check
```

`pnpm run check` 执行 Rust 格式检查、Clippy、workspace 测试、JavaScript workspace 测试和本地文档链接检查。它不运行完整 screeps-ok 套件；后者还需准备上游依赖和 release 二进制，步骤见 [compat/README.md](compat/README.md)。

构建后可使用 `target/debug/kkscreeps --help` 查看命令。`embed` 的消息格式与适配器入口见 [总体架构](docs/architecture/overall.md)和 [compat/README.md](compat/README.md)。

## 阅读入口

完整导航见 [文档目录](docs/README.md)。

| 要了解的问题 | 文档 |
|---|---|
| 当前模块职责、调用链与未实现的边界 | [总体架构](docs/architecture/overall.md) |
| 玩家执行、世界结算和失败重试 | [游戏循环](docs/architecture/game-loop.md)、[tick 契约](docs/architecture/tick-contract.md) |
| 兼容性要求、已知差异和验收方法 | [兼容性设计](docs/designs/compatibility.md)、[验收工具](compat/README.md) |
| 技术选择及其依据 | [架构决策记录](docs/decisions/README.md)、[研究证据](docs/research/README.md) |
| 未来范围与验收目标 | [路线图](docs/roadmap.md)、[功能设计](docs/designs/README.md) |
| 维护注释与理解术语 | [注释规范](docs/contributing/comment-style.md)、[术语词典](docs/glossary.md) |

## 仓库布局

```text
crates/     Rust workspace：基础数据、世界、结算、沙箱、协调层与命令行工具
compat/     screeps-ok 适配器、运行脚本、回归判定与报告
apps/       客户端应用的预留位置
packages/   渲染器、API、编辑器和共享类型的占位包
scripts/    常量生成、依赖准备、bot 下载与提交/文档检查
spikes/     独立技术验证工程，保留实验代码与证据
docs/      架构、设计、决策、计划与研究文档
```

Rust workspace 的实际成员和依赖见各 crate 的 `Cargo.toml`；模块说明见 [总体架构](docs/architecture/overall.md)。前端包目前只有占位测试，计划职责见 [前端设计](docs/architecture/frontend.md)。

## 提交检查

`pnpm install` 通过根目录的 `prepare` 脚本安装 Git hooks：

- `pre-commit` 和 `pre-merge-commit` 检查 Rust 格式。失败时运行 `pnpm run fmt`，检查并重新暂存改动。
- `commit-msg` 要求首行采用“前缀：描述”，支持全角或半角冒号。标题至多 350 个 Unicode 码点，前缀至多 48 个；匹配 merge/revert/fixup/squash 前缀的标题免检。

修改 hook 配置后运行 `pnpm exec simple-git-hooks` 重新安装。常规质量检查仍需执行 `pnpm run check`；hook 的配置与命令以 [package.json](package.json) 为准。
