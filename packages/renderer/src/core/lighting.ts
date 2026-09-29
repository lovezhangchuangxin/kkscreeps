/**
 * lighting 离屏 pass（G2：真 pass 优先）。
 *
 * 官方语义（metadata/index.js lighting 层）：层挂 PassThroughFilter 以
 * MULTIPLY 合成回场景；成员=0x808080 环境光（normal 混合）+ 各类明暗件
 * （官方经 display 钩子把成员改 SCREEN，_overrideBlend 件保持 MULTIPLY）。
 * v8 无 pixi-layers——等价实现：成员容器离屏渲染到 RenderTexture（容器内
 * 混合模式自然生效），输出纹理以 MULTIPLY 精灵盖回场景（objects 与 effects
 * 之间的固定槽位）。内容世界对齐（非屏幕对齐）——地形件静态，仅 dirty 时
 * 重渲，平移/缩放零成本。
 *
 * 降级路径（G2 登记）：离屏渲染不可用时 display 隐藏（仅无全局压暗，亮斑
 * 类成员若有则走场景直加 ADD——R2 仅地形件，降级即无 lighting）。
 */
import { Container, Graphics, RenderTexture, Sprite, Texture } from 'pixi.js';
import { TERRAIN_SPEC } from './terrain-spec.js';

export class LightingPass {
  /** 离屏内容容器（世界坐标；ambient + 明暗件）。不进场景树。 */
  readonly content = new Container({ label: 'lighting-content' });
  /** MULTIPLY 输出精灵（世界坐标，挂场景层栈）。 */
  readonly display: Sprite;
  private rt: RenderTexture | null = null;
  private dirty = true;
  private degraded = false;

  constructor(view: number, cell: number) {
    const half = cell / 2;
    const ambient = new Graphics()
      .rect(-half, -half, view, view)
      .fill({ color: TERRAIN_SPEC.lighting.ambient, alpha: TERRAIN_SPEC.lighting.ambientAlpha });
    ambient.label = 'lighting-ambient';
    this.content.addChild(ambient);

    this.display = new Sprite(Texture.EMPTY);
    this.display.label = 'lighting-display';
    this.display.position.set(-half, -half);
    this.display.setSize(view, view);
    this.display.blendMode = 'multiply';
    this.display.visible = false;
  }

  /** 加入明暗件（世界坐标）。blend 由调用方按 TERRAIN_SPEC.wall.*InLayerBlend 设置。 */
  addMember(member: Container): void {
    this.content.addChild(member);
    this.dirty = true;
  }

  /** 清空全部成员（地形重建时；ambient 由 LightingPass 自持不在此列）。 */
  clearMembers(): void {
    while (this.content.children.length > 1) {
      const child = this.content.removeChildAt(this.content.children.length - 1)!;
      child.destroy({ children: true, texture: true, textureSource: true });
    }
    this.dirty = true;
  }

  markDirty(): void {
    this.dirty = true;
  }

  /** 确保 RT 与渲染器匹配并按需重渲（ticker 驱动；dirty 才有成本）。
   * 内容为世界尺度，渲进 renderSize² RT 前按 view/renderSize 缩放（评审 A1：
   * 缩放缺失会把成员裁剪到世界 [0,renderSize]² 并放大 view/renderSize 倍）。 */
  update(renderer: { render(options: { container: Container; target: RenderTexture; clear?: boolean }): void }, renderSize: number, view: number): void {
    if (this.degraded || !this.dirty) return;
    try {
      if (!this.rt || this.rt.width !== renderSize || this.rt.height !== renderSize) {
        this.rt?.destroy(true);
        this.rt = RenderTexture.create({ width: renderSize, height: renderSize });
        this.display.texture = this.rt;
      }
      const previousScale = this.content.scale.x;
      this.content.scale.set(renderSize / view);
      try {
        renderer.render({ container: this.content, target: this.rt, clear: true });
      } finally {
        this.content.scale.set(previousScale);
      }
      this.display.visible = true;
      this.dirty = false;
    } catch (e) {
      // G2 降级：离屏不可用（如 WebGPU 能力缺口）→ 无全局压暗，登记灰区。
      // 降级必须留痕（评审 C3：静默 latch 会把编程错误伪装成能力缺口）。
      console.warn('[renderer] lighting pass 降级（G2）:', e);
      this.degraded = true;
      this.display.visible = false;
    }
  }

  get isDegraded(): boolean {
    return this.degraded;
  }

  destroy(): void {
    // 成员纹理均为烘焙私有（canvas 产物），显式回收（评审 S3：与 clearMembers 对称）
    this.rt?.destroy(true);
    this.content.destroy({ children: true, texture: true, textureSource: true });
    this.display.destroy();
  }
}
