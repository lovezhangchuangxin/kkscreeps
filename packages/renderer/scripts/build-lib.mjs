/**
 * 库构建编排（R6）：清 dist → vite bundle（ESM，pixi/shared 外置）→ tsc d.ts →
 * 公共面守卫（dist/index.d.ts 不得引用 pixi——ADR-009「Pixi 对象绝不进公共
 * API」的产物级 CI 锚）。
 */
import { spawnSync } from 'node:child_process';
import { rmSync } from 'node:fs';
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

// R7 AST graph guard covers import(), subpaths and bare-package re-exports.
run('node', ['scripts/check-public-types.mjs', 'dist/index.d.ts']);
console.log('库构建完成：dist/index.js + dist/*.d.ts');
