// 棘轮逐用例回归判定（compat/scripts/ratchet-flips.mjs）的单测：
// 对冲场景必须有测试锁死——门禁脚本自身不可 import（顶层副作用跑全量套件）。

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { analyzeFlips } from '../scripts/ratchet-flips.mjs';

const report = (pairs) => ({
  testResults: [
    {
      name: 'suite.test.js',
      assertionResults: pairs.map(([fullName, status]) => ({ fullName, status })),
    },
  ],
});

test('对冲不放行：passed→skipped 与 failed 同级计红', () => {
  // 一条转跳过 + 一条转通过：passed 总数与用例总数都不变——只认 failed 的
  // 门禁会放行，这是外部复审实测构造出的漏洞
  const baseline = report([
    ['A', 'passed'],
    ['B', 'failed'],
  ]);
  const current = report([
    ['A', 'skipped'],
    ['B', 'passed'],
  ]);
  const { flipsRed, flipsGreen } = analyzeFlips(baseline, current);
  assert.equal(flipsRed.length, 1, 'A 转跳过必须计红');
  assert.equal(flipsRed[0][0], 'A');
  assert.equal(flipsGreen, 1);
});

test('基线 passed 用例缺失计红；状态不变无红', () => {
  const baseline = report([
    ['A', 'passed'],
    ['B', 'passed'],
  ]);
  const missing = report([['A', 'passed']]);
  assert.equal(analyzeFlips(baseline, missing).flipsRed.length, 1, 'B 缺失计红');
  const same = report([
    ['A', 'passed'],
    ['B', 'passed'],
  ]);
  const r = analyzeFlips(baseline, same);
  assert.equal(r.flipsRed.length, 0);
  assert.equal(r.flipsGreen, 0);
});

test('转绿不设限：基线 failed→当前 passed 只计绿', () => {
  const baseline = report([['A', 'failed']]);
  const current = report([['A', 'passed']]);
  const { flipsRed, flipsGreen } = analyzeFlips(baseline, current);
  assert.equal(flipsRed.length, 0);
  assert.equal(flipsGreen, 1);
});
