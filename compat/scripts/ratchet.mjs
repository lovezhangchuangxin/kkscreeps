#!/usr/bin/env node
// 运行完整 screeps-ok 套件，并检查通过数下限、总用例数和逐用例回归。
// 基线通过的用例若失败、跳过或缺失，均计为回归；ratchet-allow.json 可显式豁免，
// 豁免不降低 ratchet.json 的总通过数要求。
//
// 基线固定取 HEAD 的父提交报告，避免代码与报告同次提交时与自身比较。
// 在 PR 合并提交上它通常是目标分支；普通 push 上只是上一提交，
// 不保证是上次推送或上次通过 CI 的状态。CI 至少需要两层提交历史。
// 本地运行：node compat/scripts/ratchet.mjs。

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { analyzeFlips, applyAllow } from './ratchet-flips.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const ratchetPath = resolve(root, 'compat/reports/ratchet.json');
const reportPath = resolve(root, 'compat/reports/kkscreeps.json');
const suiteLogPath = '/tmp/kk-run-suite.log';

// 先验证通过数下限；缺少 floor 时 passed < undefined 为 false，会绕过总量检查。
let ratchet;
try {
  ratchet = JSON.parse(readFileSync(ratchetPath, 'utf8'));
} catch (e) {
  console.error(
    `ratchet: 无法读取/解析 ${ratchetPath}: ${e?.message ?? e}——地板文件缺失即门禁失效，拒绝继续`,
  );
  process.exit(1);
}
if (!Number.isInteger(ratchet.floor) || ratchet.floor <= 0) {
  console.error(
    `ratchet: ratchet.json 形状非法——需 {floor: 正整数}，实得 ${JSON.stringify(ratchet)}`,
  );
  process.exit(1);
}
if (!Number.isInteger(ratchet.numTotalTests) || ratchet.numTotalTests <= 0) {
  console.error(
    `ratchet: ratchet.json 形状非法——需 {numTotalTests: 正整数}，实得 ${JSON.stringify(ratchet)}`,
  );
  process.exit(1);
}
// 豁免表是门禁契约的一部分：缺失/形状非法即红（空表 = 零豁免的正常态）
const allowPath = resolve(root, 'compat/reports/ratchet-allow.json');
let allowPatterns;
try {
  const allow = JSON.parse(readFileSync(allowPath, 'utf8'));
  if (
    !Array.isArray(allow.allow) ||
    !allow.allow.every((p) => typeof p === 'string' && p.length > 0)
  ) {
    throw new Error('需 {allow: 非空字符串[]}');
  }
  allowPatterns = allow.allow;
} catch (e) {
  console.error(`ratchet: 无法读取/解析 ${allowPath}: ${e?.message ?? e}——fail closed`);
  process.exit(1);
}

function tailSuiteLog(lines = 100) {
  // --quiet 将详细输出写入文件；报告缺失或损坏时附日志末尾，帮助定位套件启动失败。
  if (!existsSync(suiteLogPath)) return;
  const tail = readFileSync(suiteLogPath, 'utf8').split('\n').slice(-lines).join('\n');
  console.error(`--- 套件日志尾部（${suiteLogPath}，最后 ${lines} 行）---`);
  console.error(tail);
}

