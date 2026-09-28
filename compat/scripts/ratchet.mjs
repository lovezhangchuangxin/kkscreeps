#!/usr/bin/env node
// screeps-ok 通过率棘轮（方案 D6 / M1 步 7）：
//   全量跑套件（run-suite.sh --quiet，约 3 分钟）→ 解析 vitest JSON 报告 →
//   passed < floor ⇒ exit 1（打印差值与最近转红用例）；passed > floor ⇒ exit 0 并
//   提示上调地板；用例总数漂移（上游 pin 固定，理论不可能）打印警告。
// 地板唯一真相源：compat/reports/ratchet.json。本地等价：node compat/scripts/ratchet.mjs
// （macOS 无 timeout 命令——本脚本与其调用的 run-suite.sh 都不用它）。

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const ratchetPath = resolve(root, 'compat/reports/ratchet.json');
const reportPath = resolve(root, 'compat/reports/kkscreeps.json');
const suiteLogPath = '/tmp/kk-run-suite.log';

// 形状校验先行：floor 缺失/非正整数时 `passed < undefined` 恒 false——门禁会
// 对任何回归静默放行（手工编辑键名错、合并冲突解出合法 JSON 都会走到这里），
// 必须拒绝运行而非空转
let ratchet;
try {
  ratchet = JSON.parse(readFileSync(ratchetPath, 'utf8'));
} catch (e) {
  console.error(`ratchet: 无法读取/解析 ${ratchetPath}: ${e?.message ?? e}——地板文件缺失即门禁失效，拒绝继续`);
  process.exit(1);
}
if (!Number.isInteger(ratchet.floor) || ratchet.floor <= 0) {
  console.error(`ratchet: ratchet.json 形状非法——需 {floor: 正整数}，实得 ${JSON.stringify(ratchet)}`);
  process.exit(1);
}
if (!Number.isInteger(ratchet.numTotalTests) || ratchet.numTotalTests <= 0) {
  console.error(`ratchet: ratchet.json 形状非法——需 {numTotalTests: 正整数}，实得 ${JSON.stringify(ratchet)}`);
  process.exit(1);
}

function tailSuiteLog(lines = 100) {
  // 套件 --quiet 把 vitest 输出重定向到 /tmp——早崩/解析失败路径上必须把它带回
  // CI log，否则红得毫无诊断信息（多为 CI 特有故障、本地不可复现）
  if (!existsSync(suiteLogPath)) return;
  const tail = readFileSync(suiteLogPath, 'utf8').split('\n').slice(-lines).join('\n');
  console.error(`--- 套件日志尾部（${suiteLogPath}，最后 ${lines} 行）---`);
  console.error(tail);
}

/** 报告 → fullName 到 status 的映射（vitest json reporter 形状）。 */
function statusesOf(report) {
  const out = new Map();
  for (const tr of report.testResults ?? []) {
    for (const a of tr.assertionResults ?? tr.assertions ?? []) {
      out.set(a.fullName ?? a.title ?? '', a);
    }
  }
  return out;
}

// 新鲜度守卫：套件必须真跑过（vitest 有失败用例时 exit 1 属常态——预期失败池
// 数以千计，套件退出码不作判定；但套件早崩会留下上一轮的陈旧报告，靠 mtime 拦截）
const mtimeBefore = existsSync(reportPath) ? statSync(reportPath).mtimeMs : 0;
const run = spawnSync(
  'bash',
  [resolve(root, 'compat/scripts/run-suite.sh'), '--quiet'],
  { stdio: 'inherit' },
);
if (run.error || run.status === null) {
  console.error(`ratchet: 套件未能执行（${run.error ?? '无退出码'}）`);
  process.exit(1);
}
if (!existsSync(reportPath) || statSync(reportPath).mtimeMs === mtimeBefore) {
  console.error('ratchet: 套件报告未更新（compat/reports/kkscreeps.json）——套件可能早崩，拒绝读取陈旧报告');
  tailSuiteLog();
  process.exit(1);
}
let report;
try {
  report = JSON.parse(readFileSync(reportPath, 'utf8'));
} catch (e) {
  console.error(`ratchet: 报告 JSON 解析失败: ${e?.message ?? e}`);
  tailSuiteLog();
  process.exit(1);
}

