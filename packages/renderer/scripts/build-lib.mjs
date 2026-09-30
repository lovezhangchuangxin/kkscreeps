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

const publicDts = readFileSync(join(root, 'dist', 'index.d.ts'), 'utf8');
if (/from ['"]pixi\.js['"]/.test(publicDts)) {
  console.error('库构建守卫失败：dist/index.d.ts 引用了 pixi.js（ADR-009 公共面零 Pixi 类型）');
  process.exit(1);
}
console.log('库构建完成：dist/index.js + dist/*.d.ts（公共面零 pixi 引用 ✓）');
