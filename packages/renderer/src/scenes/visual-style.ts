/**
 * RoomVisual 样式解析（纯逻辑半件，零 pixi 导入——单测直采）。
 *
 * 官方默认值表与逐原语不对称性见 shared `feed.ts` VisualStyle 头注
 * （权威源 screeps/docs `api/source/RoomVisual.md`，2026-09-30 二轮取证勘正；
 * engine `src/game/rooms.js:1146-1218` 只透传玩家原样 style，默认值由闭源壳
 * 在渲染端应用——我方映射层同位应用，逐值单测断言）。单位：入参样式为游戏
 * 坐标，解析输出像素（×cell）。
 *
 * 黑盒登记（G1，官方壳未公开的实现细节按观感近似，不宣称源码级保真）：
 *   - 线端帽/拐角取 round（官方截图观感）；
 *   - dashed/dotted 的具体节奏官方未载，自定常量且间隙宽度感知（本文件）；
 *   - 文字垂直定位：官方明载「背景启用=middle，默认=基线点」——背景启用按
 *     middle 精确实现；无背景取文本框底边近似基线（≈基线+descender，字号
 *     越大偏差越大，待字体实测校准）；
 *   - text font 缺省值官方未载，取 0.8 游戏坐标（官方示例值同参）；
 *   - 颜色：官方 any web format——本文件支持 hex×4/rgb(a)/hsl(a)/CSS 命名色
 *     全表/'transparent'；残余系统色（Canvas/UI 关键字）退该字段默认（G1）。
 */
import type { VisualStyle } from '@kkscreeps/shared';

/** 解析后的颜色（0xRRGGBB + 0..1 α；α=0 视为无该层）。 */
export interface ResolvedColor {
  color: number;
  alpha: number;
}

