// 验证逐用例回归与豁免分类，尤其是总通过数不变但覆盖减少的情况。

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { analyzeFlips, applyAllow } from '../scripts/ratchet-flips.mjs';

const report = (pairs) => ({
  testResults: [
    {
      name: 'suite.test.js',
      assertionResults: pairs.map(([fullName, status]) => ({ fullName, status })),
    },
  ],
});

test('对冲不放行：passed→skipped 与 failed 同级计红', () => {
  // 一条转跳过、另一条转通过，总通过数与总用例数均不变；只检查这两个总数会漏报。
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

test('豁免表：精确/前缀命中分流，未命中计红', () => {
  const red = [
    ['suite > a', null],
    ['suite > b1', null],
    ['other > c', null],
  ];
  const { blocked, allowed, stale } = applyAllow(red, ['suite > a', 'suite > b*']);
  assert.deepEqual(
    allowed.map((e) => e[0]),
    ['suite > a', 'suite > b1'],
  );
  assert.deepEqual(
    blocked.map((e) => e[0]),
    ['other > c'],
  );
  assert.deepEqual(stale, []);
});

test('豁免表：陈旧条目（未命中任何转红）单列供警告', () => {
  const red = [['a', null]];
  const { blocked, stale } = applyAllow(red, ['a', 'gone > x']);
  assert.deepEqual(blocked, []);
  assert.deepEqual(stale, ['gone > x']);
});

test('豁免表：空表 = 零豁免（全部红转待归因）', () => {
  const red = [['a', null]];
  const { blocked, allowed } = applyAllow(red, []);
  assert.equal(blocked.length, 1);
  assert.equal(allowed.length, 0);
});
