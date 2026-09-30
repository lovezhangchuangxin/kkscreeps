/**
 * 渲染半件 join（自然物家族）——PartDecl.values 的 pixi 对应物（create/apply）。
 * 规格常量直引 specs.ts；节点构造遵守：共享 atlas/standalone 纹理绝不带
 * texture/textureSource 销毁（ObjectsStage 统一拆树销毁）。
 */
import { Container, Graphics, Sprite, Text, Texture } from 'pixi.js';
import type { PartRenderer } from './part-decl.js';
import {
  ADD_BLEND,
  CONTAINER_SPEC,
  DECAY_SPEC,
  DEPOSIT_SPEC,
  KEEPER_LAIR_SPEC,
  MINERAL_SPEC,
  PORTAL_SPEC,
  LIGHTING_MEMBER_BLEND,
  SOURCE_SPEC,
} from './specs.js';
import {
  gfxCircle,
  gfxRoundRect,
  makeGlow,
  makeText,
  redraw,
  setSpriteSize,
  vBool,
  vNum,
  vStr,
} from './draw-helpers.js';

/** join 构建上下文（atlas 帧 + glow 独立纹理；缺帧降级隐藏）。 */
export interface JoinAssets {
  readonly frames: ReadonlyMap<string, Texture>;
  readonly glow: Texture | null;
}

export type PartJoin = Record<string, PartRenderer>;

function sprite(
  assets: JoinAssets,
  name: string,
  width: number,
  height: number | undefined,
  setup?: (s: Sprite) => void,
): Container {
  const s = assets.frames.get(name)
    ? (() => {
        const sp = new Sprite(assets.frames.get(name)!);
        sp.anchor.set(0.5, 0.5);
        setSpriteSize(sp, width, height);
        return sp;
      })()
    : new Container(); // 缺帧降级：空件（无 atlas 占位模式）
  setup?.(s as Sprite);
  return s;
}

// ---- source（source.metadata.js） ----

export function joinSource(assets: JoinAssets): PartJoin {
  return {
    frame: {
      // payload tint 0x595026 × [白描边 + 0x111111 填充]（source.metadata.js:35-63）
      create: () => {
        const g = new Graphics();
        g.roundRect(
          -SOURCE_SPEC.rect.size / 2,
          -SOURCE_SPEC.rect.size / 2,
          SOURCE_SPEC.rect.size,
          SOURCE_SPEC.rect.size,
          SOURCE_SPEC.rect.radius,
        );
        g.fill({ color: SOURCE_SPEC.rect.fillColor });
        g.stroke({ width: SOURCE_SPEC.rect.strokeWidth, color: 0xffffff, alpha: 1 });
        g.tint = SOURCE_SPEC.rect.strokeColor;
        return g;
      },
      apply: () => {},
    },
    energyRect: {
      // tint 0xffe56d × 白填充，尺寸=energySize（source.metadata.js:89-109）
      create: () => {
        const g = new Graphics();
        g.tint = SOURCE_SPEC.energy.fillColor;
        return g;
      },
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const size = vNum(v, 'size');
        if (size > 0) {
          gfxRoundRect(g, -size / 2, -size / 2, size, size, SOURCE_SPEC.energy.radius, 0xffffff);
        }
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, SOURCE_SPEC.glowBig.width, SOURCE_SPEC.glowBig.height, SOURCE_SPEC.glowBig.alpha, SOURCE_SPEC.glowBig.tint),
      apply: () => {},
    },
    glowSmall: {
      create: () => makeGlow(assets.glow, SOURCE_SPEC.glowSmall.width, SOURCE_SPEC.glowSmall.height, SOURCE_SPEC.glowSmall.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
  };
}

// ---- mineral（mineral.metadata.js） ----

export function joinMineral(assets: JoinAssets): PartJoin {
  return {
    disc: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxCircle(g, MINERAL_SPEC.radius, {
          fill: vNum(v, 'bg'),
          stroke: vNum(v, 'fg'),
          strokeWidth: MINERAL_SPEC.strokeWidth,
        });
      },
    },
    symbol: {
      create: () => makeText({
        text: '',
        style: {
          align: 'center',
          fill: 0xffffff,
          fontFamily: MINERAL_SPEC.text.fontFamily,
          fontSize: MINERAL_SPEC.text.fontSize,
          fontWeight: MINERAL_SPEC.text.fontWeight,
        },
        anchorX: 0.5,
        anchorY: 0.5,
      }),
      apply: (v, node) => {
        const t = node as unknown as Text;
        t.text = vStr(v, 'text');
        t.style.fill = vNum(v, 'fg');
      },
    },
    glowSmall: {
      create: () => makeGlow(assets.glow, MINERAL_SPEC.glowSmall.width, MINERAL_SPEC.glowSmall.height, MINERAL_SPEC.glowSmall.alpha),
      apply: () => {},
    },
    glowBig: {
      create: () => new Container(),
      apply: (v, node) => {
        // tint 随矿物色（mineral.metadata.js:110-119）
        if (node.children.length === 0) {
          node.addChild(makeGlow(assets.glow, MINERAL_SPEC.glowBig.width, MINERAL_SPEC.glowBig.height, MINERAL_SPEC.glowBig.alpha, vNum(v, 'tint')));
        }
      },
    },
  };
}

