/**
 * pixi v8 绘制帮手——官方处理器语义（engine/src/lib/processors/{object,sprite,
 * circle,draw,siteProgress}.js）到 v8 Graphics/Sprite/Text 的映射层。
 *
 * 官方语义锚（object.js）：
 *   - sprite width/height → scale = 目标/纹理自然尺寸（只给 width 等比）；
 *   - sprite 默认 anchor (0.5,0.5)（pivot 未给时）；
 *   - blendMode 1 = ADD；lighting 层成员经 display 钩子 = SCREEN。
 * 全部节点不销毁纹理（共享 atlas/standalone；R2 评审⑤纪律）。
 */
import { Container, Graphics, Sprite, Text, Texture, type TextStyleOptions } from 'pixi.js';
import type { PartValues } from './part-decl.js';
import { LIGHTING_MEMBER_BLEND } from './specs.js';

export { LIGHTING_MEMBER_BLEND };

// ---- 值读取小帮手（values 记录的窄化） ----

export function vNum(v: PartValues, key: string): number {
  return typeof v[key] === 'number' ? (v[key] as number) : 0;
}
export function vStr(v: PartValues, key: string): string {
  return typeof v[key] === 'string' ? (v[key] as string) : '';
}
export function vBool(v: PartValues, key: string): boolean {
  return v[key] === true;
}
export function vNums(v: PartValues, key: string): readonly number[] {
  const a = v[key];
  return Array.isArray(a) ? a : [];
}
export function vTint(v: PartValues, key: string): number | undefined {
  const t = v[key];
  return typeof t === 'number' ? t : undefined;
}

// ---- 精灵 ----

/** atlas 帧 Sprite（缺帧→null，调用方降级隐藏——无 atlas 占位模式）。 */
export function frameSprite(frames: ReadonlyMap<string, Texture>, name: string): Sprite | null {
  const texture = frames.get(name);
  if (!texture) return null;
  const sprite = new Sprite(texture);
  sprite.anchor.set(0.5, 0.5);
  return sprite;
}

/** 官方 width(/height) 语义：scale = 目标尺寸/纹理自然尺寸。 */
export function setSpriteSize(sprite: Sprite, width: number, height?: number): void {
  const tex = sprite.texture;
  if (height === undefined) {
    const s = width / tex.width;
    sprite.scale.set(s, s);
  } else {
    sprite.scale.set(width / tex.width, height / tex.height);
  }
}

// ---- Graphics 原语（draw.js 方法映射） ----

/** 圆（circle.js 语义：strokeWidth 有则描边环、fill 有则填充；环外半可见）。 */
export function gfxCircle(
  g: Graphics,
  radius: number,
  opts: { fill?: number; fillAlpha?: number; stroke?: number; strokeWidth?: number; strokeAlpha?: number },
): Graphics {
  if (opts.stroke !== undefined && (opts.strokeWidth ?? 0) > 0) {
    g.circle(0, 0, radius)
      .stroke({ width: opts.strokeWidth, color: opts.stroke, alpha: opts.strokeAlpha ?? 1 });
  }
  if (opts.fill !== undefined) {
    g.circle(0, 0, radius).fill({ color: opts.fill, alpha: opts.fillAlpha ?? 1 });
  }
  return g;
}

/** 椭圆（drawEllipse：半宽/半高）。 */
export function gfxEllipse(
  g: Graphics,
  halfWidth: number,
  halfHeight: number,
  opts: { fill?: number; stroke?: number; strokeWidth?: number },
): Graphics {
  const path = g.ellipse(0, 0, halfWidth, halfHeight);
  if (opts.stroke !== undefined && (opts.strokeWidth ?? 0) > 0) {
    path.stroke({ width: opts.strokeWidth, color: opts.stroke });
  }
  if (opts.fill !== undefined) path.fill({ color: opts.fill });
  return g;
}

export function gfxRoundRect(
  g: Graphics,
  x: number,
  y: number,
  w: number,
  h: number,
  radius: number,
  color: number,
): Graphics {
  if (w > 0 && h > 0) g.roundRect(x, y, w, h, radius).fill({ color });
  return g;
}

export function gfxRect(g: Graphics, x: number, y: number, w: number, h: number, color: number): Graphics {
  if (w > 0 && h > 0) g.rect(x, y, w, h).fill({ color });
  return g;
}

export function gfxPoly(g: Graphics, points: readonly number[], color: number): Graphics {
  if (points.length >= 6) g.poly([...points]).fill({ color });
  return g;
}

/** 圆弧描边（spawn/powerSpawn/invaderCore arc：lineStyle+arc 语义）。 */
export function gfxArcStroke(
  g: Graphics,
  radius: number,
  startAngle: number,
  endAngle: number,
  color: number,
  strokeWidth: number,
): Graphics {
  if (endAngle <= startAngle) return g;
  g.moveTo(Math.cos(startAngle) * radius, Math.sin(startAngle) * radius)
    .arc(0, 0, radius, startAngle, endAngle)
    .stroke({ width: strokeWidth, color });
  return g;
}

/**
 * siteProgress（siteProgress.js 逐行语义）：rotation=−π/2；lineWidth>0 时
 * r+lineWidth/2 全环描边；progress>0 时自 0 角起的扇形填充（起点=12 点钟），
 * 扇形带同色 1px 描边（:42 lineStyle(1,color,1)，评审 A-S4）。
 */
export function gfxSiteProgress(
  g: Graphics,
  angle: number,
  radius: number,
  lineWidth: number,
  color: number,
): Graphics {
  g.clear();
  g.rotation = -Math.PI / 2;
  if (lineWidth > 0) {
    g.circle(0, 0, radius + lineWidth / 2).stroke({ width: lineWidth, color });
  }
  if (angle > 0) {
    g.moveTo(0, 0)
      .lineTo(radius, 0)
      .arc(0, 0, radius, 0, angle)
      .lineTo(0, 0)
      .fill({ color });
    g.stroke({ width: 1, color });
  }
  return g;
}

/** 重绘入口（apply 每次全量 clear+重画——事件驱动、非逐帧；§2.4 无重建）。 */
export function redraw(node: Container): Graphics {
  const g = node as Graphics;
  g.clear();
  return g;
}

// ---- 文本（text.js 语义；v8 stroke 为对象样式） ----

export function makeText(payload: {
  text: string;
  style: TextStyleOptions;
  anchorX?: number;
  anchorY?: number;
}): Text {
  const t = new Text({ text: payload.text, style: payload.style });
  t.anchor.set(payload.anchorX ?? 0.5, payload.anchorY ?? 0.5);
  return t;
}

// ---- glow（lighting 层成员；display 钩子 SCREEN 语义） ----
// LIGHTING_MEMBER_BLEND 单源在 specs.ts（B-S5），此处转出口。

export function makeGlow(
  texture: Texture | null,
  width: number,
  height: number | undefined,
  alpha: number,
  tint?: number,
): Container {
  const holder = new Container({ label: 'glow' });
  if (!texture) return holder; // 无 atlas 占位模式：空件
  const sprite = new Sprite(texture);
  sprite.anchor.set(0.5, 0.5);
  setSpriteSize(sprite, width, height);
  sprite.alpha = alpha;
  if (tint !== undefined) sprite.tint = tint;
  sprite.blendMode = LIGHTING_MEMBER_BLEND;
  holder.addChild(sprite);
  return holder;
}

/** 隐/显工具（values.visible 惯例）。 */
export function setVisibility(node: Container, values: PartValues): void {
  node.visible = vBool(values, 'visible');
}
