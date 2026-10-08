#!/usr/bin/env node
// 汇集批次验收证据为机器可读 JSON 片段：棘轮计数（compat/reports/
// kkscreeps.json + ratchet.json 地板）、bot 报告、sim 场景报告与 Rust
// 测试计数（经参数传入）。输出直接可用作 research note 的 checks/
// ratchet/bots/sims 段，替代手工同步——数字与报告产物必然一致。
// 用法示例：
//   node scripts/collect-batch-evidence.mjs \
//     --bots compat/reports/bot-tooangel.json /tmp/bot-overmind.json \
//     --sims /tmp/sim-combat-json.json /tmp/sim-combat-kkpv.json \
//     --rust-passed 499 --rust-ignored 5 --out /tmp/evidence.json
import { readFileSync, writeFileSync } from 'node:fs';

function fail(message) {
  console.error(`collect-batch-evidence: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  const out = { bots: [], sims: [], rust: {}, ratchet: 'compat/reports/kkscreeps.json' };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const next = () => {
      if (++i >= argv.length) fail(`参数 ${arg} 缺少值`);
      return argv[i];
    };
    if (arg === '--bots') {
      while (!argv[i + 1]?.startsWith('--') && argv[i + 1] !== undefined) out.bots.push(next());
    } else if (arg === '--sims') {
      while (!argv[i + 1]?.startsWith('--') && argv[i + 1] !== undefined) out.sims.push(next());
    } else if (arg === '--ratchet') out.ratchet = next();
    else if (arg === '--out') out.out = next();
    else if (arg === '--rust-passed') out.rust.passed = Number(next());
    else if (arg === '--rust-failed') out.rust.failed = Number(next());
    else if (arg === '--rust-ignored') out.rust.ignored = Number(next());
    else fail(`未知参数 ${arg}`);
  }
  return out;
}

function readJson(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    return fail(`读取 ${path} 失败：${error.message}`);
  }
}

const args = parseArgs(process.argv.slice(2));

// 棘轮：机器报告的 jest 形态字段 + 地板配置；总数漂移由 ratchet.mjs
// 本身把门，这里只汇总，不重复门禁。
const report = readJson(args.ratchet);
const ratchet = {
  total: report.numTotalTests,
  passed: report.numPassedTests,
  failed: report.numFailedTests,
  skipped: report.numPendingTests,
};
const floor = readJson('compat/reports/ratchet.json').floor;
if (typeof floor === 'number') ratchet.floor = floor;
for (const key of ['total', 'passed', 'failed', 'skipped']) {
  if (!Number.isFinite(ratchet[key])) fail(`棘轮报告缺少字段 ${key}`);
}
if (ratchet.passed < ratchet.floor) {
  fail(`通过数 ${ratchet.passed} 低于地板 ${ratchet.floor}——不要收集证据，先修回归`);
}

// bot 报告：文件名即夹具名；只搬运固定字段，缺失即失败。
const bots = args.bots.map((path) => {
  const raw = readJson(path);
  const name = path
    .split('/')
    .pop()
    .replace(/^bot-|\.json$/g, '');
  const entry = { name };
  for (const key of [
    'ticks_requested',
    'ticks_completed',
    'ticks_terminated',
    'ticks_skipped_penalty',
    'ticks_with_console_error',
  ]) {
    if (!Number.isFinite(raw[key])) fail(`bot 报告 ${path} 缺少 ${key}`);
    entry[key] = raw[key];
  }
  return entry;
});

// sim 场景报告：断言计数、通道与终态；双通道一致性由成对报告推断。
const sims = args.sims.map((path) => {
  const raw = readJson(path);
  const assertions = raw.assertions;
  if (!Array.isArray(assertions)) fail(`sim 报告 ${path} 缺少 assertions 数组`);
  return {
    channel: raw.channel ?? fail(`sim 报告 ${path} 缺少 channel`),
    scenario: raw.scenario,
    ticks: raw.world_ticks_committed,
    assertions_total: assertions.length,
    assertions_passed: assertions.filter((a) => a.passed).length,
    executable_sha256: raw.executable_sha256,
  };
});
const shas = new Set(sims.map((s) => s.executable_sha256));
if (shas.size > 1) fail(`sim 报告二进制摘要不一致：${[...shas].join(' vs ')}`);

const evidence = {
  collected_at: new Date().toISOString(),
  executable_sha256: sims[0]?.executable_sha256 ?? null,
  ratchet,
  bots,
  sims,
};
if (Number.isFinite(args.rust.passed)) {
  evidence.rust = {
    passed: args.rust.passed,
    failed: args.rust.failed ?? 0,
    ignored: args.rust.ignored ?? 0,
  };
}

const text = JSON.stringify(evidence, null, 2) + '\n';
if (args.out) {
  writeFileSync(args.out, text);
  console.log(`写入 ${args.out}`);
} else {
  process.stdout.write(text);
}
