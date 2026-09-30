/**
 * 地形层（官方 terrain.js 同构复刻，方案 §2.2 terrain 槽位）：
 * plain 底色+双噪声平铺 → 出口四边贴边 → swamp 合并 path（填充+双白罩+双
 * noise2 ADD 滚动）→ wall 合并 path 烘焙（+bump 噪声，黑影/亮填进 lighting）。
 *
 * 烘焙路径：合并轮廓 SVG path（path-merge.ts）→ Path2D → canvas 2D 烘焙
 * （renderSize²）→ 纹理。与官方「运行时生成 SVG data-URI」不同（§2.4 坑）：
 * 无异步 img 解码，同步烘焙 + hash 门控（重绘仅当地形变化）。
 * SVG 语义映射：paint-order:stroke = 先描边后填充；ADD = canvas 'lighter'；
 * 官方 BlurFilter 黑影 = canvas filter blur（σ 语义近似，灰区表 G7 盯梢）。
 */
import { Container, Sprite, Texture, TilingSprite } from 'pixi.js';
import { bakeCanvas, textureFromBaked } from '../core/bake.js';
import { mergedOutlinePath } from '../core/path-merge.js';
import { TERRAIN_SPEC } from '../core/terrain-spec.js';
import type { LightingPass } from '../core/lighting.js';

/** 地形所需独立纹理（位图供 canvas 烘焙复用；无资产管线的占位集可缺位）。 */
export interface TerrainTextures {
  ground: { texture: Texture; bitmap?: ImageBitmap };
  'ground-mask': { texture: Texture; bitmap?: ImageBitmap };
  noise1: { texture: Texture; bitmap?: ImageBitmap };
  noise2: { texture: Texture; bitmap?: ImageBitmap };
  'exit-top': { texture: Texture };
  'exit-bottom': { texture: Texture };
  'exit-left': { texture: Texture };
  'exit-right': { texture: Texture };
}

export class TerrainStage {
  readonly root = new Container({ label: 'terrain-stage' });
  private readonly view: number;
  private readonly cell: number;
  /** 烘焙画布边长（lighting RT 同尺寸）。 */
  readonly renderSize = TERRAIN_SPEC.renderSize;
  private wallHash: string | undefined;
  private swampHash: string | undefined;
  private swampNoise: TilingSprite[] = [];

  constructor(
    private readonly textures: TerrainTextures,
    private readonly lighting: LightingPass,
    view: number,
    cell: number,
  ) {
    this.view = view;
    this.cell = cell;
    this.buildFloor();
    this.buildExits();
  }

  /** plain：底色 + ground 平铺(×3, α.3) + ground-mask 乘算(×7, α.15)。静态。
   * 层序照官方（terrain.js:455→479）：mask 先、ground 后（mask 乘算在下）。 */
  private buildFloor(): void {
    const half = this.cell / 2;
    const spec = TERRAIN_SPEC.plain;
    const floor = new Sprite(Texture.WHITE);
    floor.label = 'terrain-floor';
    floor.position.set(-half, -half);
    floor.setSize(this.view, this.view);
    floor.tint = spec.color;
    this.root.addChild(floor);

    const mask = new TilingSprite({
      texture: this.textures['ground-mask'].texture,
      width: this.view,
      height: this.view,
    });
    mask.label = 'terrain-ground-mask';
    mask.position.set(-half, -half);
    mask.tileScale.set(spec.groundMaskTileScale);
    mask.blendMode = spec.groundMaskBlend;
    mask.alpha = spec.groundMaskAlpha;
    this.root.addChild(mask);

    const ground = new TilingSprite({
      texture: this.textures.ground.texture,
      width: this.view,
      height: this.view,
    });
    ground.label = 'terrain-ground';
    ground.position.set(-half, -half);
    ground.tileScale.set(spec.groundTileScale);
    ground.alpha = spec.groundAlpha;
    this.root.addChild(ground);
  }

  /** 出口：四边 TilingSprite（ADD α0.5，tileScale=cell/纹宽）。静态。 */
  private buildExits(): void {
    const half = this.cell / 2;
    const spec = TERRAIN_SPEC.exit;
    const strips: Array<{ key: keyof TerrainTextures; x: number; y: number; w: number; h: number }> = [
      { key: 'exit-left', x: -half, y: -half, w: this.cell, h: this.view },
      { key: 'exit-top', x: -half, y: -half, w: this.view, h: this.cell },
      { key: 'exit-bottom', x: -half, y: -half + this.view - this.cell, w: this.view, h: this.cell },
      { key: 'exit-right', x: -half + this.view - this.cell, y: -half, w: this.cell, h: this.view },
    ];
    for (const { key, x, y, w, h } of strips) {
      const texture = (this.textures[key] as { texture: Texture }).texture;
      const strip = new TilingSprite({ texture, width: w, height: h });
      strip.label = `terrain-exit-${key}`;
      strip.position.set(x, y);
      // 平铺节距 = texture.frame × tileScale（v8 setUvs）——@1x 32×3.125 与
      // @2x 64×1.5625 的覆盖同为 cell=100（评审 A4 撤回后定案：几何与
      // resolution 无关，@2x 纹素密度翻倍仅更锐）
      strip.tileScale.set(this.cell / texture.width);
      strip.blendMode = spec.blend;
      strip.alpha = spec.alpha;
      this.root.addChild(strip);
    }
  }

