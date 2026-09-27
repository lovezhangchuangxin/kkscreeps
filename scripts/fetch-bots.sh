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

# tooangel 源码（src/ 即为上传产物形态）。pin 固定 commit（ADR-006 复现纪律：
# 夹具内容固化在可对账的版本；与 SCREEPS_OK_PIN 同款机制，重跑可复现）
TOOANGEL_PIN="87645a0b933360224737cca3b18ae9d112f46fef"
if [ ! -d "$DEST/tooangel/.git" ]; then
  git clone https://github.com/TooAngel/screeps.git "$DEST/tooangel"
fi
git -C "$DEST/tooangel" fetch --all
git -C "$DEST/tooangel" checkout "$TOOANGEL_PIN"
TOOANGEL_COMMIT="$(git -C "$DEST/tooangel" rev-parse HEAD)"

# overmind：release v0.5.2.1 的 main.js。
# -f 让 HTTP 404/限流以非零退出（无 -f 时 curl exit=0，错误响应体如 9 字节 "Not Found"
# 会被写进 main.js 且被 [ ! -s ] 缓存条件固化）；下载后做最小长度校验兜底
OVERMIND_URL="https://github.com/bencbartlett/Overmind/releases/download/v0.5.2.1/main.js"
mkdir -p "$DEST/overmind-dist"
if [ ! -s "$DEST/overmind-dist/main.js" ]; then
  curl -fsSL "$OVERMIND_URL" -o "$DEST/overmind-dist/main.js.tmp"
  if [ "$(wc -c < "$DEST/overmind-dist/main.js.tmp" | tr -d ' ')" -lt 1000 ]; then
    rm -f "$DEST/overmind-dist/main.js.tmp"
    echo "overmind main.js 下载异常（长度 <1000 字节），已丢弃半成品" >&2
    exit 1
  fi
  mv "$DEST/overmind-dist/main.js.tmp" "$DEST/overmind-dist/main.js"
fi

# 合成 wasm 夹具：require('mod.wasm') 返回 Buffer → 编译为 WebAssembly.Module
mkdir -p "$DEST/wasm-bot"
if [ ! -s "$DEST/wasm-bot/main.js" ] || [ ! -s "$DEST/wasm-bot/mod.wasm" ]; then
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
echo "  tooangel:  $DEST/tooangel/src ($(ls "$DEST/tooangel/src" | wc -l | tr -d ' ') 个模块，pin $TOOANGEL_COMMIT)"
echo "  overmind:  $DEST/overmind-dist/main.js ($(wc -c < "$DEST/overmind-dist/main.js" | tr -d ' ') 字节)"
echo "  wasm-bot:  $DEST/wasm-bot（合成，覆盖二进制模块 require）"