// ---- 掉落资源（energy.metadata.js + resourceCircle.js） ----

export function joinResource(assets: JoinAssets): PartJoin {
  void assets; // 圆形程序绘制，无帧依赖（签名与 registry 装配一致）
  return {
    circle: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const radius = vNum(v, 'radius');
        if (radius > 0) {
          g.circle(0, 0, radius).fill({ color: vNum(v, 'color') });
        }
      },
    },
    glow: {
      // lighting 层同径同色圆（energy.metadata.js:10-19）——payload tint:0xFFFFFF
      // 被 resourceCircle.js:50-56 重建 payload 时丢弃，运行时实为资源色（评审 A-B7）
      create: () => {
        const g = new Graphics();
        g.blendMode = LIGHTING_MEMBER_BLEND;
        return g;
      },
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const radius = vNum(v, 'radiusLighting');
        if (radius > 0) {
          g.circle(0, 0, radius).fill({ color: vNum(v, 'color') });
        }
      },
    },
  };
}

// ---- deposit（deposit.metadata.js） ----

export function joinDeposit(assets: JoinAssets): PartJoin {
  return {
    sprites: {
      // fill 在下、主贴图在上（声明序 deposit.metadata.js:77-97）
      create: () => {
        const root = new Container();
        const fill = sprite(assets, 'deposit-biomass-fill', DEPOSIT_SPEC.size, DEPOSIT_SPEC.size);
        fill.label = 'deposit-fill';
        const main = sprite(assets, 'deposit-biomass', DEPOSIT_SPEC.size, DEPOSIT_SPEC.size);
        main.label = 'deposit-main';
        root.addChild(fill, main);
        return root;
      },
      apply: (v, node) => {
        const kind = vStr(v, 'kind');
        const fill = node.getChildByLabel('deposit-fill');
        const main = node.getChildByLabel('deposit-main');
        const fillTex = assets.frames.get(`deposit-${kind}-fill`);
        const mainTex = assets.frames.get(`deposit-${kind}`);
        if (fill instanceof Sprite && fillTex) {
          fill.texture = fillTex;
          fill.tint = vNum(v, 'color');
        }
        if (main instanceof Sprite && mainTex) {
          main.texture = mainTex;
          main.tint = vNum(v, 'color');
        }
        if (fill instanceof Sprite) fill.alpha = vNum(v, 'fillAlpha');
      },
    },
    glow: {
      create: () => {
        const root = new Container();
        const main = sprite(assets, 'deposit-biomass', DEPOSIT_SPEC.size, DEPOSIT_SPEC.size);
        main.label = 'deposit-lighting';
        main.blendMode = LIGHTING_MEMBER_BLEND;
        const glow = makeGlow(assets.glow, DEPOSIT_SPEC.glow.width, DEPOSIT_SPEC.glow.height, DEPOSIT_SPEC.glow.alpha);
        glow.label = 'deposit-glow';
        root.addChild(main, glow);
        return root;
      },
      apply: (v, node) => {
        const kind = vStr(v, 'kind');
        const main = node.getChildByLabel('deposit-lighting');
        const tex = assets.frames.get(`deposit-${kind}`);
        if (main instanceof Sprite && tex) main.texture = tex;
        const glow = node.getChildByLabel('deposit-glow');
        if (glow) (glow.getChildAt(0) as Sprite).tint = vNum(v, 'tint');
      },
    },
  };
}

// ---- container（container.metadata.js） ----