  /**
   * 按 2500 地形码重建 swamp/wall（各自 hash 门控——不变则零成本）。
   * wall 的黑影/亮填成员进 lighting pass（先清旧成员再挂新）。
   * 返回 wall 层是否实际重建（lighting dirty 门控，评审 C2/S1）。
   */
  setTerrain(codes: ArrayLike<number>): { wallRebuilt: boolean } {
    this.rebuildSwamp(codes);
    return { wallRebuilt: this.rebuildWall(codes) };
  }

  private rebuildSwamp(codes: ArrayLike<number>): void {
    const merged = mergedOutlinePath(codes, (c) => (c & 2) !== 0, { cell: this.cell }, this.swampHash);
    if (merged.hash === this.swampHash) return;
    this.swampHash = merged.hash;
    if (merged.count === 0) {
      this.clearSwampSprites();
      return;
    }
    if (merged.path === null) return; // hash 未变，跳过重建

    this.clearSwampSprites();
    const spec = TERRAIN_SPEC.swamp;
    const half = this.cell / 2;
    // 双白罩同一形状——烘一次共享纹理（评审 C4：省 1/6 烘焙面）
    const overlayTexture = textureFromBaked(
      bakeCanvas(merged.path, this.view, this.renderSize, (ctx, p) => {
        ctx.fillStyle = spec.overlayFill;
        ctx.fill(p);
      }),
    );
    // 层序照官方 zIndex 语义（terrain.js：白罩 0 在下、base/噪声 1 在上）。
    // G8 二轮实验定案：一轮的「该序下 mask 失效」系清场跳格 bug 污染的观察
    // （stale 噪声满房裸奔同形），修复后官方序 mask 剪裁正常——灰区 G8 销账。
    const masks = [0, 1].map((i) => {
      const overlay = new Sprite(overlayTexture);
      overlay.label = `terrain-swamp-overlay-${i}`;
      overlay.position.set(-half, -half);
      overlay.setSize(this.view, this.view);
      overlay.alpha = spec.overlayAlpha;
      this.root.addChild(overlay);
      return overlay;
    });

    const base = new Sprite(
      textureFromBaked(
        bakeCanvas(merged.path, this.view, this.renderSize, (ctx, p) => {
          // paint-order:stroke——描边在下
          ctx.lineWidth = spec.strokeWidth;
          ctx.strokeStyle = spec.fill;
          ctx.stroke(p);
          ctx.fillStyle = spec.fill;
          ctx.fill(p);
        }),
      ),
    );
    base.label = 'terrain-swamp-base';
    base.position.set(-half, -half);
    base.setSize(this.view, this.view);
    base.alpha = spec.alpha;
    this.root.addChild(base);

    spec.noiseTileScales.forEach((tileScale, i) => {
      const noise = new TilingSprite({
        texture: this.textures.noise2.texture,
        width: this.view,
        height: this.view,
      });
      noise.label = `terrain-swamp-noise-${i}`;
      noise.position.set(-half, -half);
      noise.tileScale.set(tileScale);
      noise.blendMode = spec.noiseBlend;
      noise.alpha = spec.noiseAlpha;
      noise.tint = spec.noiseTint;
      noise.mask = masks[i]!;
      this.root.addChild(noise);
      this.swampNoise.push(noise);
    });
  }

  private clearSwampSprites(): void {
    // 先摘 mask 再销毁（评审 C6 预言的脆弱点实证：v8 MaskFilter 在 mask 精灵
    // 已毁/纹理置空后于渲染期读 null.destroyed 直接抛错且打断渲染帧）
    for (const child of this.root.children) {
      if (child.label.includes('swamp-noise')) child.mask = null;
    }
    // 快照迭代（评审 A/C 复审 N1：destroy→removeFromParent 会 splice 活数组，
    // 直接 for...of 跳格——每轮残留 mask 已摘的噪声与持已毁纹理的白罩）
    for (const child of [...this.root.children]) {
      // 噪声层持共享 atlas 纹理（noise2）——只拆显示对象不销毁纹理（评审 B2/C1：
      // 误毁共享源后再次进入沼泽房即渲染损坏）；base/白罩为烘焙私有 canvas 纹理。
      if (!child.label.startsWith('terrain-swamp')) continue;
      if (child.label.includes('swamp-noise')) child.destroy();
      else child.destroy({ texture: true, textureSource: true });
    }
    this.swampNoise = [];
  }

