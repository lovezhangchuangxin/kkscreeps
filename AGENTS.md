# 仓库贡献指南

## 项目结构与模块组织

`kkscreeps` 使用 Rust 重写 Screeps 服务端，主要模块及职责如下：

```text
kkscreeps/
├── crates/                 Rust workspace
│   ├── kk-core/            游戏常量、坐标、地形与随机数工具
│   ├── kk-schema/          对象字段定义、二进制快照编解码
│   ├── kk-world/           内存世界、空间索引与玩家视野投影
│   ├── kk-engine/          指令清洗、规则结算与世界候选状态提交
│   ├── kk-runner/          V8 玩家沙箱、玩家 API 与指令收集
│   ├── kk-vm/              玩家/玩法 VM 共用 CPU 时钟、看门狗与资源预算
│   ├── kk-runtime/         玩家执行编排、Memory/CPU 候选提交与失败重试
│   ├── kk-mod-api/         玩法声明、校验与冻结计划
│   ├── kk-mod-host/        可信 JS Mod 包装载与注册
│   ├── kk-pathfinder/      原生 C++ 寻路封装，经 kk-runner op 供玩家面搜索
│   ├── kk-cli/             embed、sim run、mod inspect、bot-check 与性能命令入口
│   ├── kk-storage-bench/   存储引擎选型基准
│   ├── kk-storage/         持久化预留模块
│   └── kk-backend/         网络服务预留模块
├── compat/                 TypeScript 兼容性适配器、测试与回归报告
├── apps/game/              游戏客户端占位包
├── packages/               API、编辑器、渲染器与共享类型占位包
├── mods/official/           使用公共 JS SDK 的官方内容原型
├── scripts/                维护、依赖准备与基准工具
├── docs/                   架构、设计、决策、计划与研究文档
│   ├── README.md           文档目录与分工索引
│   ├── architecture/       模块机制、接口边界与目标架构；未实现的能力明确标注
│   ├── designs/            玩家行为与功能的设计要求及验收条件
│   ├── decisions/          ADR 技术决策及取舍理由
│   ├── plans/              开发批次的拆分与阶段验收
│   ├── research/           调研、实验与原始证据（含批次交付笔记）
│   ├── contributing/       注释规范等贡献约定
│   ├── roadmap.md          里程碑范围与验收目标
│   └── glossary.md         术语词典
└── spikes/                 独立实验，不属于 Rust workspace
```

各 crate 的源码位于 `src/`，集成测试位于 `tests/`；沙箱资源位于 `crates/kk-runner/assets/`。目录树表示模块职责；详细依赖和数据流见[总体架构](docs/architecture/overall.md)。主流程为：世界投影 → 玩家沙箱执行 → 收集指令 → 规则结算 → 提交候选状态，由 `kk-runtime` 协调。

阅读 `docs/` 时先区分当前实现与目标设计——文档采用某个方案不代表功能已实现；`plans/` 与 `research/` 保留开发过程和原始证据、不追改结论，批次完成后现行机制以 `architecture/` 为准。按问题查找入口的完整索引见 [docs/README.md](docs/README.md)。

## AI 接手与进度维护

接手 Mod 开发或恢复上下文时，先阅读[后续任务与接手台账](docs/plans/mod-next-batches.md)和[完整玩法计划](docs/plans/mod-complete-gameplay.md)，核对实际 HEAD、工作区、相关架构与验收证据，再按台账的执行队列与子片依赖选择当前任务，不按父编号机械串行。新增写入域须当批补齐保存恢复、版本策略和验证，不能等远期恢复任务才接线。台账是持续更新的执行入口；历史计划和 research 记录不能直接替代当前进度。

开始任务、调整范围/依赖、完成批次，以及结束开发会话或转交其他 AI 前，主动更新台账的状态、当前接手位置和实施记录：写清已完成事项、剩余消费者、实际验证、复审问题、提交或未提交工作、下一步。不得只在聊天里交代，不得把未执行检查写成通过或把首个切片标成全领域完成。

实现变化须同步现行 architecture、SDK 使用说明、独立 JS 样例与 research 机器证据；里程碑变化同步 roadmap 和完整玩法计划，新增文档同步索引。历史证据保留当时结论；当前台账持续维护并追加可追溯记录。单纯文档修改无需虚构 JS 示例或功能交付记录，执行适用的文档与仓库门禁即可。