// 套件存在已知失败，因此不直接用非零退出码判断回归。
// 检查报告 mtime 是否变化，拒绝读取套件启动失败后留下的旧报告；
// 后续仍需校验用例总数和逐用例状态，mtime 变化本身不能证明全量运行完成。
const mtimeBefore = existsSync(reportPath) ? statSync(reportPath).mtimeMs : 0;
const run = spawnSync('bash', [resolve(root, 'compat/scripts/run-suite.sh'), '--quiet'], {
  stdio: 'inherit',
});
if (run.error || run.status === null) {
  console.error(`ratchet: 套件未能执行（${run.error ?? '无退出码'}）`);
  process.exit(1);
}
if (!existsSync(reportPath) || statSync(reportPath).mtimeMs === mtimeBefore) {
  console.error(
    'ratchet: 套件报告未更新（compat/reports/kkscreeps.json）——套件可能早崩，拒绝读取陈旧报告',
  );
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
console.log(
  `ratchet: passed ${passed} / failed ${failed} / skipped ${skipped}（floor ${ratchet.floor}）`,
);
let gateFailed = false;
if (total !== ratchet.numTotalTests) {
  console.error(
    `ratchet: 用例总数漂移 ${ratchet.numTotalTests} → ${total}（上游 pin 固定，理论不可能）——套件范围变化须显式归因，门禁红`,
  );
  gateFailed = true;
}
if (passed < ratchet.floor) {
  console.error(
    `ratchet: 回归——passed ${passed} < floor ${ratchet.floor}（差 ${ratchet.floor - passed}）。逐用例归因后修复，或（确属有意的行为变化）随全量重跑上调地板`,
  );
  gateFailed = true;
}
// 逐用例检查可识别一条变差、一条变好而总通过数不变的情况。
// 有意变化使用豁免表说明原因；豁免后仍打印命中的用例。
const flipsRed = printFlipsAgainstBaseline(report);
if (flipsRed < 0) {
  console.error('ratchet: 基线不可读 ⇒ 门禁红（fail closed）');
  gateFailed = true;
} else if (flipsRed > 0) {
  console.error(`ratchet: 相对 HEAD~1 基线有 ${flipsRed} 条未豁免转红 ⇒ 门禁红`);
  gateFailed = true;
}
if (gateFailed) {
  process.exit(1);
}
if (passed > ratchet.floor) {
  console.log(
    `ratchet: 地板可上调 ${ratchet.floor} → ${passed}（行为变化收尾时更新 ratchet.json 并同步 README/roadmap 数字）`,
  );
}

/** 从 Git 对象库读取 HEAD~1 的报告，比较本轮结果；工作区报告被覆盖不会改变基线。 */
function printFlipsAgainstBaseline(current) {
  // 完整 JSON 报告可能超过 spawnSync 默认的 1 MiB 输出缓冲，显式提高到 64 MiB。
  const shown = spawnSync('git', ['-C', root, 'show', 'HEAD~1:compat/reports/kkscreeps.json'], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  // 读取失败返回 -1，使调用方拒绝放行，而不是跳过逐用例检查。
  if (shown.status !== 0) {
    console.error(
      'ratchet: 无法读取 HEAD~1 基线（git show 失败——CI 需 fetch-depth ≥ 2）——逐用例回归不可验证',
    );
    return -1;
  }
  const { flipsRed, flipsGreen } = analyzeFlips(JSON.parse(shown.stdout), current);
  const { blocked, allowed, stale } = applyAllow(flipsRed, allowPatterns);
  console.error(
    `ratchet: 相对 HEAD~1 基线——转红 ${flipsRed.length} 条（豁免 ${allowed.length}、待归因 ${blocked.length}），转绿 ${flipsGreen} 条`,
  );
  for (const [name] of allowed.slice(0, 10)) {
    console.error(`  [豁免] ${name}`);
  }
  if (allowed.length > 10) console.error(`  …另有 ${allowed.length - 10} 条豁免`);
  for (const [name, cur] of blocked.slice(0, 10)) {
    const msg = (cur?.failureMessages?.[0] ?? '').split('\n')[0].slice(0, 200);
    console.error(`  [红] ${name}${msg ? `\n       ${msg}` : ''}`);
  }
  if (blocked.length > 10) console.error(`  …另有 ${blocked.length - 10} 条待归因转红`);
  for (const p of stale) {
    console.warn(`ratchet: 警告——豁免条目未命中任何转红（可能已转绿，应清理）: ${p}`);
  }
  return blocked.length;
}
