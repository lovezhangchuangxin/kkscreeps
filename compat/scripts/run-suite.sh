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
  echo "缺少 $ADAPTER——先在 compat/ 下 pnpm install && pnpm run build" >&2
  exit 1
fi
if [ ! -d "$ROOT/.compat/screeps-ok/node_modules" ]; then
  echo "缺少 .compat/screeps-ok——先运行 scripts/setup-screeps-ok.sh" >&2
  exit 1
fi
if [ -z "${KKSCREEPS_BIN:-}" ] && [ ! -x "$ROOT/target/release/kkscreeps" ] && [ ! -x "$ROOT/target/debug/kkscreeps" ]; then
  echo "缺少 kkscreeps 二进制——先 cargo build -p kk-cli" >&2
  exit 1
fi

cd "$ROOT/.compat/screeps-ok"
export SCREEPS_OK_ADAPTER="$ADAPTER"
export SCREEPS_OK_REPORT_NAME="kkscreeps"
export SCREEPS_OK_PROJECT_ROOT="$ROOT/compat"
export CI=1
exec npx vitest run "$@"
