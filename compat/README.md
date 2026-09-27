# compat/：兼容性验收

screeps-ok 适配器与 bot 环境验收工具（M0 交付，ADR-010）。

## 目录

```
compat/
├── src/                TS 源码
│   ├── adapter.ts        screeps-ok 适配器（createAdapter 导出）
│   ├── rpc.ts            kkscreeps embed sidecar 的 JSON-RPC 客户端
│   ├── capabilities.ts   29 项 capability 声明（M0 全 false，如实）
│   └── parity.json       空起步（M0 的失败都是真失败，即基线）
├── test/                冒烟测试（无二进制时自动跳过）
├── scripts/run-suite.sh 用本适配器跑 screeps-ok 全量套件
├── reports/             验收报告与基线产物（入库）
└── fixtures/            bot 代码（gitignored，scripts/fetch-bots.sh 拉取）
```

## 复现步骤

```bash
# 1. 构建引擎二进制
cargo build --release -p kk-cli

# 2. 准备 screeps-ok（pin 34a630e → .compat/screeps-ok，gitignored）
scripts/setup-screeps-ok.sh

# 3. 构建适配器
cd compat && pnpm install && pnpm run build

# 4. 跑全量套件出通过率报告（→ compat/reports/kkscreeps.json）
cd .. && compat/scripts/run-suite.sh

# bot 环境验收（tooangel / overmind / wasm 合成夹具）
scripts/fetch-bots.sh
target/release/kkscreeps bot-check --path compat/fixtures/tooangel/src --ticks 2000 --report /tmp/tooangel.json
target/release/kkscreeps bot-check --path compat/fixtures/overmind-dist --ticks 2000 --report /tmp/overmind.json
```

## 报告索引

| 报告 | 内容 |
|---|---|
| [reports/m0-screeps-ok-parity.md](reports/m0-screeps-ok-parity.md) | 上游基准独立复现（vanilla 2897 / xxscreeps 2642，零偏差） |
| [reports/m0-screeps-ok-baseline.md](reports/m0-screeps-ok-baseline.md) | 我们的适配器首份通过率基线（M0） |
| [reports/m0-bot-missing-api.md](reports/m0-bot-missing-api.md) | bot 环境验收：缺失 API 清单与登记事项 |
