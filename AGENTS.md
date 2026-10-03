# 仓库贡献指南

## 项目结构与模块组织

`kkscreeps` 使用 Rust 重写 Screeps 服务端。`crates/` 是 Rust workspace，包含基础类型、世界状态、规则结算、V8 沙箱、后端协调、Mod 支持及命令行工具。各 crate 的源码位于 `src/`，集成测试位于 `tests/`；沙箱资源位于 `crates/kk-runner/assets/`。

`compat/` 包含 TypeScript screeps-ok 适配器、Node 测试、脚本和回归报告。`apps/game/` 与 `packages/{api,editor,renderer,shared}/` 是前端占位包。`scripts/` 存放维护及基准工具，`docs/` 存放架构、决策和贡献规范。`spikes/` 保存独立实验，不属于 Rust workspace。

## 构建、测试与开发命令

使用仓库固定的 Rust 工具链、Node.js 24+ 和 pnpm 12.4.2。在仓库根目录执行：

- `pnpm install`：安装依赖及 Git hooks。
- `cargo build -p kk-cli`：构建 `target/debug/kkscreeps`。
- `target/debug/kkscreeps embed`：在本地启动逐行 JSON 调用服务。
- `pnpm run fmt`：使用 rustfmt 格式化 Rust 代码。
- `pnpm run check`：执行格式检查、Clippy、Rust 测试、JavaScript workspace 测试和文档链接检查。
- `cargo test -p kk-engine`：单独运行引擎 crate 的测试。
- `pnpm --filter @kkscreeps/compat test`：构建并测试适配器。

HTTP 服务和前端开发服务器尚未实现。

## 代码风格与命名约定

遵循 Rust 2024 约定：四空格缩进，函数和模块使用 `snake_case`，类型使用 `PascalCase`。格式以 rustfmt 为准；workspace 将 Clippy 的 `all` 组设为 `deny`。TypeScript/JavaScript 沿用现有风格：两空格缩进、单引号、分号和 ESM 导入；目前未配置 JavaScript 格式化或 lint 工具。

遵循[注释规范](docs/contributing/comment-style.md)：说明性注释默认使用简体中文，写清当前约束、原因及可核验的来源。

## 测试要求

使用 Rust 单元测试、集成测试及 Node 的 `node:test`。JavaScript 测试文件命名为 `*.test.mjs`，Rust 测试名称应体现被测行为。目前未配置覆盖率百分比门槛。

涉及兼容性的修改应按 [compat/README.md](compat/README.md) 重新构建 release CLI、准备固定版本的 screeps-ok 依赖、构建适配器，并运行 `node compat/scripts/ratchet.mjs`。通过数不得低于下限，已通过用例不得出现未豁免回归。可通过 `KKSCREEPS_BIN` 指定二进制；未找到二进制时，引擎冒烟测试可能跳过。

## 提交与 Pull Request 要求

历史提交使用描述性前缀，常见中文批次标签。Git hooks 要求首行采用“前缀：描述”，支持半角或全角冒号；标题最多 350 个 Unicode 码点，前缀最多 48 个。

PR 应说明行为变化、关联问题或设计决策，以及验证命令和结果。修改实现时，同步更新受影响的架构文档和兼容性证据。
