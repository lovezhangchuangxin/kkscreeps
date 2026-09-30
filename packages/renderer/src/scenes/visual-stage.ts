/**
 * RoomVisual 渲染舞台（R6）：五原语（line/rect/circle/poly/text）→ Graphics/Text。
 * 挂 RoomScene root 顶（effects 之上——visuals 是标注面，官方观感恒在对象/
 * 特效之上且不受 lighting 压暗；z 序黑盒推断登记 G1）。
 *
 * 重建策略：setVisuals 全量替换（官方每 tick 全量下发语义；typical ≤100 条，
 * R7 预算场景——每 tick 重建的帧成本登记为 R7 测量点）；同引用短路
 * （FeedChange.visuals 仓库原引用跨增量稳定）。Text 自绘 canvas 纹理是私有
 * 源，销毁走精确四标志（C-S2/R3 纪律）；其余 Graphics 无纹理。
 *
 * 样式默认值/颜色/线型解析在 visual-style.ts（纯逻辑半件，逐值单测）；
 * 单条渲染失败（无 canvas 测试环境/脏数据）跳过该条不崩溃（say 气泡降级同款）。
 */
import { Container, Graphics, Text } from 'pixi.js';
import { isRoomVisual, type RoomVisual } from '@kkscreeps/shared';
import {
  circlePoints,
  dashSegments,
  resolveCircle,
  resolveLine,
  resolvePoly,
  resolveRect,
  resolveText,
  type Pt,
  type ShapeResolved,
} from './visual-style.js';

interface Entry {
  container: Container;
  text: Text | null;
}

const STROKE_CAP = { cap: 'round', join: 'round' } as const;

export class VisualStage {
  readonly root: Container;
  private current: readonly RoomVisual[] | null = null;
  private readonly entries: Entry[] = [];
  private applied = 0;

  constructor(private readonly cell: number) {
    this.root = new Container({ label: 'layer-visuals' });
  }

  /** 当前已渲染的原语条数（无效条目不计——getStats().visuals 数据源）。 */
  get count(): number {
    return this.applied;
  }

  /** 全量替换（官方每 tick 全量下发语义）；同引用零成本短路。 */
  setVisuals(visuals: readonly RoomVisual[]): void {
    if (visuals === this.current) return;
    this.current = visuals;
    this.clear();
    let applied = 0;
    for (const visual of visuals) {
      if (isRoomVisual(visual) && this.render(visual)) applied++;
    }
    this.applied = applied;
  }

  destroy(): void {
    this.clear();
    this.current = null;
    this.root.destroy({ children: true, context: true });
  }

  private clear(): void {
    // Pixi v8 的非空 destroy options 需 context:true；所有 Graphics 均独立持有。
    for (const entry of this.entries) {
      entry.text?.destroy({ style: true, texture: true, textureSource: true });
      entry.container.destroy({ children: true, context: true });
    }
    this.entries.length = 0;
    this.applied = 0;
  }

  /** 渲染单条；false=降级跳过（measure 失败等），不中断整批。 */
  private render(visual: RoomVisual): boolean {
    try {
      switch (visual.type) {
        case 'line':
          return this.drawLine(visual.points.map((p) => ({ x: p.x * this.cell, y: p.y * this.cell })), resolveLine(visual.style, this.cell));
        case 'rect':
          return this.drawRect(visual);
        case 'circle':
          return this.drawCircle(visual);
        case 'poly':
          return this.drawPoly(visual);
        case 'text':
          return this.drawText(visual);
      }
    } catch {
      return false;
    }
  }

  private add(container: Container): boolean {
    this.root.addChild(container);
    this.entries.push({ container, text: null });
    return true;
  }

  /** 折线描边：实线=原生单路径；dashed/dotted=切段后多子路径一次 stroke。 */
  private strokePath(g: Graphics, points: readonly Pt[], shape: ShapeResolved, widthPx: number, closed: boolean): void {
    if (shape.stroke === null || shape.stroke.alpha <= 0 || widthPx <= 0) return;
    const style = { color: shape.stroke.color, alpha: shape.stroke.alpha * shape.opacity, width: widthPx, ...STROKE_CAP };
    if (shape.dash === 'solid') {
      g.moveTo(points[0]!.x, points[0]!.y);
      for (let i = 1; i < points.length; i++) g.lineTo(points[i]!.x, points[i]!.y);
      if (closed) g.closePath();
      g.stroke(style);
      return;
    }
    const path = closed ? [...points, points[0]!] : [...points];
    for (const [a, b] of dashSegments(path, shape.dash, this.cell, widthPx)) {
      g.moveTo(a.x, a.y).lineTo(b.x, b.y);
    }
    g.stroke(style);
  }

  private drawLine(points: Pt[], resolved: ReturnType<typeof resolveLine>): boolean {
    if (resolved.color.alpha * resolved.opacity <= 0 || resolved.widthPx <= 0) return this.add(new Graphics());
    const g = new Graphics();
    if (resolved.dash === 'solid') {
      g.moveTo(points[0]!.x, points[0]!.y);
      g.lineTo(points[1]!.x, points[1]!.y);
    } else {
      for (const [a, b] of dashSegments(points, resolved.dash, this.cell, resolved.widthPx)) g.moveTo(a.x, a.y).lineTo(b.x, b.y);
    }
    g.stroke({ color: resolved.color.color, alpha: resolved.color.alpha * resolved.opacity, width: resolved.widthPx, ...STROKE_CAP });
    return this.add(g);
  }

