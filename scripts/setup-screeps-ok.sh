#!/usr/bin/env bash
# screeps-ok 独立复现（M0 验收前置项，ADR-006 加固措施 1）：
# 按锁定 commit 克隆到 .compat/screeps-ok（gitignored），装依赖并构建两个基准适配器。
# 之后的 `npm run parity` 在该目录里跑，对账 vanilla 2897 / xxscreeps 2642 两个数字。
set -euo pipefail

SCREEPS_OK_PIN="34a630efa42908c117c19c25d2350fa2387bd633"
REPO_URL="https://github.com/misterwise/screeps-ok.git"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/.compat/screeps-ok"

if [ ! -d "$DEST/.git" ]; then
  git clone "$REPO_URL" "$DEST"
fi
git -C "$DEST" fetch --all
git -C "$DEST" checkout "$SCREEPS_OK_PIN"

# 上游 lockfile 把 isolated-vm 锁成 git+ssh（需要 GitHub SSH 密钥），
# 改写成 https 形式（同一 commit，无需凭据）。
sed -i '' 's|git+ssh://git@github.com/|git+https://github.com/|g' "$DEST/package-lock.json"

cd "$DEST"
npm ci
npm run setup

echo "screeps-ok setup done (pin $SCREEPS_OK_PIN). Next: cd $DEST && npm run parity"