  private clearWallSprites(): void {
    for (const child of this.root.children.filter((c) => c.label === 'terrain-wall')) {
      child.destroy({ texture: true, textureSource: true });
    }
  }

  private rebuildWall(codes: ArrayLike<number>): boolean {
    const merged = mergedOutlinePath(codes, (c) => (c & 1) !== 0, { cell: this.cell }, this.wallHash);
    if (merged.hash === this.wallHash) return false;
    this.wallHash = merged.hash;
    if (merged.count === 0) {
      this.clearWallSprites();
      this.lighting.clearMembers();
      return true;
    }
    if (merged.path === null) return false; // hash 未变，跳过重建

    this.clearWallSprites();
    this.lighting.clearMembers();

    const spec = TERRAIN_SPEC.wall;
    const half = this.cell / 2;
    const sizeRatio = this.renderSize / this.view;

    // 主墙纹理：#111 填充 + #000 描边（paint-order:stroke）+ noise1 bump（ADD、
    // clip 到 path）。bump 比例（评审 A2 的空间换算）：官方 TilingSprite
    // tileScale=8×sizeRatio（世界 px/纹素），canvas 烘焙在缩放后的 user 空间
    // （view 跨度）里，纹素的世界尺寸 512×8×sizeRatio 折算到 user 尺度恰为
    // 512×8×sizeRatio——即 drawScale=8×sizeRatio；循环上界是 user 跨度 view
    // （此前误写 renderSize 只铺 41% 画布）。
    const wallCanvas = bakeCanvas(merged.path, this.view, this.renderSize, (ctx, p) => {
      ctx.lineWidth = spec.strokeWidth;
      ctx.strokeStyle = spec.stroke;
      ctx.stroke(p);
      ctx.fillStyle = spec.fill;
      ctx.fill(p);
      ctx.save();
      ctx.globalCompositeOperation = 'lighter';
      ctx.globalAlpha = spec.bumpAlpha;
      ctx.clip(p);
      const noise = this.textures.noise1.bitmap;
      if (noise) {
        const drawScale = spec.bumpTileScaleFactor * sizeRatio;
        const tile = noise.width * drawScale;
        for (let x = 0; x < this.view; x += tile) {
          for (let y = 0; y < this.view; y += tile) {
            ctx.drawImage(noise, x, y, tile, tile);
          }
        }
      }
      ctx.restore();
    });
    const wall = new Sprite(textureFromBaked(wallCanvas));
    wall.label = 'terrain-wall';
    wall.position.set(-half, -half);
    wall.setSize(this.view, this.view);
    this.root.addChild(wall);

    // lighting 成员一：黑影（blur 后 MULTIPLY，压暗墙周）
    const blurPx = this.renderSize * spec.shadowBlurRatio;
    const shadowCanvas = bakeCanvas(merged.path, this.view, this.renderSize, (ctx, p) => {
      ctx.filter = `blur(${blurPx}px)`;
      ctx.fillStyle = spec.shadowFill;
      ctx.fill(p);
    });
    const shadow = new Sprite(textureFromBaked(shadowCanvas));
    shadow.label = 'lighting-wall-shadow';
    shadow.position.set(-half, -half);
    shadow.setSize(this.view, this.view);
    shadow.blendMode = spec.shadowInLayerBlend;
    this.lighting.addMember(shadow);

    // lighting 成员二：#808080 亮填（官方 display 钩子 → SCREEN，提亮墙体）
    const lightCanvas = bakeCanvas(merged.path, this.view, this.renderSize, (ctx, p) => {
      ctx.lineWidth = spec.strokeWidth;
      ctx.strokeStyle = spec.lightStroke;
      ctx.stroke(p);
      ctx.fillStyle = spec.lightFill;
      ctx.fill(p);
    });
    const lightFill = new Sprite(textureFromBaked(lightCanvas));
    lightFill.label = 'lighting-wall-fill';
    lightFill.position.set(-half, -half);
    lightFill.setSize(this.view, this.view);
    lightFill.blendMode = spec.fillInLayerBlend;
    this.lighting.addMember(lightFill);
    return true;
  }

  /** swamp 噪声滚动（ticker 驱动；framesDelta = deltaMS/16.667 的 60fps 基准帧数）。 */
  update(framesDelta: number): void {
    const drift = TERRAIN_SPEC.swamp.noiseDrift;
    for (let i = 0; i < this.swampNoise.length; i++) {
      const noise = this.swampNoise[i]!;
      noise.tilePosition.x += framesDelta * drift[i * 2]!;
      noise.tilePosition.y += framesDelta * drift[i * 2 + 1]!;
    }
  }

  destroy(): void {
    // 先按所有权清掉烘焙私有纹理；共享 atlas 纹理只拆树不销毁
    // （评审 B3：Texture.WHITE/atlas standalone 是进程级共享源）
    this.clearSwampSprites();
    this.clearWallSprites();
    this.root.destroy({ children: true });
  }
}
