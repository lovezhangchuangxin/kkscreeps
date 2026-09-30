import { describe, expect, it } from 'vitest';
import type { VisualStyle } from '@kkscreeps/shared';
import {
  circlePoints,
  DASH_PATTERN,
  dashSegments,
  parseColor,
  resolveCircle,
  resolveLine,
  resolvePoly,
  resolveRect,
  resolveText,
} from '../src/scenes/visual-style.js';

// 官方默认值逐值断言（docs.screeps.com/api/#RoomVisual 2026-09-30 取证；
// engine 只透传 style，默认值渲染端应用=我方同位——G1 黑盒契约锚）。

describe('parseColor', () => {
  it('#rgb/#rrggbb/#rrggbbaa/transparent，其余形态 null（G1 退默认）', () => {
    expect(parseColor('#fff')).toEqual({ color: 0xffffff, alpha: 1 });
    expect(parseColor('#f00')).toEqual({ color: 0xff0000, alpha: 1 });
    expect(parseColor('#12abEF')).toEqual({ color: 0x12abef, alpha: 1 });
    expect(parseColor('#ff000080')).toEqual({ color: 0xff0000, alpha: 128 / 255 });
    expect(parseColor('transparent')).toEqual({ color: 0, alpha: 0 });
    expect(parseColor('red')).toBeNull();
    expect(parseColor('rgba(1,2,3,4)')).toBeNull();
    expect(parseColor('#12345')).toBeNull();
    expect(parseColor('')).toBeNull();
  });
});

describe('resolveLine（width 0.1 / color #ffffff / opacity 0.5）', () => {
  const cell = 100;
  it('全缺省=官方默认值', () => {
    const r = resolveLine(undefined, cell);
    expect(r).toEqual({ color: { color: 0xffffff, alpha: 1 }, widthPx: 10, opacity: 0.5, dash: 'solid' });
  });
  it('显式样式覆盖 + dash 透传', () => {
    const style: VisualStyle = { width: 0.2, color: '#00ff00', opacity: 1, lineStyle: 'dashed' };
    const r = resolveLine(style, cell);
    expect(r.color.color).toBe(0x00ff00);
    expect(r.widthPx).toBe(20);
    expect(r.opacity).toBe(1);
    expect(r.dash).toBe('dashed');
  });
  it('脏数据防御：非有限/负数退默认，opacity 钳 [0,1]', () => {
    expect(resolveLine({ width: -3 } as VisualStyle, cell).widthPx).toBe(10);
    expect(resolveLine({ width: Number.NaN } as VisualStyle, cell).widthPx).toBe(10);
    expect(resolveLine({ opacity: 7 } as VisualStyle, cell).opacity).toBe(1);
    expect(resolveLine({ opacity: -1 } as VisualStyle, cell).opacity).toBe(0);
  });
});

describe('resolveRect（fill #ffffff / stroke 无 / strokeWidth 0.5）', () => {
  const cell = 100;
  it('全缺省=填充无描边（与 poly 不对称）——全形断言', () => {
    expect(resolveRect(undefined, cell)).toEqual({
      fill: { color: 0xffffff, alpha: 1 },
      stroke: null,
      strokePx: 50,
      opacity: 0.5,
      dash: 'solid',
    });
  });
  it('transparent 填充 → α0；显式描边生效', () => {
    const r = resolveRect({ fill: 'transparent', stroke: '#105010', strokeWidth: 0.25 } as VisualStyle, cell);
    expect(r.fill!.alpha).toBe(0);
    expect(r.stroke).toEqual({ color: 0x105010, alpha: 1 });
    expect(r.strokePx).toBe(25);
  });
  it('不支持的颜色形态：默认有色字段退默认色；默认无层字段保持无层（评审 A-J1）', () => {
    expect(resolveRect({ fill: 'blue' } as VisualStyle, cell).fill).toEqual({ color: 0xffffff, alpha: 1 });
    expect(resolveRect({ stroke: 'blue' } as VisualStyle, cell).stroke).toBeNull();
  });
});

describe('resolveCircle（radius 10 / fill #ffffff / stroke 无）', () => {
  it('全缺省=官方默认值——全形断言', () => {
    expect(resolveCircle(undefined, 100)).toEqual({
      fill: { color: 0xffffff, alpha: 1 },
      stroke: null,
      strokePx: 50,
      opacity: 0.5,
      dash: 'solid',
      radiusPx: 1000,
    });
  });
  it('负半径退默认', () => {
    expect(resolveCircle({ radius: -1 } as VisualStyle, 100).radiusPx).toBe(1000);
  });
});

describe('resolvePoly（fill 无 / stroke #ffffff / strokeWidth 0.5）', () => {
  it('全缺省=描边无填充（与 rect/circle 不对称）——全形断言', () => {
    expect(resolvePoly(undefined, 100)).toEqual({
      fill: null,
      stroke: { color: 0xffffff, alpha: 1 },
      strokePx: 50,
      opacity: 0.5,
      dash: 'solid',
    });
  });
  it('默认无层字段收到不支持色串保持无层（评审 A-J1）', () => {
    expect(resolvePoly({ fill: 'blue' } as VisualStyle, 100).fill).toBeNull();
  });
});

