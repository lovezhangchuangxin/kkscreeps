#!/usr/bin/env bash
# 在 compat/fixtures/ 准备 bot-check 使用的两个真实 bot 和一个合成 WASM 夹具。
# TooAngel 使用固定 commit 的 src/，Overmind 直接下载固定 release 的 main.js。
# WASM 夹具用于检查 require 二进制模块后能否构造 WebAssembly.Module。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/compat/fixtures"
mkdir -p "$DEST"

# TooAngel 按 commit 固定源码版本，不在这里构建或修改 src/；依据见 ADR-006。
TOOANGEL_PIN="87645a0b933360224737cca3b18ae9d112f46fef"
if [ ! -d "$DEST/tooangel/.git" ]; then
  git clone https://github.com/TooAngel/screeps.git "$DEST/tooangel"
fi
git -C "$DEST/tooangel" fetch --all
git -C "$DEST/tooangel" checkout "$TOOANGEL_PIN"
TOOANGEL_COMMIT="$(git -C "$DEST/tooangel" rev-parse HEAD)"

# overmind：release v0.5.2.1 的 main.js。
# curl -f 拒绝 HTTP 错误响应；下载到临时文件，通过最小长度检查后再替换正式文件。
# 已存在的非空 main.js 会直接复用，此处不校验其版本或内容。
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
// 合成 bot：加载包含 WASM 文件头和版本号的空模块，检查二进制 require 与编译路径。
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
