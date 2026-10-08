/**
 * RoomVisual 渲染舞台（R6）：五原语（line/rect/circle/poly/text）→ Graphics/Text。
 * 挂 RoomScene root 顶（effects 之上——visuals 是标注面，官方观感恒在对象/
 * 特效之上且不受 lighting 压暗；z 序黑盒推断登记 G1）。
 *
 * 全量替换语义：按输入槽位复用同类节点，重绘几何；删减/变类即时销毁，
 * 不保留 detached 池。typical ≤100 条；同引用短路
 * （FeedChange.visuals 仓库原引用跨增量稳定）。Text 自绘 canvas 纹理是私有
 * 源，销毁走精确四标志（C-S2/R3 纪律）；其余 Graphics 无纹理。
 *
 * 样式默认值/颜色/线型解析在 visual-style.ts（纯逻辑半件，逐值单测）；
 * 单条渲染失败（无 canvas 测试环境/脏数据）跳过该条不崩溃（say 气泡降级同款）。
 */
import { Container, Graphics, GraphicsContext, Text } from 'pixi.js';
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
  type: RoomVisual['type'];
  container: Container;
  text: Text | null;
  background: Graphics | null;
  styleKey: string;
  geometryKey: string;
  shapes: Array<{ key: string; context: GraphicsContext }>;
}

const STROKE_CAP = { cap: 'round', join: 'round' } as const;