## 构建、测试与开发命令

使用仓库固定的 Rust 工具链、Node.js 24+ 和 pnpm 12.4.2。在仓库根目录执行：

- `pnpm install`：安装依赖及 Git hooks。
- `cargo build -p kk-cli`：构建 `target/debug/kkscreeps`。
- `target/debug/kkscreeps embed`：在本地启动逐行 JSON 调用服务。
- `pnpm run fmt`：使用 rustfmt 格式化 Rust 代码。
- `pnpm run check`：执行格式检查、Clippy、Rust 测试、JavaScript workspace 测试和文档链接检查；不运行完整 screeps-ok 套件。
- `pnpm run check:fast`：开发中程快车道，步骤同 `check` 但 Rust 测试跳过 doctest（rustdoc 每次全量重编 doctest 二进制，约占热缓存测试时间四分之三）；提交前仍须跑完整 `check`。可限定 crate：`pnpm run check:fast -- kk-engine kk-runtime`。
- `cargo test -p kk-engine`：单独运行引擎 crate 的测试。
- `KKSCREEPS_BIN="$PWD/target/debug/kkscreeps" pnpm --filter @kkscreeps/compat test`：构建适配器，并通过绝对路径选择上述 debug 二进制。

HTTP 服务和前端开发服务器尚未实现。

## 代码风格与命名约定

遵循 Rust 2024 约定：四空格缩进，函数和模块使用 `snake_case`，类型使用 `PascalCase`。格式以 rustfmt 为准；workspace 将 Clippy 的 `all` 组设为 `deny`。TypeScript/JavaScript 沿用现有风格：两空格缩进、单引号、分号和 ESM 导入；目前未配置 JavaScript 格式化或 lint 工具。

遵循[注释规范](docs/contributing/comment-style.md)：说明性注释默认使用简体中文，写清当前约束、原因及可核验的来源。

主项目源码和测试文件不得超过 1000 行，`pnpm run check:source-size` 检查此门槛并纳入 `pnpm run check`。接近门槛时按领域职责拆分，保持公开 API、候选提交边界和串行测试顺序；不要通过压缩代码或删除必要注释降低行数。第三方固定源码与独立实验的范围说明，以及当前模块导航见[源码组织](docs/architecture/source-organization.md)。

## 测试要求

使用 Rust 单元测试、集成测试及 Node 的 `node:test`。JavaScript 测试文件命名为 `*.test.mjs`，Rust 测试名称应体现被测行为。目前未配置覆盖率百分比门槛。

修改 Rust 后，须重新构建测试使用的二进制。适配器默认优先选择 release，再回退到 debug；建议通过 `KKSCREEPS_BIN` 显式指定，避免测试旧构建产物。未找到二进制且未显式指定时，引擎冒烟测试会跳过，不能据此判断引擎验证通过。

涉及兼容性的修改，按 [compat/README.md](compat/README.md) 构建 release CLI、准备固定版本的 screeps-ok 依赖与 bot 基线（`bash scripts/fetch-bots.sh`，tooangel/overmind/wasm 三夹具）、构建适配器，再运行 `node compat/scripts/ratchet.mjs`。门禁要求总用例数匹配配置、通过数不低于下限，且基线已通过用例不得出现未豁免的失败、跳过或缺失。

## 提交与 Pull Request 要求

历史提交使用描述性前缀，常见中文批次标签。`pnpm install` 通过 `prepare` 脚本安装 Git hooks，配置以 [package.json](package.json) 为准，修改后运行 `pnpm exec simple-git-hooks` 重新安装。`pre-commit` 与 `pre-merge-commit` 检查 Rust 格式，未过时运行 `pnpm run fmt` 修复后重新暂存；`commit-msg` 要求首行采用“前缀：描述”，支持半角或全角冒号，标题最多 350 个 Unicode 码点、前缀最多 48 个，merge/revert/fixup/squash 标题免检。

PR 应说明行为变化、关联问题或设计决策，以及验证命令和结果。修改实现时，同步更新受影响的架构文档和兼容性证据。
