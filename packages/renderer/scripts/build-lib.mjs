/**
 * 库构建编排（R6）：清 dist → vite bundle（ESM，pixi/shared 外置）→ tsc d.ts →
 * 公共面守卫（dist/index.d.ts 不得引用 pixi——ADR-009「Pixi 对象绝不进公共
 * API」的产物级 CI 锚）。
 */
import { spawnSync } from 'node:child_process';
import { rmSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const run = (cmd, args) => {
  const r = spawnSync(cmd, args, { cwd: root, stdio: 'inherit', shell: false });
  if (r.status !== 0) process.exit(r.status ?? 1);
};

rmSync(join(root, 'dist'), { recursive: true, force: true });
run('pnpm', ['exec', 'vite', 'build', '--config', 'vite.build.config.ts']);
run('pnpm', ['exec', 'tsc', '-p', 'tsconfig.build.json']);

// 公共面守卫（评审 B-B3 二轮收紧）：从 dist/index.d.ts 出发遍历相对 import
// 闭包，闭包内任何 d.ts 不得引用 pixi——字面匹配入口文件会被内部模块转手的
// 类型传递绕过。
const dist = join(root, 'dist');
const seen = new Set();
const queue = ['index.d.ts'];
let pixiHit = null;
while (queue.length > 0 && pixiHit === null) {
  const rel = queue.pop();
  if (seen.has(rel)) continue;
  seen.add(rel);
  const text = readFileSync(join(dist, rel), 'utf8');
  if (/(?:from|import)\s*['"]pixi\.js['"]/.test(text)) { pixiHit = rel; break; }
  for (const m of text.matchAll(/from\s+'(\.[^']+)'/g)) {
    // tsc d.ts 的相对 import 以 .js 结尾，产物文件是 .d.ts
    queue.push(m[1].replace(/^\.\//, '').replace(/\.js$/, '.d.ts'));
  }
}
if (pixiHit !== null) {
  console.error(`库构建守卫失败：公共 d.ts 闭包内 ${pixiHit} 引用了 pixi.js（ADR-009 公共面零 Pixi 类型）`);
  process.exit(1);
}
console.log(`库构建完成：dist/index.js + dist/*.d.ts（公共面闭包 ${seen.size} 文件零 pixi 引用 ✓）`);