/** CSS 命名色全表（含 grey 系别名；CSS3/Color4 标准 148+9 键）。 */
export const NAMED_COLORS: Readonly<Record<string, number>> = {
  aliceblue: 0xf0f8ff, antiquewhite: 0xfaebd7, aqua: 0x00ffff, aquamarine: 0x7fffd4,
  azure: 0xf0ffff, beige: 0xf5f5dc, bisque: 0xffe4c4, black: 0x000000,
  blanchedalmond: 0xffebcd, blue: 0x0000ff, blueviolet: 0x8a2be2, brown: 0xa52a2a,
  burlywood: 0xdeb887, cadetblue: 0x5f9ea0, chartreuse: 0x7fff00, chocolate: 0xd2691e,
  coral: 0xff7f50, cornflowerblue: 0x6495ed, cornsilk: 0xfff8dc, crimson: 0xdc143c,
  cyan: 0x00ffff, darkblue: 0x00008b, darkcyan: 0x008b8b, darkgoldenrod: 0xb8860b,
  darkgray: 0xa9a9a9, darkgreen: 0x006400, darkgrey: 0xa9a9a9, darkkhaki: 0xbdb76b,
  darkmagenta: 0x8b008b, darkolivegreen: 0x556b2f, darkorange: 0xff8c00,
  darkorchid: 0x9932cc, darkred: 0x8b0000, darksalmon: 0xe9967a, darkseagreen: 0x8fbc8f,
  darkslateblue: 0x483d8b, darkslategray: 0x2f4f4f, darkslategrey: 0x2f4f4f,
  darkturquoise: 0x00ced1, darkviolet: 0x9400d3, deeppink: 0xff1493,
  deepskyblue: 0x00bfff, dimgray: 0x696969, dimgrey: 0x696969, dodgerblue: 0x1e90ff,
  firebrick: 0xb22222, floralwhite: 0xfffaf0, forestgreen: 0x228b22,
  fuchsia: 0xff00ff, gainsboro: 0xdcdcdc, ghostwhite: 0xf8f8ff, gold: 0xffd700,
  goldenrod: 0xdaa520, gray: 0x808080, green: 0x008000, greenyellow: 0xadff2f,
  grey: 0x808080, honeydew: 0xf0fff0, hotpink: 0xff69b4, indianred: 0xcd5c5c,
  indigo: 0x4b0082, ivory: 0xfffff0, khaki: 0xf0e68c, lavender: 0xe6e6fa,
  lavenderblush: 0xfff0f5, lawngreen: 0x7cfc00, lemonchiffon: 0xfffacd,
  lightblue: 0xadd8e6, lightcoral: 0xf08080, lightcyan: 0xe0ffff,
  lightgoldenrodyellow: 0xfafad2, lightgray: 0xd3d3d3, lightgreen: 0x90ee90,
  lightgrey: 0xd3d3d3, lightpink: 0xffb6c1, lightsalmon: 0xffa07a,
  lightseagreen: 0x20b2aa, lightskyblue: 0x87cefa, lightslategray: 0x778899,
  lightslategrey: 0x778899, lightsteelblue: 0xb0c4de, lightyellow: 0xffffe0,
  lime: 0x00ff00, limegreen: 0x32cd32, linen: 0xfaf0e6, magenta: 0xff00ff,
  maroon: 0x800000, mediumaquamarine: 0x66cdaa, mediumblue: 0x0000cd,
  mediumorchid: 0xba55d3, mediumpurple: 0x9370db, mediumseagreen: 0x3cb371,
  mediumslateblue: 0x7b68ee, mediumspringgreen: 0x00fa9a, mediumturquoise: 0x48d1cc,
  mediumvioletred: 0xc71585, midnightblue: 0x191970, mintcream: 0xf5fffa,
  mistyrose: 0xffe4e1, moccasin: 0xffe4b5, navajowhite: 0xffdead, navy: 0x000080,
  oldlace: 0xfdf5e6, olive: 0x808000, olivedrab: 0x6b8e23, orange: 0xffa500,
  orangered: 0xff4500, orchid: 0xda70d6, palegoldenrod: 0xeee8aa,
  palegreen: 0x98fb98, paleturquoise: 0xafeeee, palevioletred: 0xdb7093,
  papayawhip: 0xffefd5, peachpuff: 0xffdab9, peru: 0xcd853f, pink: 0xffc0cb,
  plum: 0xdda0dd, powderblue: 0xb0e0e6, purple: 0x800080, rebeccapurple: 0x663399,
  red: 0xff0000, rosybrown: 0xbc8f8f, royalblue: 0x4169e1, saddlebrown: 0x8b4513,
  salmon: 0xfa8072, sandybrown: 0xf4a460, seagreen: 0x2e8b57, seashell: 0xfff5ee,
  sienna: 0xa0522d, silver: 0xc0c0c0, skyblue: 0x87ceeb, slateblue: 0x6a5acd,
  slategray: 0x708090, slategrey: 0x708090, snow: 0xfffafa, springgreen: 0x00ff7f,
  steelblue: 0x4682b4, tan: 0xd2b48c, teal: 0x008080, thistle: 0xd8bfd8,
  tomato: 0xff6347, turquoise: 0x40e0d0, violet: 0xee82ee, wheat: 0xf5deb3,
  white: 0xffffff, whitesmoke: 0xf5f5f5, yellow: 0xffff00, yellowgreen: 0x9acd32,
};

/** HSL→RGB（h∈[0,360)，s/l∈[0,1]）。 */
function hslToRgb(h: number, s: number, l: number): number {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = ((((h % 360) + 360) % 360) / 60);
  const x = c * (1 - Math.abs((hp % 2) - 1));
  const seg = Math.floor(hp);
  let r = 0, g = 0, b = 0;
  if (seg === 0) { r = c; g = x; }
  else if (seg === 1) { r = x; g = c; }
  else if (seg === 2) { g = c; b = x; }
  else if (seg === 3) { g = x; b = c; }
  else if (seg === 4) { r = x; b = c; }
  else { r = c; b = x; }
  const m = l - c / 2;
  const byte = (v: number): number => Math.round(Math.min(1, Math.max(0, v + m)) * 255);
  return (byte(r) << 16) | (byte(g) << 8) | byte(b);
}