  private drawRect(visual: Extract<RoomVisual, { type: 'rect' }>): boolean {
    const r = resolveRect(visual.style, this.cell);
    const x = visual.x * this.cell;
    const y = visual.y * this.cell;
    const w = visual.w * this.cell;
    const h = visual.h * this.cell;
    const g = new Graphics();
    if (r.fill !== null && r.fill.alpha > 0) g.rect(x, y, w, h).fill({ color: r.fill.color, alpha: r.fill.alpha * r.opacity });
    const corners: Pt[] = [
      { x, y },
      { x: x + w, y },
      { x: x + w, y: y + h },
      { x, y: y + h },
    ];
    this.strokePath(g, corners, r, r.strokePx, true);
    return this.add(g);
  }

  private drawCircle(visual: Extract<RoomVisual, { type: 'circle' }>): boolean {
    const r = resolveCircle(visual.style, this.cell);
    const cx = visual.x * this.cell;
    const cy = visual.y * this.cell;
    const g = new Graphics();
    if (r.fill !== null && r.fill.alpha > 0) g.circle(cx, cy, r.radiusPx).fill({ color: r.fill.color, alpha: r.fill.alpha * r.opacity });
    if (r.stroke !== null && r.stroke.alpha > 0 && r.strokePx > 0) {
      if (r.dash === 'solid') {
        g.circle(cx, cy, r.radiusPx).stroke({ color: r.stroke.color, alpha: r.stroke.alpha * r.opacity, width: r.strokePx, ...STROKE_CAP });
      } else {
        this.strokePath(g, circlePoints(cx, cy, r.radiusPx), r, r.strokePx, true);
      }
    }
    return this.add(g);
  }

  private drawPoly(visual: Extract<RoomVisual, { type: 'poly' }>): boolean {
    const r = resolvePoly(visual.style, this.cell);
    const pts = visual.points.map((p) => ({ x: p.x * this.cell, y: p.y * this.cell }));
    if (pts.length < 1) return this.add(new Graphics());
    const g = new Graphics();
    if (pts.length >= 3 && r.fill !== null && r.fill.alpha > 0) {
      // canvas 语义：开放路径填充按闭合区域计（官方 poly=折线）
      g.poly(pts).fill({ color: r.fill.color, alpha: r.fill.alpha * r.opacity });
    }
    if (pts.length >= 2) this.strokePath(g, pts, r, r.strokePx, false); // 折线开放描边（评审 GPT-P1#3）
    return this.add(g);
  }

  private drawText(visual: Extract<RoomVisual, { type: 'text' }>): boolean {
    const r = resolveText(visual.style, this.cell);
    const container = new Container({ label: 'visual-text' });
    let text: Text | null = null;
    try {
      text = new Text({
        text: visual.text,
        style: {
          fontFamily: r.fontFamily,
          fontSize: r.fontSizePx,
          fontWeight: r.fontWeight,
          fontStyle: r.fontStyle,
          fontVariant: r.fontVariant,
          fill: { color: r.color.color, alpha: r.color.alpha },
          align: r.align,
          // v8 text stroke 样式对象；join=round 防 miter 尖刺（canvas 观感近似）
          ...(r.stroke !== null && r.stroke.alpha > 0 && r.strokePx > 0
            ? { stroke: { color: r.stroke.color, alpha: r.stroke.alpha, width: r.strokePx, join: 'round' as const } }
            : {}),
        },
      });
      // 官方契约：x/y 为基线点，align 锚 x=left 0/center .5/right 1（canvas
      // textAlign 语义）；背景启用时垂直居中（官方明载），无背景取文本框底边
      // 近似基线（≈基线+descender——G1 登记，待字体实测校准）。
      const ax = r.align === 'left' ? 0 : r.align === 'right' ? 1 : 0.5;
      const ay = r.verticalMiddle ? 0.5 : 1;
      text.anchor.set(ax, ay);
      if (r.background !== null && r.background.alpha > 0) {
        const bg = new Graphics();
        const pad = r.paddingPx;
        const topLocal = -ay * text.height;
        bg.roundRect(-ax * text.width - pad, topLocal - pad, text.width + pad * 2, text.height + pad * 2, Math.min(pad, text.height / 2));
        // 背景独立承担 background α×opacity（评审 A-J4：color α 不应淡化背景）
        bg.fill({ color: r.background.color, alpha: r.background.alpha * r.opacity });
        container.addChild(bg);
      }
      container.addChild(text);
      // 填充/描边各自承担颜色 alpha；Text 只承担共同 opacity。
      text.alpha = r.opacity;
      container.position.set(visual.x * this.cell, visual.y * this.cell);
    } catch {
      // 降级：半成品精确销毁（text 私有 canvas 源四标志），整批不中断
      text?.destroy({ style: true, texture: true, textureSource: true });
      container.destroy({ children: true, context: true });
      return false;
    }
    this.root.addChild(container);
    this.entries.push({ container, text });
    return true;
  }
}
