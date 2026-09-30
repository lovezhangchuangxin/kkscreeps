/**
 * RoomVisual 样式解析（纯逻辑半件，零 pixi 导入——单测直采）。
 *
 * 官方默认值表与逐原语不对称性见 shared `feed.ts` VisualStyle 头注
 * （docs.screeps.com/api/#RoomVisual 2026-09-30 两路取证一致；engine
 * `src/game/rooms.js:1146-1218` 只透传玩家原样 style，默认值由闭源壳在
 * 渲染端应用——我方映射层同位应用，逐值单测断言）。单位：入参样式为游戏
 * 坐标，解析输出像素（×cell）。
 *
 * 黑盒登记（G1，官方壳未公开的实现细节按观感近似，不宣称源码级保真）：
 *   - 线端帽/拐角取 round（官方截图观感）；
 *   - dashed/dotted 的具体节奏官方未载，自定常量（本文件 DASH/DOT）；
 *   - 文字垂直锚取底边（近似 canvas textBaseline=alphabetic）；
 *   - 文字背景圆角半径取 paddingPx；
 *   - 其余 CSS 颜色形态（命名色/rgba() 等）官方未载 → 退该字段默认色。
 */
import type { VisualStyle } from '@kkscreeps/shared';

/** 解析后的颜色（0xRRGGBB + 0..1 α；α=0 视为无该层）。 */
export interface ResolvedColor {
  color: number;
  alpha: number;
}

/** #rgb/#rrggbb/#rrggbbaa/'transparent' → 颜色；其余形态官方未载 → null。 */
export function parseColor(value: string): ResolvedColor | null {
  if (value === 'transparent') return { color: 0, alpha: 0 };
  const m = /^#([0-9a-f]+)$/i.exec(value);
  const hex = m?.[1];
  if (hex === undefined) return null;
  const byte = (i: number): number => parseInt(hex.slice(i, i + 2), 16);
  switch (hex.length) {
    case 3: {
      const d = (c: string): number => parseInt(c, 16);
      const r = d(hex[0]!) * 17;
      const g = d(hex[1]!) * 17;
      const b = d(hex[2]!) * 17;
      return { color: (r << 16) | (g << 8) | b, alpha: 1 };
    }
    case 6:
      return { color: (byte(0) << 16) | (byte(2) << 8) | byte(4), alpha: 1 };
    case 8:
      return {
        color: (byte(0) << 16) | (byte(2) << 8) | byte(4),
        alpha: byte(6) / 255,
      };
    default:
      return null;
  }
}

/** undefined/非有限/负数 → 缺省（渲染端防御脏数据，G11 同精神）。 */
function numOr(value: number | undefined, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : fallback;
}

function clamp01(value: number | undefined, fallback: number): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) return fallback;
  return Math.min(1, Math.max(0, value));
}

