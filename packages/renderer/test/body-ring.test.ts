import { describe, expect, it } from 'vitest';
import { bodyRingArcs, hasLivingTough } from '../src/objects/body-ring.js';
import { CREEP_SPEC } from '../src/objects/specs.js';

// body 环算法：creepBuildBody.js:84-157 的逐项断言（常量/过滤/合并/升序/
// 双向展开/弧长）。

const R = CREEP_SPEC.ring;
const PA = R.partAngle; // 每 100 hits 弧长 = 2π/50/2

function part(type: string, hits: number): { type: string; hits: number } {
  return { type, hits };
}

describe('CREEP_SPEC.ring 常量（creepBuildBody.js:7-12）', () => {
  it('官方常量逐值', () => {
    expect(R.maxParts).toBe(50);
    expect(R.angleShift).toBe(-Math.PI / 2);
    expect(R.partAngle).toBeCloseTo((2 * Math.PI) / 50 / 2);
    expect(R.maxPartHits).toBe(100);
    expect(R.radius).toBe(50);
    expect(R.lineWidth).toBe(18);
  });

  it('色表（:13-43）', () => {
    expect(R.colors.move).toBe(0xaab7c5);
    expect(R.colors.work).toBe(0xfde574);
    expect(R.colors.carry).toBe(0x666666); // 色值存在但被过滤不画
    expect(R.colors.attack).toBe(0xf72e41);
    expect(R.colors.ranged_attack).toBe(0x7fa7e5);
    expect(R.colors.heal).toBe(0x56cf5e);
    expect(R.colors.claim).toBe(0xb99cfb);
  });
});

describe('bodyRingArcs', () => {
  it('空 body → 无弧', () => {
    expect(bodyRingArcs([])).toEqual([]);
    expect(bodyRingArcs(undefined)).toEqual([]);
  });

  it('过滤 tough/carry/hits≤0（:108-109）', () => {
    const arcs = bodyRingArcs([part('tough', 100), part('carry', 100), part('work', 100), part('move', 0), part('work', -10)]);
    // work 100 → 一对镜像弧
    expect(arcs).toHaveLength(2);
    expect(arcs[0]!.color).toBe(R.colors.work);
  });

  it('同类型合并 hits（:111-119）', () => {
    const arcs = bodyRingArcs([part('work', 100), part('work', 100), part('work', 100)]);
    const sweep = arcs[0]!.endAngle - arcs[0]!.startAngle;
    expect(sweep).toBeCloseTo(PA * 3);
  });

  it('合并后按 hits 升序（:121-129；order 字段死代码）', () => {
    // 声明序 attack(200) 在前 work(100) 在后 → 排序后 work 先画
    const arcs = bodyRingArcs([part('attack', 200), part('work', 100)]);
    expect(arcs[0]!.color).toBe(R.colors.work);
    expect(arcs[2]!.color).toBe(R.colors.attack);
    // work 弧自头顶起（非 move：frontAngle=0）
    expect(arcs[0]!.startAngle).toBeCloseTo(-Math.PI / 2);
    expect(arcs[0]!.anticlockwise).toBe(false);
  });

  it('非 move 自头顶向两侧对称展开（:131-143 frontAngle=0）', () => {
    const arcs = bodyRingArcs([part('heal', 100)]);
    expect(arcs).toHaveLength(2);
    // 正向弧：ANGLE_SHIFT+0 → +sweep（顺时针）
    expect(arcs[0]!.startAngle).toBeCloseTo(-Math.PI / 2);
    expect(arcs[0]!.endAngle).toBeCloseTo(-Math.PI / 2 + PA);
    expect(arcs[0]!.anticlockwise).toBe(false);
    // 镜像弧：ANGLE_SHIFT−0 → −sweep（逆时针）
    expect(arcs[1]!.startAngle).toBeCloseTo(-Math.PI / 2);
    expect(arcs[1]!.endAngle).toBeCloseTo(-Math.PI / 2 - PA);
    expect(arcs[1]!.anticlockwise).toBe(true);
  });

  it('move（backSide）自正下对称展开（:131-152 backAngle=π）', () => {
    const arcs = bodyRingArcs([part('move', 100)]);
    expect(arcs[0]!.startAngle).toBeCloseTo(Math.PI / 2); // ANGLE_SHIFT+π
    // 镜像弧 ANGLE_SHIFT−π = −3π/2（官方不归一角度——与 π/2 几何同相位）
    expect(arcs[1]!.startAngle).toBeCloseTo(-Math.PI / 2 - Math.PI);
  });

  it('弧长 = PART_ANGLE × hits/100（:139）', () => {
    const arcs = bodyRingArcs([part('work', 37)]);
    expect(arcs[0]!.endAngle - arcs[0]!.startAngle).toBeCloseTo(PA * 0.37);
  });

  it('50 work 部件铺满全环（两侧各 π）', () => {
    const arcs = bodyRingArcs(Array.from({ length: 50 }, () => part('work', 100)));
    expect(arcs).toHaveLength(2);
    expect(arcs[0]!.endAngle - arcs[0]!.startAngle).toBeCloseTo(Math.PI);
    expect(arcs[1]!.startAngle - arcs[1]!.endAngle).toBeCloseTo(Math.PI);
  });

  it('未知类型跳过（官方 warn 后无色表不画）', () => {
    expect(bodyRingArcs([part('boost', 100), part('work', 50)])).toHaveLength(2);
  });

  it('混合 body（move 在后展开）——bootstrapper 形态', () => {
    // work100+carry+move200：carry 过滤；work 前 100、move 后 200
    const arcs = bodyRingArcs([part('work', 100), part('carry', 100), part('move', 200)]);
    expect(arcs).toHaveLength(4);
    // 升序：work(100) 先
    expect(arcs[0]!.color).toBe(R.colors.work);
    expect(arcs[2]!.color).toBe(R.colors.move);
    // move 自正下（π）：正向弧起 −π/2+π
    expect(arcs[2]!.startAngle).toBeCloseTo(-Math.PI / 2 + Math.PI);
    expect(arcs[2]!.endAngle - arcs[2]!.startAngle).toBeCloseTo(PA * 2);
  });
});

describe('hasLivingTough（creepBuildBody.js:154-156）', () => {
  it('任一 tough hits>0 → true', () => {
    expect(hasLivingTough([part('tough', 1), part('work', 100)])).toBe(true);
  });
  it('tough 全灭/无 tough → false', () => {
    expect(hasLivingTough([part('tough', 0)])).toBe(false);
    expect(hasLivingTough([part('work', 100)])).toBe(false);
    expect(hasLivingTough(undefined)).toBe(false);
  });
});