let passed = 0;
let failed = 0;
let skipped = 0;
for (const tr of report.testResults ?? []) {
  for (const a of tr.assertionResults ?? tr.assertions ?? []) {
    if (a.status === 'passed') passed += 1;
    else if (a.status === 'failed') failed += 1;
    else skipped += 1;
  }
}
const total = passed + failed + skipped;
console.log(`ratchet: passed ${passed} / failed ${failed} / skipped ${skipped}（floor ${ratchet.floor}）`);
let gateFailed = false;
if (total !== ratchet.numTotalTests) {
  console.error(`ratchet: 用例总数漂移 ${ratchet.numTotalTests} → ${total}（上游 pin 固定，理论不可能）——套件范围变化须显式归因，门禁红`);
  gateFailed = true;
}
if (passed < ratchet.floor) {
  console.error(`ratchet: 回归——passed ${passed} < floor ${ratchet.floor}（差 ${ratchet.floor - passed}）。逐用例归因后修复，或（确属有意的行为变化）随全量重跑上调地板`);
  gateFailed = true;
}
// 逐用例回归：总数口径对冲放不过门禁（一红一绿 passed 不变、少跑用例总数降）——
// 相对上一入库基线（HEAD 的 kkscreeps.json）任何转红即红；有意行为变化须随全量
// 重跑更新基线入库并在提交说明归因，疑似偶发先重跑 job 复核（flake 策略）
const flipsRed = printFlipsAgainstBaseline(report);
if (flipsRed > 0) {
  console.error(`ratchet: 相对上一入库基线转红 ${flipsRed} 条 ⇒ 门禁红`);
  gateFailed = true;
}
if (gateFailed) {
  process.exit(1);
}
if (passed > ratchet.floor) {
  console.log(`ratchet: 地板可上调 ${ratchet.floor} → ${passed}（行为变化收尾时更新 ratchet.json 并同步 README/roadmap 数字）`);
}

/** 与上一入库基线逐用例 diff（git show 读对象库——工作区报告已被本轮覆盖不受影响；
 *  浅克隆（actions/checkout 默认 fetch-depth 1）下 HEAD 的 blob 完整可用）。 */
function printFlipsAgainstBaseline(current) {
  // 报告约 1.7MB，超过 spawnSync 默认 1MB maxBuffer（ENOBUFS）——显式放大
  const shown = spawnSync('git', ['-C', root, 'show', 'HEAD:compat/reports/kkscreeps.json'], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  if (shown.status !== 0) {
    console.error('ratchet: 无法读取上一入库基线（git show 失败），跳过逐用例 diff');
    return;
  }
  const baseline = statusesOf(JSON.parse(shown.stdout));
  const now = statusesOf(current);
  const flipsRed = [];
  let flipsGreen = 0;
  for (const [name, a] of baseline) {
    if (a.status !== 'passed') continue;
    const cur = now.get(name);
    if (!cur || cur.status === 'failed') flipsRed.push([name, cur]);
    else if (cur.status === 'passed') continue;
  }
  for (const [name, a] of now) {
    if (a.status === 'passed' && baseline.get(name)?.status !== 'passed') flipsGreen += 1;
  }
  console.error(`ratchet: 相对上一基线——转红 ${flipsRed.length} 条，转绿 ${flipsGreen} 条`);
  for (const [name, cur] of flipsRed.slice(0, 10)) {
    const msg = (cur?.failureMessages?.[0] ?? '').split('\n')[0].slice(0, 200);
    console.error(`  [红] ${name}${msg ? `\n       ${msg}` : ''}`);
  }
  if (flipsRed.length > 10) console.error(`  …另有 ${flipsRed.length - 10} 条转红`);
  return flipsRed.length;
}
