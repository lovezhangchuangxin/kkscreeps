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

// 官方默认值逐值断言（权威源 screeps/docs api/source/RoomVisual.md 2026-09-30
// 二轮取证——docs.screeps.com 整页抓取会命中 Game.map.visual 的表，GPT-P1 勘
// 正；engine 只透传 style，默认值渲染端应用=我方同位——G1 黑盒契约锚）。

describe('parseColor（官方 any web format）', () => {
  it('hex 四形态与 transparent', () => {
    expect(parseColor('#fff')).toEqual({ color: 0xffffff, alpha: 1 });
    expect(parseColor('#f00')).toEqual({ color: 0xff0000, alpha: 1 });
    expect(parseColor('#12abEF')).toEqual({ color: 0x12abef, alpha: 1 });
    expect(parseColor('#f008')).toEqual({ color: 0xff0000, alpha: 136 / 255 }); // nibble 8=0x88
    expect(parseColor('#ff000080')).toEqual({ color: 0xff0000, alpha: 128 / 255 });
    expect(parseColor('transparent')).toEqual({ color: 0, alpha: 0 });
    expect(parseColor('#12345')).toBeNull();
    expect(parseColor('')).toBeNull();
  });
  it('CSS 命名色（官方示例 green/aqua 在列；grey 系别名）', () => {
    expect(parseColor('red')).toEqual({ color: 0xff0000, alpha: 1 });
    expect(parseColor('GREEN')).toEqual({ color: 0x008000, alpha: 1 });
    expect(parseColor('aqua')).toEqual({ color: 0x00ffff, alpha: 1 });
    expect(parseColor('rebeccapurple')).toEqual({ color: 0x663399, alpha: 1 });
    expect(parseColor('grey')).toEqual({ color: 0x808080, alpha: 1 });
    expect(parseColor('lightslategrey')).toEqual({ color: 0x778899, alpha: 1 });
  });
  it('rgb()/rgba()（逗号/空格语法、%、/α）', () => {
    expect(parseColor('rgb(255, 0, 0)')).toEqual({ color: 0xff0000, alpha: 1 });
    expect(parseColor('rgb(255 0 0)')).toEqual({ color: 0xff0000, alpha: 1 });
    expect(parseColor('rgba(255, 0, 0, 0.5)')).toEqual({ color: 0xff0000, alpha: 0.5 });
    expect(parseColor('rgb(100% 0% 0% / 50%)')).toEqual({ color: 0xff0000, alpha: 0.5 });
    expect(parseColor('rgba(16, 32, 48, 25%)')).toEqual({ color: 0x102030, alpha: 0.25 });
  });
  it('hsl()/hsla()', () => {
    expect(parseColor('hsl(0, 100%, 50%)')).toEqual({ color: 0xff0000, alpha: 1 });
    expect(parseColor('hsl(120 100% 50% / 0.5)')).toEqual({ color: 0x00ff00, alpha: 0.5 });
    expect(parseColor('hsl(240, 100%, 50%)')).toEqual({ color: 0x0000ff, alpha: 1 });
  });
  it.each([
    ['rgba(255,0,0,.5)', 0xff0000, 0.5],
    ['rgb(255\n0\n0 / .5)', 0xff0000, 0.5],
    ['rgba(255,0,0,+.5)', 0xff0000, 0.5],
    ['rgb(100% 0 0 / 5e-1)', 0xff0000, 0.5],
    ['hsl(.5turn,100%,50%)', 0x00ffff, 1],
    ['hsl(100grad,100%,50%)', 0x80ff00, 1],
    ['hsl(180deg 100 50)', 0x00ffff, 1],
    ['hsl(3.141592653589793rad 100% 50% / .5)', 0x00ffff, 0.5],
    ['hsla(120,100%,50%,.5)', 0x00ff00, 0.5],
    ['hsl(-.5turn 100% 50%)', 0x00ffff, 1],
    [' TRANSPARENT ', 0, 0],
  ])('完整 CSS token：%s', (input, color, alpha) => {
    expect(parseColor(input)).toEqual({ color, alpha });
  });
  it.each([
    'rgbx(255,0,0)',
    'rgb(255,0,0,junk)',
    'rgb(1 2 3 garbage)',
    'rgb(1,2,3 / .5)',
    'hsl(1foo,100%,50%)',
    '#ggg',
    'constructor',
    'toString',
  ])('非法颜色不误命中：%s', (input) => {
    expect(parseColor(input)).toBeNull();
  });
  it('系统色等非基础形态 → null（G1 退默认）', () => {
    expect(parseColor('inherit')).toBeNull();
    expect(parseColor('rgb(1, 2)')).toBeNull();
  });
});

