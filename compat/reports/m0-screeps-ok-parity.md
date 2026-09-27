# M0 报告：screeps-ok 独立复现（parity 对账）

> 2026-09-27。本报告是 ADR-006 加固措施 1 的交付物：在依赖 screeps-ok 作为兼容验收关卡之前，
> 先独立复现它宣称的基准数字，验证其基准是真实的。

## 结论

**对账通过，零偏差。** 本地全量复现的数字与上游 `docs/status.md`（同 commit）逐项一致：

| 适配器 | passed | expected-failure | skipped | 上游数字 | 偏差 |
|---|---|---|---|---|---|
| vanilla（官方引擎） | **2897** | 10 | 7 | 2897 / 10 / 7 | 无 |
| xxscreeps | **2642** | 77 | 195 | 2642 / 77 / 195 | 无 |

vanilla 的 10 个 expected-failure 全部落在上游注册的 gap 内，`npm run parity` 退出码 0（其退出码规则：所有失败都在注册 gap 内即视为通过）。

## 环境

| 项 | 值 |
|---|---|
| 上游 pin | `34a630efa42908c117c19c25d2350fa2387bd633`（master，2026-09-26） |
| 获取方式 | `scripts/setup-screeps-ok.sh`（克隆到 `.compat/screeps-ok`，gitignored） |
| Node | v24.11.0（engines 要求 ≥24） |
| OS | macOS 15.7.3（arm64） |
| 原生模块 | isolated-vm（laverdet fork）、@screeps/driver native.node、xxscreeps dist 全部本地编译成功 |

复现命令：`scripts/setup-screeps-ok.sh && cd .compat/screeps-ok && npm run parity`（全量两适配器约 10 分钟）。

## 过程中的处置

1. 上游 lockfile 把 `isolated-vm` 锁成 `git+ssh://git@github.com/...`（需要 GitHub SSH 密钥），
   setup 脚本将其改写为 `git+https://` 等价形式（同一 commit），无需凭据即可安装；
2. 除此外零干预——`npm ci` → `npm run setup`（xxscreeps 构建 + vanilla 原生模块编译）→ `npm run parity` 一条链路直通。

## 对后续里程碑的含义

- 2897 / 2642 两个数字确认可信，可作为我们的通过率棘轮基线锚点（M1 起接我们的适配器）；
- screeps-ok 的 vanilla 基准（真实官方引擎子进程）与 xxscreeps 基准都在本机可复现，
  M1+ 出现分歧时可以三方对照（官方 / xxscreeps / 我们）定位归因。