export function joinContainer(assets: JoinAssets): PartJoin {
  return {
    frame: {
      create: () => {
        const root = new Container();
        const border = sprite(assets, 'rectangle', CONTAINER_SPEC.border.width, CONTAINER_SPEC.border.height);
        border.label = 'container-border';
        (border as Sprite).tint = CONTAINER_SPEC.border.tint;
        const internal = sprite(assets, 'rectangle', CONTAINER_SPEC.internalBorder.width, CONTAINER_SPEC.internalBorder.height);
        internal.label = 'container-internal';
        (internal as Sprite).tint = CONTAINER_SPEC.internalBorder.tint;
        root.addChild(border, internal);
        return root;
      },
      apply: () => {},
    },
    bars: {
      // 官方 pivot.y=高 ⇒ 锚点底边（object.js:78-84 pivot/scale 换算=纹理高）
      create: () => {
        const root = new Container();
        for (const [key, color] of [
          ['other', CONTAINER_SPEC.colors.other],
          ['power', CONTAINER_SPEC.colors.power],
          ['energy', CONTAINER_SPEC.colors.energy],
        ] as const) {
          const bar = sprite(assets, 'rectangle', CONTAINER_SPEC.barWidth, 1);
          bar.label = `bar-${key}`;
          (bar as Sprite).tint = color;
          (bar as Sprite).anchor.set(0.5, 1);
          bar.position.set(0, CONTAINER_SPEC.barY);
          root.addChild(bar);
        }
        return root;
      },
      apply: (v, node) => {
        for (const key of ['other', 'power', 'energy'] as const) {
          const bar = node.getChildByLabel(`bar-${key}`) as Sprite | null;
          if (!bar) continue;
          const h = vNum(v, `${key}H`);
          bar.visible = vBool(v, `has${key[0]!.toUpperCase()}${key.slice(1)}`);
          setSpriteSize(bar, CONTAINER_SPEC.barWidth, Math.max(h, 0.001));
        }
      },
    },
    glow: {
      create: () => makeGlow(assets.glow, CONTAINER_SPEC.glow.width, CONTAINER_SPEC.glow.height, CONTAINER_SPEC.glow.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
  };
}

// ---- tombstone / ruin（tombstone/ruin.metadata.js） ----

function joinDecay(assets: JoinAssets, borderTexture: string): PartJoin {
  return {
    sprites: {
      create: () => {
        const root = new Container();
        const border = sprite(assets, borderTexture, DECAY_SPEC.size, DECAY_SPEC.size);
        border.label = 'decay-border';
        (border as Sprite).blendMode = ADD_BLEND;
        const resource = sprite(assets, 'tombstone-resource', DECAY_SPEC.size, DECAY_SPEC.size);
        resource.label = 'decay-resource';
        resource.alpha = DECAY_SPEC.resourceAlpha;
        root.addChild(border, resource);
        return root;
      },
      apply: (v, node) => {
        const border = node.getChildByLabel('decay-border') as Sprite;
        border.alpha = vNum(v, 'borderAlpha');
        const resource = node.getChildByLabel('decay-resource') as Sprite;
        resource.tint = vNum(v, 'resourceTint');
      },
    },
    glow: {
      create: () => sprite(assets, 'tombstone-resource', DECAY_SPEC.size, DECAY_SPEC.size, (s) => {
        s.blendMode = LIGHTING_MEMBER_BLEND;
      }),
      apply: () => {},
    },
  };
}

export const joinTombstone = (assets: JoinAssets): PartJoin => joinDecay(assets, 'tombstone-border');
export const joinRuin = (assets: JoinAssets): PartJoin => joinDecay(assets, 'ruin');

// ---- portal / keeperLair（portal/keeperLair.metadata.js：底盘+双波，波动画 R5） ----

function joinWaveFamily(assets: JoinAssets, spec: {
  baseCircle: { radius: number; color: number; alpha: number };
  wave: { radius: number; color: number; alpha: number };
  wave2: { radius: number; color: number; alpha: number };
  glowBig: { width: number; height: number; alpha: number; tint?: number };
  glowSmall: { width: number; height: number; alpha: number; tint?: number };
}): PartJoin {
  return {
    disc: {
      create: () => gfxCircle(new Graphics(), spec.baseCircle.radius, { fill: spec.baseCircle.color, fillAlpha: spec.baseCircle.alpha }),
      apply: () => {},
    },
    wave1: {
      create: () => gfxCircle(new Graphics(), spec.wave.radius, { fill: spec.wave.color, fillAlpha: spec.wave.alpha }),
      apply: () => {},
    },
    wave2: {
      create: () => gfxCircle(new Graphics(), spec.wave2.radius, { fill: spec.wave2.color, fillAlpha: spec.wave2.alpha }),
      apply: () => {},
    },
    glowBig: {
      create: () => makeGlow(assets.glow, spec.glowBig.width, spec.glowBig.height, spec.glowBig.alpha, spec.glowBig.tint),
      apply: () => {},
    },
    glowSmall: {
      create: () => makeGlow(assets.glow, spec.glowSmall.width, spec.glowSmall.height, spec.glowSmall.alpha, spec.glowSmall.tint),
      apply: () => {},
    },
  };
}

export const joinPortal = (assets: JoinAssets): PartJoin => joinWaveFamily(assets, PORTAL_SPEC);
export const joinKeeperLair = (assets: JoinAssets): PartJoin => joinWaveFamily(assets, KEEPER_LAIR_SPEC);
