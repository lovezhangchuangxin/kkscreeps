#!/usr/bin/env node
// 开发中程快车道：与 pnpm run check 相同的步骤，但 Rust 测试只跑
// 单元/集成（--tests），跳过 doctest——rustdoc 每次运行都重新编译
// 链接全部 doctest 二进制（本机实测占热缓存测试时间的 ~76%，147s/192s），
// 而单元/集成测试由 cargo 正常缓存。完整门禁（提交前）仍走 pnpm run
// check。可选参数限定 crate：pnpm run check:fast -- kk-engine kk-runtime
import { spawnSync } from 'node:child_process';

const args = process.argv.slice(2).filter((a) => a !== '--');
const unknown = args.filter((a) => !a.startsWith('-'));
if (unknown.some((a) => !/^kk-[\w-]+$/.test(a))) {
  console.error(`check-fast: 参数须是 crate 名（kk-…），得到 ${unknown.join(' ')}`);
  process.exit(1);
}
const selection = args.length ? args.flatMap((c) => ['-p', c]) : ['--workspace'];

const steps = [
  ['格式检查', ['cargo', 'fmt', '--all', '--check']],
  ['Clippy', ['cargo', 'clippy', ...selection, '--all-targets']],
  ['Rust 测试（无 doctest）', ['cargo', 'test', ...selection, '--tests']],
  ['JS workspace 测试', ['pnpm', 'run', '-r', 'test']],
  ['文档链接', ['node', 'scripts/check-doc-links.mjs']],
  ['源码规模', ['node', 'scripts/check-source-size.mjs']],
];

for (const [label, command] of steps) {
  console.log(`\n== check:fast · ${label} ==`);
  const result = spawnSync(command[0], command.slice(1), { stdio: 'inherit' });
  if (result.status !== 0) {
    console.error(`\ncheck-fast: ${label} 失败（${result.status}）`);
    process.exit(result.status ?? 1);
  }
}
console.log('\ncheck:fast 通过（doctest 未跑——提交前须跑完整 pnpm run check）');
