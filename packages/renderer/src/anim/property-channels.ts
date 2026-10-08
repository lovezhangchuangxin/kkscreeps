import type { Container } from 'pixi.js';

/** A join owns the base values. Ambient/action players own multiplicative
 * channels. Restoring before a join prevents animated values becoming its base.
 * Writes are gated, including dirty callbacks for lighting members. */
export class PropertyChannels {
  private alpha: number;
  private sx: number;
  private sy: number;
  private rotation: number;
  private tint: number;
  private tintValue?: number;
  private alphaFactor = 1;
  private scaleFactor = 1;
  private rotationOffset = 0;
  constructor(
    readonly node: Container,
    private readonly dirty: () => void = () => {},
  ) {
    this.alpha = node.alpha;
    this.sx = node.scale.x;
    this.sy = node.scale.y;
    this.rotation = node.rotation;
    this.tint = node.tint;
  }
  restore(): void {
    this.node.alpha = this.alpha;
    this.node.scale.set(this.sx, this.sy);
    this.node.rotation = this.rotation;
    this.node.tint = this.tint;
  }
  capture(): void {
    this.alpha = this.node.alpha;
    this.sx = this.node.scale.x;
    this.sy = this.node.scale.y;
    this.rotation = this.node.rotation;
    this.tint = this.node.tint;
    this.write();
  }
  sample(alpha = 1, scale = 1, rotation = 0, tint?: number): void {
    this.alphaFactor = alpha;
    this.scaleFactor = scale;
    this.rotationOffset = rotation;
    this.tintValue = tint;
    this.write();
  }
  setRotation(rotation: number): void {
    this.rotation = rotation;
    this.write();
  }
  invalidate(): void {
    this.dirty();
  }
  reset(): void {
    this.sample();
  }
  /** End a rotation channel at its sampled angle; later cycles start here. */
  commit(): void {
    this.alphaFactor = this.scaleFactor = 1;
    this.rotationOffset = 0;
    this.tintValue = undefined;
    this.capture();
  }
  private write(): void {
    const a = this.alpha * this.alphaFactor;
    const x = this.sx * this.scaleFactor,
      y = this.sy * this.scaleFactor;
    const r = this.rotation + this.rotationOffset;
    const tint = this.tintValue ?? this.tint;
    if (
      this.node.alpha === a &&
      this.node.scale.x === x &&
      this.node.scale.y === y &&
      this.node.rotation === r &&
      this.node.tint === tint
    )
      return;
    this.node.alpha = a;
    this.node.scale.set(x, y);
    this.node.rotation = r;
    this.node.tint = tint;
    this.dirty();
  }
}

/** Movement and an action's lunge compose once, for root and all overlays. */
export class PositionChannels {
  private x: number;
  private y: number;
  private ox = 0;
  private oy = 0;
  constructor(
    x: number,
    y: number,
    private readonly write: (x: number, y: number) => void,
  ) {
    this.x = x;
    this.y = y;
  }
  get base(): Readonly<{ x: number; y: number }> {
    return { x: this.x, y: this.y };
  }
  setBase(x: number, y: number): void {
    this.x = x;
    this.y = y;
    this.apply();
  }
  setOffset(x: number, y: number): void {
    this.ox = x;
    this.oy = y;
    this.apply();
  }
  private apply(): void {
    this.write(this.x + this.ox, this.y + this.oy);
  }
}