describe('resolveLine（width 0.1 / color #ffffff / opacity 0.5）', () => {
  const cell = 100;
  it('全缺省=官方默认值', () => {
    const r = resolveLine(undefined, cell);
    expect(r).toEqual({
      color: { color: 0xffffff, alpha: 1 },
      widthPx: 10,
      opacity: 0.5,
      dash: 'solid',
    });
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

describe('resolveRect（fill #ffffff / stroke 无 / strokeWidth 0.1）', () => {
  const cell = 100;
  it('全缺省=填充无描边（与 poly 不对称）——全形断言', () => {
    expect(resolveRect(undefined, cell)).toEqual({
      fill: { color: 0xffffff, alpha: 1 },
      stroke: null,
      strokePx: 10,
      opacity: 0.5,
      dash: 'solid',
    });
  });
  it('transparent 填充 → α0；显式描边生效', () => {
    const r = resolveRect(
      { fill: 'transparent', stroke: '#105010', strokeWidth: 0.25 } as VisualStyle,
      cell,
    );
    expect(r.fill!.alpha).toBe(0);
    expect(r.stroke).toEqual({ color: 0x105010, alpha: 1 });
    expect(r.strokePx).toBe(25);
  });
  it('不支持的颜色形态：默认有色字段退默认色；默认无层字段保持无层（评审 A-J1）', () => {
    expect(resolveRect({ fill: 'not-a-color' } as VisualStyle, cell).fill).toEqual({
      color: 0xffffff,
      alpha: 1,
    });
    expect(resolveRect({ stroke: 'not-a-color' } as VisualStyle, cell).stroke).toBeNull();
  });
});

describe('resolveCircle（radius 0.15 / fill #ffffff / stroke 无）', () => {
  it('全缺省=官方默认值——全形断言', () => {
    expect(resolveCircle(undefined, 100)).toEqual({
      fill: { color: 0xffffff, alpha: 1 },
      stroke: null,
      strokePx: 10,
      opacity: 0.5,
      dash: 'solid',
      radiusPx: 15,
    });
  });
  it('负半径退默认', () => {
    expect(resolveCircle({ radius: -1 } as VisualStyle, 100).radiusPx).toBe(15);
  });
});

describe('resolvePoly（fill 无 / stroke #ffffff / strokeWidth 0.1）', () => {
  it('全缺省=描边无填充（与 rect/circle 不对称）——全形断言', () => {
    expect(resolvePoly(undefined, 100)).toEqual({
      fill: null,
      stroke: { color: 0xffffff, alpha: 1 },
      strokePx: 10,
      opacity: 0.5,
      dash: 'solid',
    });
  });
  it('默认无层字段收到不支持色串保持无层（评审 A-J1）', () => {
    expect(resolvePoly({ fill: 'not-a-color' } as VisualStyle, 100).fill).toBeNull();
  });
});

describe('resolveText（font 0.8 缺省 / strokeWidth 0.15 / padding 0.3 / align center / opacity 1.0）', () => {
  const cell = 100;
  it('全缺省=官方默认值（font 缺省 0.8=官方示例值，官方未载缺省——G1）', () => {
    expect(resolveText(undefined, cell)).toEqual({
      color: { color: 0xffffff, alpha: 1 },
      opacity: 1,
      fontSizePx: 80,
      fontFamily: 'sans-serif',
      fontWeight: 'normal',
      fontStyle: 'normal',
      fontVariant: 'normal',
      stroke: null,
      strokePx: 15,
      background: null,
      paddingPx: 30,
      align: 'center',
      verticalMiddle: false,
    });
  });
  it('font 数字=游戏坐标相对尺寸；脏数据退缺省', () => {
    expect(resolveText({ font: 1.5 } as VisualStyle, cell).fontSizePx).toBe(150);
    expect(resolveText({ font: -2 } as VisualStyle, cell).fontSizePx).toBe(80);
    expect(resolveText({ font: Number.NaN } as VisualStyle, cell).fontSizePx).toBe(80);
  });
  it('font 字符串四形态（官方文档）', () => {
    // '20px' 绝对像素
    const abs = resolveText({ font: '20px' } as VisualStyle, cell);
    expect(abs.fontSizePx).toBe(20);
    expect(abs.fontFamily).toBe('sans-serif');
    // '0.7 serif' 尺寸+族
    const rel = resolveText({ font: '0.7 serif' } as VisualStyle, cell);
    expect(rel.fontSizePx).toBe(70);
    expect(rel.fontFamily).toBe('serif');
    // 'bold italic 1.5 Times New Roman' 前缀旗标+尺寸+多词族
    const full = resolveText({ font: 'bold italic 1.5 Times New Roman' } as VisualStyle, cell);
    expect(full.fontSizePx).toBe(150);
    expect(full.fontWeight).toBe('bold');
    expect(full.fontStyle).toBe('italic');
    expect(full.fontFamily).toBe('Times New Roman');
    // small-caps 变体
    expect(resolveText({ font: 'small-caps 1 serif' } as VisualStyle, cell).fontVariant).toBe(
      'small-caps',
    );
  });
  it('对齐/描边/背景：背景启用 verticalMiddle（官方明载）', () => {
    const styled = resolveText(
      {
        align: 'right',
        stroke: '#000000',
        strokeWidth: 0.2,
        backgroundColor: 'rgba(0, 0, 0, 0.7)',
        backgroundPadding: 0.5,
        opacity: 0.8,
      } as VisualStyle,
      cell,
    );
    expect(styled.align).toBe('right');
    expect(styled.stroke).toEqual({ color: 0x000000, alpha: 1 });
    expect(styled.strokePx).toBe(20);
    expect(styled.background).toEqual({ color: 0x000000, alpha: 0.7 });
    expect(styled.paddingPx).toBe(50);
    expect(styled.opacity).toBe(0.8);
    expect(styled.verticalMiddle).toBe(true);
    expect(
      resolveText({ backgroundColor: 'transparent' } as VisualStyle, cell).verticalMiddle,
    ).toBe(true); // 已启用（α0 观感无层）
    expect(
      resolveText({ backgroundColor: 'not-a-color' } as VisualStyle, cell).background,
    ).toBeNull(); // A-J1
  });
  it('非法 align 退 center', () => {
    expect(resolveText({ align: 'up' } as unknown as VisualStyle, cell).align).toBe('center');
  });
});

describe('dashSegments（节奏 G1 登记自定：dashed 0.5/0.25 · dotted 0.05/0.30）', () => {
  it('直线切段数与段长符合节奏', () => {
    const cell = 100; // 1 格=100px；dashed on=50px off=25px
    const segs = dashSegments(
      [
        { x: 0, y: 0 },
        { x: 2000, y: 0 },
      ],
      'dashed',
      cell,
    );
    // 2000px = 26 段节奏（50+25）×26=1950 + 尾 50 → 27 段 on
    expect(segs.length).toBe(27);
    expect(segs[0]![0]).toEqual({ x: 0, y: 0 });
    expect(segs[0]![1]!.x - segs[0]![0]!.x).toBeCloseTo(DASH_PATTERN.dashed.on * cell);
    expect(
      segs.every(([a, b]) => Math.abs(b!.x - a!.x - DASH_PATTERN.dashed.on * cell) < 1e-9),
    ).toBe(true);
  });
  it('短于一个 on 段的线整段画出', () => {
    const segs = dashSegments(
      [
        { x: 0, y: 0 },
        { x: 30, y: 0 },
      ],
      'dashed',
      100,
    );
    expect(segs).toEqual([
      [
        { x: 0, y: 0 },
        { x: 30, y: 0 },
      ],
    ]);
  });
  it('跨拐点连续行走（L 形累计长度）', () => {
    const cell = 100;
    const pts = [
      { x: 0, y: 0 },
      { x: 600, y: 0 },
      { x: 600, y: 600 },
    ]; // 600+600=1200
    const segs = dashSegments(pts, 'dashed', cell);
    const total = segs.reduce((sum, [a, b]) => sum + Math.hypot(b!.x - a!.x, b!.y - a!.y), 0);
    // 1200px = 16 整节奏（on50+off25）→ on 段总长 16×50=800，off 恰好收尾
    expect(total).toBeCloseTo(800, 6);
    expect(segs.length).toBe(16);
  });
  it('dotted 短 on 段（round 端帽成点）', () => {
    const cell = 100;
    const segs = dashSegments(
      [
        { x: 0, y: 0 },
        { x: 1000, y: 0 },
      ],
      'dotted',
      cell,
    );
    expect(segs[0]![1]!.x - segs[0]![0]!.x).toBeCloseTo(DASH_PATTERN.dotted.on * cell);
  });
  it('间隙宽度感知（评审 A-B1）：默认宽 0.1 下 off ≥ width+0.1 格，点/划离散', () => {
    const cell = 100;
    const w = 0.1 * cell; // 形状原语默认 strokeWidth（官方权威值）
    const dotted = dashSegments(
      [
        { x: 0, y: 0 },
        { x: 4000, y: 0 },
      ],
      'dotted',
      cell,
      w,
    );
    expect(dotted[1]![0]!.x - dotted[0]![1]!.x).toBe(DASH_PATTERN.dotted.off * cell); // 30px ≥ 20px，文档节奏胜出
    const dashed = dashSegments(
      [
        { x: 0, y: 0 },
        { x: 4000, y: 0 },
      ],
      'dashed',
      cell,
      w,
    );
    expect(dashed[1]![0]!.x - dashed[0]![1]!.x).toBe(DASH_PATTERN.dashed.off * cell); // 25px ≥ 20px，文档节奏胜出
    // 宽描边（0.5=历史默认）下取宽度感知间隙
    const wide = dashSegments(
      [
        { x: 0, y: 0 },
        { x: 4000, y: 0 },
      ],
      'dashed',
      cell,
      0.5 * cell,
    );
    expect(wide[1]![0]!.x - wide[0]![1]!.x).toBe(0.5 * cell + 0.1 * cell);
  });
  it('非有限坐标防御：不挂起、返回已产出段（评审 GPT-P1#8 死循环回归锚）', () => {
    const cell = 100;
    expect(
      dashSegments(
        [
          { x: 0, y: 0 },
          { x: Number.POSITIVE_INFINITY, y: 0 },
        ],
        'dashed',
        cell,
      ),
    ).toEqual([]);
    // 整路径含非有限点 → 顶栏守卫直接拒（不进入分段循环）
    expect(
      dashSegments(
        [
          { x: 0, y: 0 },
          { x: 50, y: 0 },
          { x: Number.NaN, y: 5 },
        ],
        'dotted',
        cell,
      ),
    ).toEqual([]);
    // 超大有限坐标（hypot(1e308,1e308)≈1.4e308 有限！）→ 段数上限截断，
    // 不挂起不 OOM（评审 GPT-P1#8 回归锚）
    const capped = dashSegments(
      [
        { x: 0, y: 0 },
        { x: 1e308, y: 1e308 },
      ],
      'dotted',
      cell,
    );
    expect(capped.length).toBe(4096);
    expect(capped[0]![0]).toEqual({ x: 0, y: 0 });
    expect(capped[0]!.every((p) => Number.isFinite(p.x) && Number.isFinite(p.y))).toBe(true);
  });
  it('circlePoints 首尾闭合、半径正确', () => {
    const pts = circlePoints(100, 200, 50);
    expect(pts[0]).toEqual({ x: 150, y: 200 });
    expect(pts[pts.length - 1]).toEqual({ x: 150, y: 200 });
    expect(pts[16]).toEqual({ x: 100, y: 250 });
  });
});
