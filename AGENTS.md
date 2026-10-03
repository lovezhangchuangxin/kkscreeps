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
│   ├── kk-runtime/         玩家执行编排、Memory/CPU 候选提交与失败重试
│   ├── kk-mod-api/         玩法声明、校验与冻结计划
│   ├── kk-mod-host/        可信 JS Mod 包装载与注册
│   ├── kk-pathfinder/      原生 C++ 寻路封装，尚未接入玩家搜索接口
│   ├── kk-cli/             embed、bot-check、perf-probe 命令入口
│   ├── kk-storage-bench/   存储引擎选型基准
│   ├── kk-storage/         持久化预留模块
│   └── kk-backend/         网络服务预留模块
├── compat/                 TypeScript 兼容性适配器、测试与回归报告
├── apps/game/              游戏客户端占位包
├── packages/               API、编辑器、渲染器与共享类型占位包
├── scripts/                维护、依赖准备与基准工具
├── docs/                   架构、设计、决策与贡献规范
└── spikes/                 独立实验，不属于 Rust workspace
```

各 crate 的源码位于 `src/`，集成测试位于 `tests/`；沙箱资源位于 `crates/kk-runner/assets/`。目录树表示模块职责；详细依赖和数据流见[总体架构](docs/architecture/overall.md)。主流程为：世界投影 → 玩家沙箱执行 → 收集指令 → 规则结算 → 提交候选状态，由 `kk-runtime` 协调。

## 构建、测试与开发命令

使用仓库固定的 Rust 工具链、Node.js 24+ 和 pnpm 12.4.2。在仓库根目录执行：

- `pnpm install`：安装依赖及 Git hooks。
- `cargo build -p kk-cli`：构建 `target/debug/kkscreeps`。
- `target/debug/kkscreeps embed`：在本地启动逐行 JSON 调用服务。
- `pnpm run fmt`：使用 rustfmt 格式化 Rust 代码。
- `pnpm run check`：执行格式检查、Clippy、Rust 测试、JavaScript workspace 测试和文档链接检查；不运行完整 screeps-ok 套件。
- `cargo test -p kk-engine`：单独运行引擎 crate 的测试。
- `KKSCREEPS_BIN="$PWD/target/debug/kkscreeps" pnpm --filter @kkscreeps/compat test`：构建适配器，并通过绝对路径选择上述 debug 二进制。

HTTP 服务和前端开发服务器尚未实现。

## 代码风格与命名约定

遵循 Rust 2024 约定：四空格缩进，函数和模块使用 `snake_case`，类型使用 `PascalCase`。格式以 rustfmt 为准；workspace 将 Clippy 的 `all` 组设为 `deny`。TypeScript/JavaScript 沿用现有风格：两空格缩进、单引号、分号和 ESM 导入；目前未配置 JavaScript 格式化或 lint 工具。

遵循[注释规范](docs/contributing/comment-style.md)：说明性注释默认使用简体中文，写清当前约束、原因及可核验的来源。

## 测试要求

使用 Rust 单元测试、集成测试及 Node 的 `node:test`。JavaScript 测试文件命名为 `*.test.mjs`，Rust 测试名称应体现被测行为。目前未配置覆盖率百分比门槛。

修改 Rust 后，须重新构建测试使用的二进制。适配器默认优先选择 release，再回退到 debug；建议通过 `KKSCREEPS_BIN` 显式指定，避免测试旧构建产物。未找到二进制且未显式指定时，引擎冒烟测试会跳过，不能据此判断引擎验证通过。

涉及兼容性的修改，按 [compat/README.md](compat/README.md) 构建 release CLI、准备固定版本的 screeps-ok 依赖、构建适配器，再运行 `node compat/scripts/ratchet.mjs`。门禁要求总用例数匹配配置、通过数不低于下限，且基线已通过用例不得出现未豁免的失败、跳过或缺失。

## 提交与 Pull Request 要求

历史提交使用描述性前缀，常见中文批次标签。Git hooks 要求首行采用“前缀：描述”，支持半角或全角冒号；标题最多 350 个 Unicode 码点，前缀最多 48 个。

PR 应说明行为变化、关联问题或设计决策，以及验证命令和结果。修改实现时，同步更新受影响的架构文档和兼容性证据。