/** 从函数色串提取数值元组（逗号或空格分隔，支持 `/ α` 与 `%`）。 */
function fnColorParams(value: string): number[] | null {
  const inner = /^\w+\(([^)]*)\)$/.exec(value.trim())?.[1];
  if (inner === undefined) return null;
  const parts = inner.split('/');
  const rgbPart = parts[0] ?? '';
  const alphaPart = parts[1];
  const nums = rgbPart.match(/-?\d+(?:\.\d+)?%?/g);
  if (nums === null || nums.length < 3) return null;
  const parse = (t: string, max: number): number =>
    t.endsWith('%') ? (parseFloat(t) / 100) * max : parseFloat(t);
  const out = [parse(nums[0]!, 255), parse(nums[1]!, 255), parse(nums[2]!, 255)];
  const a = alphaPart !== undefined ? alphaPart.trim() : (nums[3] ?? '1');
  out.push(parse(a.trim(), 1));
  return out;
}

/** 官方 any web format 颜色解析：'transparent'/#hex×4/rgb(a)/hsl(a)/命名色 →
 * 颜色；其余（系统色等）→ null（调用方退该字段默认，G1 登记）。 */
export function parseColor(value: string): ResolvedColor | null {
  if (value === 'transparent') return { color: 0, alpha: 0 };
  const lower = value.trim().toLowerCase();
  // #rgb/#rgba/#rrggbb/#rrggbbaa
  if (lower.startsWith('#')) {
    const hex = lower.slice(1);
    const byte = (i: number): number => parseInt(hex.slice(i, i + 2), 16);
    switch (hex.length) {
      case 3: {
        const d = (c: string): number => parseInt(c, 16);
        const r = d(hex[0]!) * 17;
        const g = d(hex[1]!) * 17;
        const b = d(hex[2]!) * 17;
        return { color: (r << 16) | (g << 8) | b, alpha: 1 };
      }
      case 4: {
        const d = (c: string): number => parseInt(c, 16);
        const r = d(hex[0]!) * 17;
        const g = d(hex[1]!) * 17;
        const b = d(hex[2]!) * 17;
        return { color: (r << 16) | (g << 8) | b, alpha: d(hex[3]!) * 17 / 255 };
      }
      case 6:
        return { color: (byte(0) << 16) | (byte(2) << 8) | byte(4), alpha: 1 };
      case 8:
        return { color: (byte(0) << 16) | (byte(2) << 8) | byte(4), alpha: byte(6) / 255 };
      default:
        return null;
    }
  }
  const named = NAMED_COLORS[lower];
  if (named !== undefined) return { color: named, alpha: 1 };
  if (lower.startsWith('rgb')) {
    const p = fnColorParams(lower);
    if (p === null) return null;
    const alpha = Number.isFinite(p[3]) ? Math.min(1, Math.max(0, p[3]!)) : 1;
    const r = Math.min(255, Math.max(0, p[0]!));
    const g = Math.min(255, Math.max(0, p[1]!));
    const b = Math.min(255, Math.max(0, p[2]!));
    return { color: (Math.round(r) << 16) | (Math.round(g) << 8) | Math.round(b), alpha };
  }
  if (lower.startsWith('hsl')) {
    // hsl 专属解析：h=度数（不标尺化），s/l 百分比→0..1（现代语法数字形式
    // ≤1 视为比值）
    const inner = /^\w+\(([^)]*)\)$/.exec(lower)?.[1];
    if (inner === undefined) return null;
    const parts = inner.split('/');
    const nums = (parts[0] ?? '').match(/-?\d+(?:\.\d+)?%?/g);
    if (nums === null || nums.length < 3) return null;
    const ch = (t: string): number => parseFloat(t);
    const ratio = (t: string): number => (t.endsWith('%') ? parseFloat(t) / 100 : Math.min(1, Math.max(0, parseFloat(t))));
    const alphaText = parts[1] !== undefined ? parts[1].trim() : (nums[3] ?? '1');
    const alpha = Math.min(1, Math.max(0, alphaText.endsWith('%') ? parseFloat(alphaText) / 100 : parseFloat(alphaText)));
    const h = ch(nums[0]!);
    const s = ratio(nums[1]!);
    const l = ratio(nums[2]!);
    if (![h, s, l].every(Number.isFinite)) return null;
    return { color: hslToRgb(h, s, l), alpha: Number.isFinite(alpha) ? alpha : 1 };
  }
  return null;
}