describe('resolveText（sans-serif / fontSize 10 / strokeWidth 0.15 / padding 2 / align center）', () => {
  const cell = 100;
  it('全缺省=官方默认值', () => {
    const r = resolveText(undefined, cell);
    expect(r).toEqual({
      color: { color: 0xffffff, alpha: 1 },
      opacity: 0.5,
      fontSizePx: 1000,
      fontFamily: 'sans-serif',
      fontStyle: 'normal',
      fontVariant: 'normal',
      stroke: null,
      strokePx: 15,
      background: null,
      paddingPx: 200,
      align: 'center',
    });
  });
  it('合法枚举透传（评审 A-J2：italic/oblique/small-caps/left/right）', () => {
    const r = resolveText({
      fontStyle: 'italic',
      fontVariant: 'small-caps',
      align: 'right',
      fontFamily: 'Roboto',
    }, cell);
    expect(r.fontStyle).toBe('italic');
    expect(r.fontVariant).toBe('small-caps');
    expect(r.align).toBe('right');
    expect(r.fontFamily).toBe('Roboto');
    expect(resolveText({ fontStyle: 'oblique', align: 'left' } as VisualStyle, cell).fontStyle).toBe('oblique');
    expect(resolveText({ align: 'left' } as VisualStyle, cell).align).toBe('left');
  });
  it('字段子集校验：非法枚举退默认；默认无层字段不支持色串保持无层（A-J1）', () => {
    const r = resolveText({
      fontStyle: 'bold' as VisualStyle['fontStyle'],
      fontVariant: 'caps' as VisualStyle['fontVariant'],
      align: 'up' as VisualStyle['align'],
      fontFamily: 'Roboto',
    }, cell);
    expect(r.fontStyle).toBe('normal');
    expect(r.fontVariant).toBe('normal');
    expect(r.align).toBe('center');
    expect(r.fontFamily).toBe('Roboto');
    const dirty = resolveText({ stroke: 'blue', backgroundColor: 'blue' } as VisualStyle, cell);
    expect(dirty.stroke).toBeNull();
    expect(dirty.background).toBeNull();
  });
});

describe('dashSegments（节奏 G1 登记自定：dashed 0.5/0.25 · dotted 0.05/0.30）', () => {
  it('直线切段数与段长符合节奏', () => {
    const cell = 100; // 1 格=100px；dashed on=50px off=25px
    const segs = dashSegments([{ x: 0, y: 0 }, { x: 2000, y: 0 }], 'dashed', cell);
    // 2000px = 26 段节奏（50+25）×26=1950 + 尾 50 → 27 段 on
    expect(segs.length).toBe(27);
    expect(segs[0]![0]).toEqual({ x: 0, y: 0 });
    expect(segs[0]![1]!.x - segs[0]![0]!.x).toBeCloseTo(DASH_PATTERN.dashed.on * cell);
    expect(segs.every(([a, b]) => Math.abs(b!.x - a!.x - DASH_PATTERN.dashed.on * cell) < 1e-9)).toBe(true);
  });
  it('短于一个 on 段的线整段画出', () => {
    const segs = dashSegments([{ x: 0, y: 0 }, { x: 30, y: 0 }], 'dashed', 100);
    expect(segs).toEqual([[{ x: 0, y: 0 }, { x: 30, y: 0 }]]);
  });
  it('跨拐点连续行走（L 形累计长度）', () => {
    const cell = 100;
    const pts = [{ x: 0, y: 0 }, { x: 600, y: 0 }, { x: 600, y: 600 }]; // 600+600=1200
    const segs = dashSegments(pts, 'dashed', cell);
    const total = segs.reduce((sum, [a, b]) => sum + Math.hypot(b!.x - a!.x, b!.y - a!.y), 0);
    // 1200px = 16 整节奏（on50+off25）→ on 段总长 16×50=800，off 恰好收尾
    expect(total).toBeCloseTo(800, 6);
    expect(segs.length).toBe(16);
  });
  it('dotted 短 on 段（round 端帽成点）', () => {
    const cell = 100;
    const segs = dashSegments([{ x: 0, y: 0 }, { x: 1000, y: 0 }], 'dotted', cell);
    expect(segs[0]![1]!.x - segs[0]![0]!.x).toBeCloseTo(DASH_PATTERN.dotted.on * cell);
  });
  it('间隙宽度感知（评审 A-B1）：默认宽 0.5 下 off ≥ width+0.1 格，点/划离散', () => {
    const cell = 100;
    const w = 0.5 * cell; // rect/circle/poly 默认 strokeWidth
    const dotted = dashSegments([{ x: 0, y: 0 }, { x: 4000, y: 0 }], 'dotted', cell, w);
    expect(dotted[1]![0]!.x - dotted[0]![1]!.x).toBe(w + 0.1 * cell); // 间隙 60px > 0
    const dashed = dashSegments([{ x: 0, y: 0 }, { x: 4000, y: 0 }], 'dashed', cell, w);
    expect(dashed[1]![0]!.x - dashed[0]![1]!.x).toBe(w + 0.1 * cell); // 60px（原 25px 会粘连）
    // 细线（line 默认 0.1）沿用文档节奏：off = max(offCell, width+0.1格) = offCell
    const thin = dashSegments([{ x: 0, y: 0 }, { x: 4000, y: 0 }], 'dashed', cell, 0.1 * cell);
    expect(thin[1]![0]!.x - thin[0]![1]!.x).toBe(DASH_PATTERN.dashed.off * cell);
  });
  it('circlePoints 首尾闭合、半径正确', () => {
    const pts = circlePoints(100, 200, 50);
    expect(pts[0]).toEqual({ x: 150, y: 200 });
    expect(pts[pts.length - 1]).toEqual({ x: 150, y: 200 });
    expect(pts[16]).toEqual({ x: 100, y: 250 });
  });
});
