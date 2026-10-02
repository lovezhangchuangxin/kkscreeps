#!/usr/bin/env bash
# 在 .compat/screeps-ok 准备固定版本的上游套件、依赖和基准适配器。
# 已有 checkout 会更新到指定 commit，并恢复 package-lock.json 后改写下载地址。
# 准备完成后可在该目录运行 npm run parity，对比 vanilla 与 xxscreeps 的结果。
# 固定版本的设计依据：ADR-006。
set -euo pipefail

SCREEPS_OK_PIN="34a630efa42908c117c19c25d2350fa2387bd633"
REPO_URL="https://github.com/misterwise/screeps-ok.git"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/.compat/screeps-ok"

if [ ! -d "$DEST/.git" ]; then
  git clone "$REPO_URL" "$DEST"
fi
git -C "$DEST" fetch --all
git -C "$DEST" checkout -- package-lock.json 2>/dev/null || true
git -C "$DEST" checkout "$SCREEPS_OK_PIN"

# 上游 lockfile 把 isolated-vm 锁成 git+ssh（需要 GitHub SSH 密钥），
# 改成公开仓库的 HTTPS 地址，保留版本引用，避免依赖 GitHub SSH 密钥。
# -i.bak 在 BSD/GNU sed 上均可使用；生成的备份在下一行删除。
sed -i.bak 's|git+ssh://git@github.com/|git+https://github.com/|g' "$DEST/package-lock.json"
rm -f "$DEST/package-lock.json.bak"

cd "$DEST"
npm ci
npm run setup

echo "screeps-ok setup done (pin $SCREEPS_OK_PIN). Next: cd $DEST && npm run parity"
