#!/usr/bin/env bash
# 拉取 M0 环境验收用的两个真实 bot 到 compat/fixtures/（gitignored）。
# - tooangel：多模块纯 JS + 重 lodash（src/ 三十余个模块，xxscreeps README 同款演示路径）
# - overmind：取 GitHub release 的构建产物单文件 main.js（仓库构建链为 2018 年代，
#   rollup 0.62 + TS 2.9，在 Node 24 下不可用——按 M0 预案绕过）
# - wasm 语义：两个 bot 均未带 .wasm 模块，用合成夹具 wasm-bot 覆盖 require 二进制模块路径
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/compat/fixtures"
mkdir -p "$DEST"

# tooangel 源码（src/ 即为上传产物形态）
if [ ! -d "$DEST/tooangel/.git" ]; then
  git clone https://github.com/TooAngel/screeps.git "$DEST/tooangel"
fi
git -C "$DEST/tooangel" fetch --all
git -C "$DEST/tooangel" checkout HEAD

# overmind：release v0.5.2.1 的 main.js
mkdir -p "$DEST/overmind-dist"
if [ ! -s "$DEST/overmind-dist/main.js" ]; then
  curl -sL https://github.com/bencbartlett/Overmind/releases/download/v0.5.2.1/main.js \
    -o "$DEST/overmind-dist/main.js"
fi

# 合成 wasm 夹具：require('mod.wasm') 返回 Buffer → 编译为 WebAssembly.Module
mkdir -p "$DEST/wasm-bot"
if [ ! -s "$DEST/wasm-bot/main.js" ]; then
  cat > "$DEST/wasm-bot/main.js" <<'EOF'
// 合成 bot：验证官方语义——二进制模块 require 返回 Buffer，可直接喂给 WebAssembly
module.exports.loop = function () {
  if (!globalThis.__wasmChecked) {
    const bytes = require('mod.wasm');
    const ok = typeof bytes === 'object' &&
      bytes.length === 8 &&
      bytes[0] === 0 && bytes[1] === 97 && // \0asm 魔数
      typeof WebAssembly.Module === 'function';
    new WebAssembly.Module(bytes);
    globalThis.__wasmChecked = true;
    globalThis.__wasmOk = ok;
    console.log('wasm module loaded:', ok);
  }
};
EOF
  printf '\x00\x61\x73\x6d\x01\x00\x00\x00' > "$DEST/wasm-bot/mod.wasm"
fi

echo "bots 已就位："
echo "  tooangel:  $DEST/tooangel/src ($(ls "$DEST/tooangel/src" | wc -l | tr -d ' ') 个模块)"
echo "  overmind:  $DEST/overmind-dist/main.js ($(wc -c < "$DEST/overmind-dist/main.js" | tr -d ' ') 字节)"
echo "  wasm-bot:  $DEST/wasm-bot（合成，覆盖二进制模块 require）"