/** undefined/非有限/负数 → 缺省（渲染端防御脏数据，G11 同精神）。 */
function numOr(value: number | undefined, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : fallback;
}

function clamp01(value: number | undefined, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.min(1, Math.max(0, value));
}

/** 颜色字段解析（默认有色字段：未给/不支持 → fallback 色、α=1；
 * 'transparent' → α=0）。 */
function colorOr(value: string | undefined, fallback: number): ResolvedColor {
  if (value === undefined) return { color: fallback, alpha: 1 };
  return parseColor(value) ?? { color: fallback, alpha: 1 };
}

/** 「默认=无该层」字段的颜色解析（poly.fill、rect/circle.stroke、
 * text.stroke/backgroundColor）：未给**或不支持色串** → null（层保持关——
 * G1 回退口径「退该字段默认色」对这些字段即无层；评审 A-J1）。 */
function colorOrOff(value: string | undefined): ResolvedColor | null {
  if (value === undefined) return null;
  return parseColor(value);
}

export type DashKind = 'solid' | 'dashed' | 'dotted';

function dashOf(style: VisualStyle | undefined): DashKind {
  return style?.lineStyle === 'dashed' || style?.lineStyle === 'dotted' ? style.lineStyle : 'solid';
}

// ---- 逐原语解析（默认值逐原语不对称——见文件头/shared 头注） ----

export interface LineResolved {
  color: ResolvedColor;
  /** 像素线宽。 */
  widthPx: number;
  opacity: number;
  dash: DashKind;
}

/** line：width 0.1 / color #ffffff / opacity 0.5。 */
export function resolveLine(style: VisualStyle | undefined, cell: number): LineResolved {
  return {
    color: colorOr(style?.color, 0xffffff),
    widthPx: numOr(style?.width, 0.1) * cell,
    opacity: clamp01(style?.opacity, 0.5),
    dash: dashOf(style),
  };
}

export interface ShapeResolved {
  /** null=无该层（rect/circle 默认有填充、poly 默认无填充；stroke 默认仅 poly 有）。 */
  fill: ResolvedColor | null;
  stroke: ResolvedColor | null;
  strokePx: number;
  opacity: number;
  dash: DashKind;
}

/** rect：fill #ffffff / stroke 无 / strokeWidth 0.1 / opacity 0.5。 */
export function resolveRect(style: VisualStyle | undefined, cell: number): ShapeResolved {
  return {
    fill: style?.fill === undefined ? { color: 0xffffff, alpha: 1 } : colorOr(style.fill, 0xffffff),
    stroke: colorOrOff(style?.stroke),
    strokePx: numOr(style?.strokeWidth, 0.1) * cell,
    opacity: clamp01(style?.opacity, 0.5),
    dash: dashOf(style),
  };
}

/** circle：radius 0.15 / 其余同 rect。 */
export function resolveCircle(style: VisualStyle | undefined, cell: number): ShapeResolved & { radiusPx: number } {
  return {
    radiusPx: numOr(style?.radius, 0.15) * cell,
    ...resolveRect(style, cell),
  };
}

/** poly：fill 无 / stroke #ffffff / strokeWidth 0.1 / opacity 0.5。 */
export function resolvePoly(style: VisualStyle | undefined, cell: number): ShapeResolved {
  return {
    fill: colorOrOff(style?.fill),
    stroke: style?.stroke === undefined ? { color: 0xffffff, alpha: 1 } : colorOr(style.stroke, 0xffffff),
    strokePx: numOr(style?.strokeWidth, 0.1) * cell,
    opacity: clamp01(style?.opacity, 0.5),
    dash: dashOf(style),
  };
}

