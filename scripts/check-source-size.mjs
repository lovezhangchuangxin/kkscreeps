// 1000 行是职责复审门槛；应拆分领域模块，不能靠压缩代码或删除必要注释过关。
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, extname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const roots = ['crates/', 'compat/', 'apps/', 'packages/', 'scripts/'];
const extensions = new Set(['.rs', '.js', '.mjs', '.ts', '.tsx', '.jsx', '.cpp', '.cc', '.c', '.h', '.hpp']);
const exceptions = new Map([
  ['crates/kk-runner/assets/lodash-3.10.1.js', '第三方固定版本源码，保留原文'],
]);
const paths = new Set(execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'], {
  cwd: root,
  encoding: 'utf8',
}).split('\0').filter(Boolean));

let checked = 0;
const oversized = [];
for (const path of [...paths].sort()) {
  if (!roots.some(prefix => path.startsWith(prefix)) || !extensions.has(extname(path))) continue;
  if (exceptions.has(path)) continue;
  let source;
  try {
    source = readFileSync(resolve(root, path), 'utf8');
  } catch (error) {
    // 已跟踪但在本次改动中删除的文件不属于当前源码集合。
    if (error.code === 'ENOENT') continue;
    throw error;
  }
  const lines = source === '' ? 0 : source.split('\n').length - Number(source.endsWith('\n'));
  checked++;
  if (lines > 1000) oversized.push({ path, lines });
}

if (oversized.length) {
  console.error('源码规模检查失败：以下文件超过 1000 行，请按职责拆分。');
  for (const { path, lines } of oversized) console.error(`  ${lines} 行：${path}`);
  process.exitCode = 1;
} else {
  console.log(`源码规模检查通过：${checked} 个主项目源码/测试文件均不超过 1000 行。`);
}
