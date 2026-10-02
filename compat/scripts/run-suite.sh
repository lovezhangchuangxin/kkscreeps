#!/usr/bin/env bash
# 用 kkscreeps 适配器跑 screeps-ok 全量套件，产出通过率基线报告。
# 前置：
#   1. cargo build --release -p kk-cli（或设置 KKSCREEPS_BIN 指向二进制）
#   2. scripts/setup-screeps-ok.sh 已运行（.compat/screeps-ok 就绪）
#   3. cd compat && pnpm install && pnpm run build
# 产物：compat/reports/kkscreeps.json（vitest JSON 报告）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ADAPTER="$ROOT/compat/dist/adapter.js"
if [ ! -f "$ADAPTER" ]; then
  echo "缺少 ${ADAPTER}——先在 compat/ 下 pnpm install && pnpm run build" >&2
  exit 1
fi
# dist 新鲜度守卫（2B 批 P1 评审 3 登记：陈旧 dist 会静默用旧 capability 跑
# 门禁——P4「陈旧 release 二进制」同类事故）。src 任一文件新于 dist 即重建
if [ ! -f "$ROOT/compat/dist/parity.json" ] \
  || [ "$ROOT/compat/src/parity.json" -nt "$ROOT/compat/dist/parity.json" ] \
  || [ -n "$(find "$ROOT/compat/src" -name '*.ts' -newer "$ADAPTER" 2>/dev/null)" ]; then
  echo "compat/src 新于 dist——重建适配器（防陈旧 capability 跑门禁）" >&2
  (cd "$ROOT/compat" && pnpm run build >/dev/null 2>&1) || { echo "重建失败" >&2; exit 1; }
fi
if [ ! -d "$ROOT/.compat/screeps-ok/node_modules" ]; then
  echo "缺少 .compat/screeps-ok——先运行 scripts/setup-screeps-ok.sh" >&2
  exit 1
fi
if [ -z "${KKSCREEPS_BIN:-}" ] && [ ! -x "$ROOT/target/release/kkscreeps" ] && [ ! -x "$ROOT/target/debug/kkscreeps" ]; then
  echo "缺少 kkscreeps 二进制——先 cargo build -p kk-cli" >&2
  exit 1
fi

# 可选：--quiet 把 vitest 输出转存 /tmp/kk-run-suite.log（stdout 清空，退出码不变）
# ——供 CI/工作流调用，避免海量测试输出撑爆调用方的输出上限
QUIET=0
if [ "${1:-}" = "--quiet" ]; then
  QUIET=1
  shift
fi

cd "$ROOT/.compat/screeps-ok"
export SCREEPS_OK_ADAPTER="$ADAPTER"
export SCREEPS_OK_REPORT_NAME="kkscreeps"
export SCREEPS_OK_PROJECT_ROOT="$ROOT/compat"
export CI=1
if [ "$QUIET" = "1" ]; then
  npx vitest run "$@" > /tmp/kk-run-suite.log 2>&1
else
  exec npx vitest run "$@"
fi