/** 颜色字段解析（未给/不支持 → fallback 色、α=1；'transparent' → α=0）。 */
function colorOr(value: string | undefined, fallback: number): ResolvedColor {
  if (value === undefined) return { color: fallback, alpha: 1 };
  return parseColor(value) ?? { color: fallback, alpha: 1 };
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

/** rect：fill #ffffff / stroke 无 / strokeWidth 0.5 / opacity 0.5。 */
export function resolveRect(style: VisualStyle | undefined, cell: number): ShapeResolved {
  return {
    fill: style?.fill === undefined ? { color: 0xffffff, alpha: 1 } : colorOr(style.fill, 0xffffff),
    stroke: style?.stroke === undefined ? null : colorOr(style.stroke, 0xffffff),
    strokePx: numOr(style?.strokeWidth, 0.5) * cell,
    opacity: clamp01(style?.opacity, 0.5),
    dash: dashOf(style),
  };
}

/** circle：radius 10 / 其余同 rect。 */
export function resolveCircle(style: VisualStyle | undefined, cell: number): ShapeResolved & { radiusPx: number } {
  return {
    radiusPx: numOr(style?.radius, 10) * cell,
    ...resolveRect(style, cell),
  };
}

/** poly：fill 无 / stroke #ffffff / strokeWidth 0.5 / opacity 0.5。 */
export function resolvePoly(style: VisualStyle | undefined, cell: number): ShapeResolved {
  return {
    fill: style?.fill === undefined ? null : colorOr(style.fill, 0xffffff),
    stroke: style?.stroke === undefined ? { color: 0xffffff, alpha: 1 } : colorOr(style.stroke, 0xffffff),
    strokePx: numOr(style?.strokeWidth, 0.5) * cell,
    opacity: clamp01(style?.opacity, 0.5),
    dash: dashOf(style),
  };
}

export interface TextResolved {
  color: ResolvedColor;
  opacity: number;
  /** 像素字号（fontSize 游戏坐标 ×cell，默认 10）。 */
  fontSizePx: number;
  fontFamily: string;
  fontStyle: 'normal' | 'italic' | 'oblique';
  fontVariant: 'normal' | 'small-caps';
  /** null=无描边（strokeWidth 默认 0.15——官方 text 默认无描边色）。 */
  stroke: ResolvedColor | null;
  strokePx: number;
  /** null=无背景（backgroundPadding 默认 2 游戏坐标）。 */
  background: ResolvedColor | null;
  paddingPx: number;
  align: 'left' | 'center' | 'right';
}

/** text：color #ffffff / fontFamily sans-serif / fontSize 10 / strokeWidth 0.15 /
 * backgroundPadding 2 / align center / opacity 0.5。 */
export function resolveText(style: VisualStyle | undefined, cell: number): TextResolved {
  const fontStyle = style?.fontStyle;
  const fontVariant = style?.fontVariant;
  const align = style?.align;
  return {
    color: colorOr(style?.color, 0xffffff),
    opacity: clamp01(style?.opacity, 0.5),
    fontSizePx: numOr(style?.fontSize, 10) * cell,
    fontFamily: typeof style?.fontFamily === 'string' && style.fontFamily !== '' ? style.fontFamily : 'sans-serif',
    fontStyle: fontStyle === 'italic' || fontStyle === 'oblique' ? fontStyle : 'normal',
    fontVariant: fontVariant === 'small-caps' ? 'small-caps' : 'normal',
    stroke: style?.stroke === undefined ? null : colorOr(style.stroke, 0xffffff),
    strokePx: numOr(style?.strokeWidth, 0.15) * cell,
    background: style?.backgroundColor === undefined ? null : colorOr(style.backgroundColor, 0xffffff),
    paddingPx: numOr(style?.backgroundPadding, 2) * cell,
    align: align === 'left' || align === 'right' ? align : 'center',
  };
}

// ---- 线型分段（dashed/dotted 沿路径切段；节奏官方未载，G1 登记自定） ----

export interface Pt {
  x: number;
  y: number;
}

/** 节奏（游戏坐标）：dashed=0.5 划/0.25 空；dotted=0.05 点（round 端帽成圆点）/0.3 空。 */
export const DASH_PATTERN = { dashed: { on: 0.5, off: 0.25 }, dotted: { on: 0.05, off: 0.3 } } as const;

/** 沿折线按 [on,off] 节切段（像素坐标入参）——输出各 on 段端点对。
 * 实线不走此路径（原生 stroke）。 */
export function dashSegments(points: readonly Pt[], kind: 'dashed' | 'dotted', cell: number): Array<[Pt, Pt]> {
  const { on: onCell, off: offCell } = DASH_PATTERN[kind];
  const on = onCell * cell;
  const off = offCell * cell;
  const out: Array<[Pt, Pt]> = [];
  let drawing = true;
  let remain = on;
  let sx = points[0]!.x;
  let sy = points[0]!.y;
  for (let i = 1; i < points.length; i++) {
    let tx = points[i]!.x;
    let ty = points[i]!.y;
    let segLen = Math.hypot(tx - sx, ty - sy);
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