export interface TextResolved {
  color: ResolvedColor;
  opacity: number;
  /** 像素字号。 */
  fontSizePx: number;
  fontFamily: string;
  fontWeight: 'normal' | 'bold';
  fontStyle: 'normal' | 'italic' | 'oblique';
  fontVariant: 'normal' | 'small-caps';
  /** null=无描边（strokeWidth 默认 0.15——官方 text 默认无描边色）。 */
  stroke: ResolvedColor | null;
  strokePx: number;
  /** null=无背景（backgroundPadding 默认 0.3 游戏坐标；背景启用时官方明载
   * 文字垂直居中）。 */
  background: ResolvedColor | null;
  paddingPx: number;
  align: 'left' | 'center' | 'right';
  /** 官方明载：背景启用时文字垂直居中（默认基线点）。 */
  verticalMiddle: boolean;
}

/** text font 解析（官方 number|string 四形态，见 shared 头注）。
 * 缺省 0.8 游戏坐标（官方未载，G1：取官方示例值）。 */
function resolveFont(font: number | string | undefined, cell: number): Pick<
  TextResolved,
  'fontSizePx' | 'fontFamily' | 'fontWeight' | 'fontStyle' | 'fontVariant'
> {
  const DEFAULT = { fontSizePx: 0.8 * cell, fontFamily: 'sans-serif', fontWeight: 'normal' as const, fontStyle: 'normal' as const, fontVariant: 'normal' as const };
  if (typeof font === 'number') {
    return { ...DEFAULT, fontSizePx: numOr(font, 0.8) * cell };
  }
  const s = typeof font === 'string' ? font.trim() : '';
  if (s === '') return DEFAULT;
  // '20px' 单 token=绝对像素
  const abs = /^(\d+(?:\.\d+)?)px$/i.exec(s);
  if (abs !== null) return { ...DEFAULT, fontSizePx: parseFloat(abs[1]!) };
  // CSS 字体串：[style|variant|weight]* size family
  const tokens = s.split(/\s+/);
  let fontStyle: TextResolved['fontStyle'] = 'normal';
  let fontVariant: TextResolved['fontVariant'] = 'normal';
  let fontWeight: TextResolved['fontWeight'] = 'normal';
  let i = 0;
  for (; i < tokens.length; i++) {
    const t = tokens[i]!.toLowerCase();
    if (t === 'italic' || t === 'oblique') fontStyle = t;
    else if (t === 'small-caps') fontVariant = 'small-caps';
    else if (t === 'bold') fontWeight = 'bold';
    else if (t === 'normal') { /* 显式 normal */ }
    else break;
  }
  const sizeToken = tokens[i];
  let fontSizePx = DEFAULT.fontSizePx;
  let familyFrom = i;
  if (sizeToken !== undefined) {
    const px = /^(\d+(?:\.\d+)?)px$/i.exec(sizeToken);
    if (px !== null) { fontSizePx = parseFloat(px[1]!); familyFrom = i + 1; }
    else if (/^\d+(?:\.\d+)?$/.test(sizeToken)) { fontSizePx = parseFloat(sizeToken) * cell; familyFrom = i + 1; }
    else familyFrom = i; // 无尺寸 token：整串视为族
  }
  const family = tokens.slice(familyFrom).join(' ');
  return {
    fontSizePx: numOr(fontSizePx, 0.8 * cell),
    fontFamily: family !== '' ? family : DEFAULT.fontFamily,
    fontWeight,
    fontStyle,
    fontVariant,
  };
}

/** text：color #ffffff / font 0.8（G1）/ strokeWidth 0.15 / backgroundPadding 0.3 /
 * align center / opacity 1.0。 */
