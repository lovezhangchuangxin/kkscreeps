import { describe, expect, it } from 'vitest';
import { mergedOutlinePath } from '../src/core/path-merge.js';

// path 合并算法移植的忠实性单测。
// 期望串来源：官方 screeps/renderer pathHelper.js（stub md5 后原版直跑）
// 与本移植在 12 个用例上逐字节对照的输出（单格/横条/竖条/2×2/对角/L形/
// 十字/整行 × diagonalConnect 两态，2026-09-30 对照 12/12 一致）。
// 注意：N=4 下 (1,0) 不是边界格——端头圆角是官方行为。

const N = 4;
const zeros = (): number[] => new Array<number>(N * N).fill(0);
const set = (codes: number[], x: number, y: number, v = 1): void => {
  codes[y * N + x] = v;
};

describe('mergedOutlinePath（对照官方 pathHelper.js 的保真断言）', () => {
  it('单格：顶角贴网格边（方角），底角内格化（外凸圆弧）', () => {
    // 官方只在 x===0/y===0 抑制顶侧圆角；底侧圆角仅在 x/y===N-1 抑制——
    // N=4 下 (0,0) 的右下角按内格圆弧处理（对照官方 pathHelper 逐字节一致）
    const codes = zeros();
    set(codes, 0, 0);
    expect(mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }).path).toBe(
      'M 0 50 v -50 h 50 h 50 v 50 a 50 50 0 0 1 -50 50 h -50 v -50 Z ',
    );
  });

  it('孤立内格：四角外凸圆弧（非边界格端头圆角）', () => {
    const codes = zeros();
    set(codes, 1, 1);
    expect(mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }).path).toBe(
      'M 100 150 a 50 50 0 0 1 50 -50 a 50 50 0 0 1 50 50 ' +
        'a 50 50 0 0 1 -50 50 a 50 50 0 0 1 -50 -50 Z ',
    );
  });

  it('横 2 连格（y=0 行）：右端外凸圆角', () => {
    const codes = zeros();
    set(codes, 0, 0);
    set(codes, 1, 0);
    expect(mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }).path).toBe(
      'M 0 50 v -50 h 50 h 50 h 50 h 50 v 50 a 50 50 0 0 1 -50 50 h -50 h -50 h -50 v -50 Z ',
    );
  });

  it('竖 2 连格（x=0 列）', () => {
    const codes = zeros();
    set(codes, 0, 0);
    set(codes, 0, 1);
    expect(mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }).path).toBe(
      'M 0 50 v -50 h 50 h 50 v 50 v 50 v 50 a 50 50 0 0 1 -50 50 h -50 v -50 v -50 v -50 Z ',
    );
  });

  it('2×2 正方块：官方 quirk 保真——拆两个子路径（不合并成单轮廓）', () => {
    const codes = zeros();
    set(codes, 0, 0);
    set(codes, 1, 0);
    set(codes, 0, 1);
    set(codes, 1, 1);
    const r = mergedOutlinePath(codes, (c) => c === 1, { cell: 100 });
    expect(r.path).toBe(
      'M 0 50 v -50 h 50 h 50 v 50 v 50 v 50 v 50 h -50 h -50 v -50 v -50 v -50 Z ' +
        'M 100 50 v -50 h 50 h 50 v 50 v 50 v 50 a 50 50 0 0 1 -50 50 h -50 v -50 v -50 v -50 Z ',
    );
    expect(r.count).toBe(4);
    expect(r.path!.match(/M /g)).toHaveLength(2);
  });

  it('对角双格：diagonalConnect 改变凹角形态（两态路径均对照官方）', () => {
    const codes = zeros();
    set(codes, 0, 0);
    set(codes, 1, 1);
    const separated = mergedOutlinePath(codes, (c) => c === 1, { cell: 100 });
    const connected = mergedOutlinePath(codes, (c) => c === 1, {
      cell: 100,
      diagonalConnect: true,
    });
    expect(separated.path).toBe(
      'M 0 50 v -50 h 50 h 50 v 50 a 50 50 0 0 1 -50 50 h -50 v -50 Z ' +
        'M 100 150 a 50 50 0 0 1 50 -50 a 50 50 0 0 1 50 50 ' +
        'a 50 50 0 0 1 -50 50 a 50 50 0 0 1 -50 -50 Z ',
    );
    expect(connected.path).toBe(
      'M 0 50 v -50 h 50 h 50 v 50 v 50 h -50 h -50 v -50 Z ' +
        'M 100 150 a 50 50 0 0 0 -50 -50 h 50 v -50 a 50 50 0 0 0 50 50 ' +
        'a 50 50 0 0 1 50 50 a 50 50 0 0 1 -50 50 a 50 50 0 0 1 -50 -50 Z ',
    );
  });

  it('L 形 3 格（凹角拆两子路径——官方 quirk）', () => {
    const codes = zeros();
    set(codes, 1, 1);
    set(codes, 2, 1);
    set(codes, 1, 2);
    expect(mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }).path).toBe(
      'M 100 150 a 50 50 0 0 1 50 -50 h 50 v 50 v 50 h 50 a 50 50 0 0 0 -50 50 ' +
        'a 50 50 0 0 1 -50 50 a 50 50 0 0 1 -50 -50 v -50 v -50 Z ' +
        'M 200 150 v -50 h 50 a 50 50 0 0 1 50 50 a 50 50 0 0 1 -50 50 h -50 v -50 Z ',
    );
  });

  it('十字 5 格（recurs 双向深入，拆三子路径——官方 quirk）', () => {
    const codes = zeros();
    set(codes, 1, 1);
    set(codes, 0, 1);
    set(codes, 2, 1);
    set(codes, 1, 0);
    set(codes, 1, 2);
    expect(mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }).path).toBe(
      'M 0 150 v -50 h 50 a 50 50 0 0 0 50 -50 v 50 h 50 h 50 v -50 a 50 50 0 0 0 50 50 ' +
        'a 50 50 0 0 1 50 50 a 50 50 0 0 1 -50 50 h -50 h -100 h -50 h -50 v -50 Z ' +
        'M 100 50 v -50 h 50 h 50 v 50 v 50 h -100 v -50 Z ' +
        'M 100 250 a 50 50 0 0 0 -50 -50 h 50 h 50 h 50 h 50 a 50 50 0 0 0 -50 50 ' +
        'a 50 50 0 0 1 -50 50 a 50 50 0 0 1 -50 -50 Z ',
    );
  });

  it('整行 4 格（横向长条）', () => {
    const codes = zeros();
    for (let i = 0; i < 4; i++) set(codes, i, 2);
    expect(mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }).path).toBe(
      'M 0 250 v -50 h 50 h 50 h 50 h 50 h 50 h 50 h 50 h 50 v 50 v 50 ' +
        'h -50 h -50 h -100 h -100 h -50 h -50 v -50 Z ',
    );
  });

  it('门控：同输入 + 同 prevHash → path null；异输入 → 重建且 hash 变', () => {
    const codes = zeros();
    set(codes, 2, 1);
    const first = mergedOutlinePath(codes, (c) => c === 1, { cell: 100 });
    expect(first.path).not.toBeNull();
    const cached = mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }, first.hash);
    expect(cached.path).toBeNull();
    expect(cached.hash).toBe(first.hash);
    set(codes, 3, 1);
    const rebuilt = mergedOutlinePath(codes, (c) => c === 1, { cell: 100 }, first.hash);
    expect(rebuilt.path).not.toBeNull();
    expect(rebuilt.hash).not.toBe(first.hash);
  });

  it('空集：path null；非法长度抛错；确定性（两次输出逐字节一致）', () => {
    const r = mergedOutlinePath(zeros(), (c) => c === 1, { cell: 100 });
    expect(r.path).toBeNull();
    expect(r.count).toBe(0);
    expect(() => mergedOutlinePath([0, 1, 2], (c) => c === 1, { cell: 100 })).toThrow('平方数');

    const codes = zeros();
    set(codes, 0, 0, 1);
    set(codes, 1, 1, 2);
    set(codes, 2, 2, 2);
    const a = mergedOutlinePath(codes, (c) => c > 0, { cell: 100 });
    const b = mergedOutlinePath(codes, (c) => c > 0, { cell: 100 });
    expect(a).toEqual(b);
  });
});