export class VisualStage {
  readonly root: Container;
  private current: readonly RoomVisual[] | null = null;
  private readonly entries: Array<Entry | null> = [];
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
    let applied = 0;
    for (let i = 0; i < visuals.length; i++) {
      const visual = visuals[i];
      let entry = this.entries[i] ?? null;
      if (!isRoomVisual(visual)) {
        if (entry) this.dispose(entry);
        this.entries[i] = null;
        continue;
      }
      if (entry && entry.type !== visual.type) {
        this.dispose(entry);
        entry = null;
      }
      entry ??= {
        type: visual.type,
        container:
          visual.type === 'text' ? new Container({ label: 'visual-text' }) : new Graphics(),
        text: null,
        background: null,
        styleKey: '',
        geometryKey: '',
        shapes: [],
      };
      if (!this.render(visual, entry)) {
        this.dispose(entry);
        this.entries[i] = null;
        continue;
      }
      this.entries[i] = entry;
      if (entry.container.parent !== this.root) this.root.addChildAt(entry.container, applied);
      else if (this.root.children[applied] !== entry.container)
        this.root.setChildIndex(entry.container, applied);
      applied++;
    }
    for (let i = visuals.length; i < this.entries.length; i++) {
      const entry = this.entries[i];
      if (entry) this.dispose(entry);
    }
    this.entries.length = visuals.length;
    this.applied = applied;
  }

  destroy(): void {
    this.clear();
    this.current = null;
    this.root.destroy({ children: true, context: true });
  }

  private clear(): void {
    // Pixi v8 的非空 destroy options 需 context:true；所有 Graphics 均独立持有。
    for (const entry of this.entries) if (entry) this.dispose(entry);
    this.entries.length = 0;
    this.applied = 0;
  }

  private dispose(entry: Entry): void {
    entry.text?.destroy({ style: true, texture: true, textureSource: true });
    // Detach Graphics listeners first; both private cached contexts are owned here.
    entry.container.destroy({ children: true, context: entry.shapes.length === 0 });
    for (const shape of entry.shapes) shape.context.destroy();
  }

  /** 单条失败时由调用方销毁整个半成品，包括复用的旧节点。 */
  private render(visual: RoomVisual, entry: Entry): boolean {
    try {
      if (visual.type === 'text') this.drawText(visual, entry);
      else {
        switch (visual.type) {
          case 'line': {
            const points = visual.points.map((p) => ({ x: p.x * this.cell, y: p.y * this.cell }));
            const r = resolveLine(visual.style, this.cell);
            this.shape(entry, { points, r }, (g) => this.drawLine(g, points, r));
            break;
          }
          case 'rect': {
            const r = resolveRect(visual.style, this.cell);
            this.shape(entry, { x: visual.x, y: visual.y, w: visual.w, h: visual.h, r }, (g) =>
              this.drawRect(g, visual, r),
            );
            break;
          }
          case 'circle': {
            const r = resolveCircle(visual.style, this.cell);
            this.shape(entry, { x: visual.x, y: visual.y, r }, (g) =>
              this.drawCircle(g, visual, r),
            );
            break;
          }
          case 'poly': {
            const points = visual.points.map((p) => ({ x: p.x * this.cell, y: p.y * this.cell }));
            const r = resolvePoly(visual.style, this.cell);
            this.shape(entry, { points, r }, (g) => this.drawPoly(g, points, r));
            break;
          }
        }
      }
      return true;
    } catch {
      return false;
    }
  }

  /** Only resolved primitive values enter the key; malformed/circular raw style
   * fields retain the existing resolver defaults instead of poisoning serialization. */
  private shape(entry: Entry, geometry: unknown, draw: (g: Graphics) => void): void {
    const key = JSON.stringify(geometry),
      g = entry.container as Graphics;
    const hit = entry.shapes.findIndex((shape) => shape.key === key);
    if (hit >= 0) {
      const shape = entry.shapes.splice(hit, 1)[0]!;
      entry.shapes.push(shape);
      g.context = shape.context;
      return;
    }
    // Fixed two-entry LRU: misses repaint a context instead of allocating more UIDs.
    const shape =
      entry.shapes.length === 0
        ? { key, context: g.context }
        : entry.shapes.length < 2
          ? { key, context: new GraphicsContext() }
          : entry.shapes.shift()!;
    g.context = shape.context;
    // Register before paint so a drawing failure releases this context too.
    entry.shapes.push(shape);
    g.clear();
    draw(g);
    shape.key = key;
  }

  /** 折线描边：实线=原生单路径；dashed/dotted=切段后多子路径一次 stroke。 */
  private strokePath(
    g: Graphics,
    points: readonly Pt[],
    shape: ShapeResolved,
    widthPx: number,
    closed: boolean,
  ): void {
    if (shape.stroke === null || shape.stroke.alpha <= 0 || widthPx <= 0) return;
    const style = {
      color: shape.stroke.color,
      alpha: shape.stroke.alpha * shape.opacity,
      width: widthPx,
      ...STROKE_CAP,
    };
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

  private drawLine(g: Graphics, points: Pt[], resolved: ReturnType<typeof resolveLine>): void {
    if (resolved.color.alpha * resolved.opacity <= 0 || resolved.widthPx <= 0) return;
    if (resolved.dash === 'solid') {
      g.moveTo(points[0]!.x, points[0]!.y);
      g.lineTo(points[1]!.x, points[1]!.y);
    } else {
      for (const [a, b] of dashSegments(points, resolved.dash, this.cell, resolved.widthPx))
        g.moveTo(a.x, a.y).lineTo(b.x, b.y);
    }
    g.stroke({
      color: resolved.color.color,
      alpha: resolved.color.alpha * resolved.opacity,
      width: resolved.widthPx,
      ...STROKE_CAP,
    });
  }

  private drawRect(
    g: Graphics,
    visual: Extract<RoomVisual, { type: 'rect' }>,
    r: ReturnType<typeof resolveRect>,
  ): void {
    const x = visual.x * this.cell;
    const y = visual.y * this.cell;
    const w = visual.w * this.cell;
    const h = visual.h * this.cell;
    if (r.fill !== null && r.fill.alpha > 0)
      g.rect(x, y, w, h).fill({ color: r.fill.color, alpha: r.fill.alpha * r.opacity });
    const corners: Pt[] = [
      { x, y },
      { x: x + w, y },
      { x: x + w, y: y + h },
      { x, y: y + h },
    ];
    this.strokePath(g, corners, r, r.strokePx, true);
  }

  private drawCircle(
    g: Graphics,
    visual: Extract<RoomVisual, { type: 'circle' }>,
    r: ReturnType<typeof resolveCircle>,
  ): void {
    const cx = visual.x * this.cell;
    const cy = visual.y * this.cell;
    if (r.fill !== null && r.fill.alpha > 0)
      g.circle(cx, cy, r.radiusPx).fill({ color: r.fill.color, alpha: r.fill.alpha * r.opacity });
    if (r.stroke !== null && r.stroke.alpha > 0 && r.strokePx > 0) {
      if (r.dash === 'solid') {
        g.circle(cx, cy, r.radiusPx).stroke({
          color: r.stroke.color,
          alpha: r.stroke.alpha * r.opacity,
          width: r.strokePx,
          ...STROKE_CAP,
        });
      } else {
        this.strokePath(g, circlePoints(cx, cy, r.radiusPx), r, r.strokePx, true);
      }
    }
  }

  private drawPoly(g: Graphics, pts: Pt[], r: ReturnType<typeof resolvePoly>): void {
    if (pts.length < 1) return;
    if (pts.length >= 3 && r.fill !== null && r.fill.alpha > 0) {
      // canvas 语义：开放路径填充按闭合区域计（官方 poly=折线）
      g.poly(pts).fill({ color: r.fill.color, alpha: r.fill.alpha * r.opacity });
    }
    if (pts.length >= 2) this.strokePath(g, pts, r, r.strokePx, false); // 折线开放描边（评审 GPT-P1#3）
  }

  private drawText(visual: Extract<RoomVisual, { type: 'text' }>, entry: Entry): void {
    const r = resolveText(visual.style, this.cell);
    const style = {
      fontFamily: r.fontFamily,
      fontSize: r.fontSizePx,
      fontWeight: r.fontWeight,
      fontStyle: r.fontStyle,
      fontVariant: r.fontVariant,
      fill: { color: r.color.color, alpha: r.color.alpha },
      align: r.align,
      ...(r.stroke !== null && r.stroke.alpha > 0 && r.strokePx > 0
        ? {
            stroke: {
              color: r.stroke.color,
              alpha: r.stroke.alpha,
              width: r.strokePx,
              join: 'round' as const,
            },
          }
        : {}),
    };
    const styleKey = JSON.stringify(style);
    if (!entry.text) {
      entry.text = new Text({ text: visual.text, style });
      entry.container.addChild(entry.text);
      entry.styleKey = styleKey;
    } else {
      entry.text.text = visual.text;
      if (entry.styleKey !== styleKey) {
        // Pixi style setter only detaches the old listener; this style is owned.
        const previous = entry.text.style;
        entry.text.style = style;
        previous.destroy();
        entry.styleKey = styleKey;
      }
    }
    const text = entry.text;
    // Preserve the existing G1 baseline approximation and background-middle rule.
    const ax = r.align === 'left' ? 0 : r.align === 'right' ? 1 : 0.5;
    const ay = r.verticalMiddle ? 0.5 : 1;
    text.anchor.set(ax, ay);
    if (r.background !== null && r.background.alpha > 0) {
      if (!entry.background) {
        entry.background = new Graphics();
        entry.container.addChildAt(entry.background, 0);
      }
      const bg = entry.background,
        pad = r.paddingPx,
        width = text.width,
        height = text.height;
      const key = JSON.stringify({
        ax,
        ay,
        pad,
        width,
        height,
        background: r.background,
        opacity: r.opacity,
      });
      if (entry.geometryKey !== key) {
        bg.clear();
        bg.roundRect(
          -ax * width - pad,
          -ay * height - pad,
          width + pad * 2,
          height + pad * 2,
          Math.min(pad, height / 2),
        );
        bg.fill({ color: r.background.color, alpha: r.background.alpha * r.opacity });
        entry.geometryKey = key;
      }
    } else if (entry.background) {
      entry.background.destroy({ context: true });
      entry.background = null;
      entry.geometryKey = '';
    }
    text.alpha = r.opacity;
    entry.container.position.set(visual.x * this.cell, visual.y * this.cell);
  }
}