export function resolveText(style: VisualStyle | undefined, cell: number): TextResolved {
  const align = style?.align;
  const background = colorOrOff(style?.backgroundColor);
  return {
    color: colorOr(style?.color, 0xffffff),
    opacity: clamp01(style?.opacity, 1.0),
    ...resolveFont(style?.font, cell),
    stroke: colorOrOff(style?.stroke),
    strokePx: numOr(style?.strokeWidth, 0.15) * cell,
    background,
    paddingPx: numOr(style?.backgroundPadding, 0.3) * cell,
    align: align === 'left' || align === 'right' ? align : 'center',
    verticalMiddle: background !== null,
  };
}

// ---- 线型分段（dashed/dotted 沿路径切段；节奏官方未载，G1 登记自定） ----

export interface Pt {
  x: number;
  y: number;
}

/** 节奏（游戏坐标，G1 登记自定）：dashed=0.5 划/0.25 空；dotted=0.05 点
 * （round 端帽成圆点）/0.3 空。**间隙宽度感知**（评审 A-B1）：round 端帽使
 * 每段视觉长 = on + widthPx，off < width+最小隙即粘连成实线——off 取
 * max(offCell×cell, widthPx + 0.1×cell)（最小隙 0.1 格）。 */
export const DASH_PATTERN = { dashed: { on: 0.5, off: 0.25 }, dotted: { on: 0.05, off: 0.3 } } as const;

/** 单条 visual 的分段上限（评审 GPT-P1#8：超大有限坐标——hypot(1e308,1e308)
 * 仍有限——会使切段循环以有限步长推进天文次数（内存/时钟双爆）。4096 段
 * ≈ 30 万 px 路径，远超任何房间内可视需求；超限即截断（超出屏外无观感）。 */
export const MAX_DASH_SEGMENTS = 4096;

/** 沿折线按 [on,off] 节切段（像素坐标入参；widthPx=描边宽，间隙宽度感知）——
 * 输出各 on 段端点对。实线不走此路径（原生 stroke）。非有限输入返回空；
 * 段数达上限即截断。 */
export function dashSegments(points: readonly Pt[], kind: 'dashed' | 'dotted', cell: number, widthPx = 0): Array<[Pt, Pt]> {
  if (points.length === 0 || !points.every((p) => Number.isFinite(p.x) && Number.isFinite(p.y))) return [];
  const { on: onCell, off: offCell } = DASH_PATTERN[kind];
  const on = onCell * cell;
  const off = Math.max(offCell * cell, widthPx + 0.1 * cell);
  const out: Array<[Pt, Pt]> = [];
  let drawing = true;
  let remain = on;
  let sx = points[0]!.x;
  let sy = points[0]!.y;
  for (let i = 1; i < points.length; i++) {
    let tx = points[i]!.x;
    let ty = points[i]!.y;
    let segLen = Math.hypot(tx - sx, ty - sy);
    if (!Number.isFinite(segLen)) return out;
    while (segLen > 0) {
      const step = Math.min(remain, segLen);
      const ratio = step / segLen;
      const nx = sx + (tx - sx) * ratio;
      const ny = sy + (ty - sy) * ratio;
      if (drawing) out.push([{ x: sx, y: sy }, { x: nx, y: ny }]);
      sx = nx;
      sy = ny;
      segLen -= step;
      remain -= step;
      if (remain <= 0) {
        drawing = !drawing;
        remain = drawing ? on : off;
      }
      if (out.length >= MAX_DASH_SEGMENTS) return out;
    }
  }
  return out;
}

/** 圆周采样为折线（虚线圆用——实线圆走原生 circle().stroke()）。 */
export function circlePoints(cx: number, cy: number, radiusPx: number, samples = 64): Pt[] {
  const pts: Pt[] = [];
  for (let i = 0; i <= samples; i++) {
    const a = (i / samples) * Math.PI * 2;
    pts.push({ x: cx + Math.cos(a) * radiusPx, y: cy + Math.sin(a) * radiusPx });
  }
  return pts;
}
