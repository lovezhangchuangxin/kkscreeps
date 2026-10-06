#!/usr/bin/env node
// 仓库检查门禁的唯一入口：pnpm run check（无参数=完整门禁），或
// pnpm run check -- kk-engine kk-runtime 限定 crate 只检查相关子集
// （开发中加速迭代；提交前须跑无参数完整门禁）。
// 步骤：格式检查、Clippy、Rust 测试、JS workspace 测试、文档链接、源码规模。
// Rust 测试用 --tests、不含 doctest——rustdoc 每次运行都重新编译链接全部
// doctest 二进制（本机实测占热缓存测试时间的 ~76%，147s/192s）。改动或
// 新增 doctest（文档注释里的 Rust 示例代码块）时，手动运行
// cargo test -p <crate> --doc 验证。
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// 门禁不能沿用调用方指定的旧二进制；CLI 构建与 JS 测试共享同一绝对路径。
export function runCheck(argv, { run = spawnSync, env = process.env, root = resolve(import.meta.dirname, '..') } = {}) {
  const args = argv.filter((a) => a !== '--');
  if (args.some((a) => !/^kk-[\w-]+$/.test(a))) {
    console.error(`check: 参数须是 crate 名（kk-…），得到 ${args.join(' ')}`);
    return 1;
  }
  const selection = args.length ? args.flatMap((c) => ['-p', c]) : ['--workspace'];
  const testEnv = { ...env, CARGO_TARGET_DIR: resolve(root, 'target'), KKSCREEPS_BIN: resolve(root, 'target/debug/kkscreeps') };

  const steps = [
    ['格式检查', ['cargo', 'fmt', '--all', '--check']],
    ['Clippy', ['cargo', 'clippy', ...selection, '--all-targets']],
    ['Rust 测试（不含 doctest）', ['cargo', 'test', ...selection, '--tests']],
    ['构建测试 CLI', ['cargo', 'build', '-p', 'kk-cli']],
    ['门禁回归', ['node', '--test', 'scripts/check.test.mjs']],
    ['JS workspace 测试', ['pnpm', 'run', '-r', 'test']],
    ['文档链接', ['node', 'scripts/check-doc-links.mjs']],
    ['源码规模', ['node', 'scripts/check-source-size.mjs']],
  ];

  for (const [label, command] of steps) {
    console.log(`\n== check · ${label} ==`);
    const result = run(command[0], command.slice(1), { stdio: 'inherit', cwd: root, env: testEnv });
    if (result.status !== 0) {
      console.error(`\ncheck: ${label} 失败（${result.status}）`);
      return result.status ?? 1;
    }
  }
  if (args.length) {
    console.log(`\ncheck 通过（限定子集：${args.join(' ')}——提交前须再跑无参数的完整 pnpm run check）`);
  } else {
    console.log('\ncheck 通过（完整门禁；doctest 不在其中，改动或新增 doctest 时手动跑 cargo test -p <crate> --doc）');
  }
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = runCheck(process.argv.slice(2));
}
