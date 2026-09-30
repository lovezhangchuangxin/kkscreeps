import type { Container } from 'pixi.js';
import type { LightingPass } from '../core/lighting.js';

/** 同一对象分散到 lighting/effects 的部件，共用位移与父级淡出语义。
 * 部件自身的 alpha/visible 仍由 join 管理；本组不拥有节点或纹理。 */
export class OverlayGroup {
  private readonly members = new Map<Container, boolean>();
  private lightingCount = 0;

  constructor(private readonly lighting: LightingPass) {}

  add(node: Container, inLighting = false): void {
    if (this.members.has(node)) return;
    this.members.set(node, inLighting);
    if (inLighting) this.lightingCount++;
  }

  remove(node: Container): void {
    if (this.members.get(node)) this.lightingCount--;
    this.members.delete(node);
  }

  moveTo(x: number, y: number): void {
    for (const node of this.members.keys()) node.position.set(x, y);
    this.markLightingDirty();
  }

  /** 捕捉局部 alpha，使 safemode/受击罩等部件透明度与父级淡出相乘。 */
  captureFade(): (alpha: number) => void {
    const bases = [...this.members.keys()].map((node) => [node, node.alpha] as const);
    return (alpha) => {
      for (const [node, base] of bases) node.alpha = base * alpha;
      this.markLightingDirty();
    };
  }

  clear(): void {
    this.members.clear();
    this.lightingCount = 0;
  }

  private markLightingDirty(): void {
    if (this.lightingCount > 0) this.lighting.markDirty();
  }
}
