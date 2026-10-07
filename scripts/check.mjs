#!/usr/bin/env node
// 仓库检查门禁的唯一入口：pnpm run check（无参数=完整门禁），或
// pnpm run check -- kk-engine kk-runtime 限定 crate 只检查相关子集
// （开发中加速迭代；提交前须跑无参数完整门禁）。
// 步骤：格式检查、Clippy、Rust 测试、JS workspace 测试、文档链接、源码规模。
// Rust 测试不含 doctest；改动文档注释中的 Rust 示例时另跑 cargo test --doc。
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// 门禁不能沿用调用方指定的旧二进制；CLI 构建与 JS 测试共享同一绝对路径。
export function runCheck(argv, { run = spawnSync, env = process.env, now = () => performance.now(), root = resolve(import.meta.dirname, '..') } = {}) {
  const args = [];
  let target;
  let filter;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--') continue;
    if (arg === '--test' || arg === '--filter') {
      const value = argv[++i];
      if (!value || value.startsWith('--') || (arg === '--test' ? target !== undefined : filter !== undefined)) {
        console.error(`check: ${arg} 须指定一个非空值，且不可重复`);
        return 1;
      }
      if (arg === '--test') target = value;
      else filter = value;
    } else if (/^kk-[\w-]+$/.test(arg)) {
      args.push(arg);
    } else {
      console.error(`check: 未知参数 ${arg}`);
      return 1;
    }
  }
  if ((target && (args.length !== 1 || !/^[\w-]+$/.test(target))) || (filter !== undefined && !target)) {
    console.error('check: --test 要求恰好一个 crate；--filter 必须与 --test 一起使用');
    return 1;
  }
  const selection = args.length ? args.flatMap((c) => ['-p', c]) : ['--workspace'];
  const tests = target ? ['--test', target, ...(filter !== undefined ? [filter] : [])] : ['--tests'];
  const testEnv = { ...env, CARGO_TARGET_DIR: resolve(root, 'target'), KKSCREEPS_BIN: resolve(root, 'target/debug/kkscreeps') };

  // 完整门禁不可继承开发时的场景过滤。
  delete testEnv.KKSCREEPS_SCENARIO;

  const steps = [
    ['格式检查', ['cargo', 'fmt', '--all', '--check']],
    ['Clippy', ['cargo', 'clippy', ...selection, '--all-targets']],
    ...(target ? [['Rust 目标发现', ['cargo', 'test', ...selection, ...tests, '--', '--list']]] : []),
    ['Rust 测试（不含 doctest）', ['cargo', 'test', ...selection, ...tests]],
    ['构建测试 CLI', ['cargo', 'build', '-p', 'kk-cli']],
    ['门禁回归', ['node', '--test', 'scripts/check.test.mjs', 'scripts/check-doc-links.test.mjs']],
    ['Python 工具测试', ['python3', '-m', 'unittest', 'discover', '-s', 'scripts/tests', '-p', 'test_*.py']],
    ['性能工具测试', ['python3', '-m', 'unittest', 'discover', '-s', 'scripts/benchmark', '-p', '*test*.py']],
    ['JS workspace 测试', ['pnpm', 'run', '-r', 'test']],
    ['文档链接', ['node', 'scripts/check-doc-links.mjs']],
    ['源码规模', ['node', 'scripts/check-source-size.mjs']],
  ];

  for (const [label, command] of steps) {
    console.log(`\n== check · ${label} ==`);
    const started = now();
    const discovery = label === 'Rust 目标发现';
    const result = run(command[0], command.slice(1), {
      stdio: discovery ? ['inherit', 'pipe', 'inherit'] : 'inherit',
      ...(discovery ? { encoding: 'utf8' } : {}), cwd: root, env: testEnv,
    });
    console.log(`check · ${label} · ${((now() - started) / 1000).toFixed(2)}s`);
    if (result.status !== 0) {
      console.error(`\ncheck: ${label} 失败（${result.status}）`);
      return result.status ?? 1;
    }
    if (discovery) {
      const matched = [...(result.stdout ?? '').matchAll(/^.+: test$/gm)].length;
      if (!matched) {
        console.error('check: 所选目标或过滤条件未匹配任何 Rust 测试，拒绝空验收');
        return 1;
      }
      console.log(`check · 已发现 ${matched} 个测试`);
    }
  }
  if (args.length) {
    console.log(`\ncheck 通过（限定子集：${args.join(' ')}${target ? ` / ${target}` : ''}${filter !== undefined ? ` / 过滤 ${filter}` : ''}——提交前须再跑无参数的完整 pnpm run check）`);
  } else {
    console.log('\ncheck 通过（完整门禁；doctest 不在其中，改动或新增 doctest 时手动跑 cargo test -p <crate> --doc）');
  }
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = runCheck(process.argv.slice(2));
}
