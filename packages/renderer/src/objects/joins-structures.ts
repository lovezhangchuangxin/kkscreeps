/**
 * 渲染半件 join（结构家族）。规格常量直引 specs.ts；语义锚 file:line 见各件。
 * 冻结相位约定：blink 模板冻结于可见相位（specs 注释；R5 接管时序）。
 */
import { Container, Graphics, Sprite, Text, Texture } from 'pixi.js';
import { SPAWN_BADGE_COLOR } from './player-color.js';
import {
  ADD_BLEND,
  CONSTRUCTED_WALL_SPEC,
  CONSTRUCTION_SITE_SPEC,
  CONTROLLER_SPEC,
  EXTRACTOR_SPEC,
  EXTENSION_SPEC,
  FACTORY_SPEC,
  FLAG_SPEC,
  INVADER_CORE_SPEC,
  LAB_SPEC,
  LINK_SPEC,
  NUKER_SPEC,
  OBSERVER_SPEC,
  POWER_BANK_SPEC,
  POWER_SPAWN_SPEC,
  RAMPART_SPRITE_SPEC,
  SPAWN_SPEC,
  STORAGE_SPEC,
  TERMINAL_SPEC,
  TOWER_SPEC,
  DISABLED_SPEC,
} from './specs.js';
import {
  gfxArcStroke,
  gfxCircle,
  gfxEllipse,
  gfxPoly,
  gfxRect,
  gfxRoundRect,
  gfxSiteProgress,
  makeGlow,
  makeText,
  redraw,
  setSpriteSize,
  vBool,
  vNum,
  vNums,
  vStr,
} from './draw-helpers.js';
import type { JoinAssets, PartJoin } from './joins-naturals.js';

function sprite(
  assets: JoinAssets,
  name: string,
  width: number,
  height: number | undefined,
  setup?: (s: Sprite) => void,
): Container {
  const tex = assets.frames.get(name);
  if (!tex) return new Container();
  const sp = new Sprite(tex);
  sp.anchor.set(0.5, 0.5);
  setSpriteSize(sp, width, height);
  setup?.(sp);
  return sp;
}

/** 尺寸随值变化的帧精灵（apply 换纹理+尺寸+着色）。 */
function dynamicSprite(
  assets: JoinAssets,
  initialName: string,
  width: number,
  height: number | undefined,
): Sprite {
  const tex = assets.frames.get(initialName) ?? Texture.EMPTY;
  const sp = new Sprite(tex);
  sp.anchor.set(0.5, 0.5);
  setSpriteSize(sp, width, height);
  return sp;
}

// ---- extension（extension.metadata.js） ----

export function joinExtension(assets: JoinAssets): PartJoin {
  return {
    border: {
      // border 贴图与 body 尺寸同随容量档位（extension.metadata.js:33-76）
      create: () => {
        const root = new Container();
        const border = dynamicSprite(assets, EXTENSION_SPEC.borders.small, 100, 100);
        border.label = 'ext-border';
        const body = sprite(assets, EXTENSION_SPEC.base.texture, EXTENSION_SPEC.sizes.small, EXTENSION_SPEC.sizes.small);
        body.label = 'ext-body';
        root.addChild(border, body);
        return root;
      },
      apply: (v, node) => {
        const tier = vStr(v, 'tier') as 'small' | 'medium' | 'large';
        const border = node.getChildByLabel('ext-border') as Sprite;
        // 官方精确相等判定（A-S5）：无匹配档（cap=150/300 等）无边框件
        const borderTier = vStr(v, 'border') as 'small' | 'medium' | 'large' | '';
        const borderTex = borderTier ? assets.frames.get(EXTENSION_SPEC.borders[borderTier]) : undefined;
        if (borderTex) border.texture = borderTex;
        border.visible = !!borderTex;
        border.tint = vNum(v, 'tint');
        const size = EXTENSION_SPEC.sizes[tier];
        const body = node.getChildByLabel('ext-body') as Sprite;
        setSpriteSize(body, size, size);
      },
    },
    circle: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const radius = vNum(v, 'radius');
        if (radius > 0) {
          g.circle(0, 0, radius).fill({ color: EXTENSION_SPEC.circleColor });
        }
        node.scale.set(vNum(v, 'scale'));
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, EXTENSION_SPEC.glowBase.width, EXTENSION_SPEC.glowBase.height, EXTENSION_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowTier: {
      create: () => new Container(),
      apply: (v, node) => {
        if (node.children.length === 0) {
          node.addChild(makeGlow(assets.glow, 1, 1, 1));
        }
        const inner = node.getChildAt(0).getChildAt(0) as Sprite | undefined;
        const width = vNum(v, 'width');
        if (inner && width > 0) {
          setSpriteSize(inner, width, vNum(v, 'height'));
          inner.alpha = vNum(v, 'alpha');
        }
        node.visible = v !== null && width > 0;
      },
    },
  };
}

// ---- storage（storage.metadata.js） ----

export function joinStorage(assets: JoinAssets): PartJoin {
  return {
    border: {
      create: () => sprite(assets, 'storage-border', STORAGE_SPEC.size, STORAGE_SPEC.size),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    body: {
      create: () => sprite(assets, 'storage', STORAGE_SPEC.size, STORAGE_SPEC.size),
      apply: () => {},
    },
    bars: {
      // 三条竖直堆叠条：白(全部)→红(power+energy)→黄(energy)，锚点底边 y=70
      create: () => {
        const root = new Container();
        for (const [key, color] of [
          ['other', STORAGE_SPEC.colors.other],
          ['power', STORAGE_SPEC.colors.power],
          ['energy', STORAGE_SPEC.colors.energy],
        ] as const) {
          const bar = dynamicSprite(assets, 'rectangle', STORAGE_SPEC.barWidth, 1);
          bar.label = `bar-${key}`;
          bar.tint = color;
          bar.anchor.set(0.5, 1);
          bar.position.set(0, STORAGE_SPEC.totalHeight / 2);
          root.addChild(bar);
        }
        return root;
      },
      apply: (v, node) => {
        for (const key of ['other', 'power', 'energy'] as const) {
          const bar = node.getChildByLabel(`bar-${key}`) as Sprite | null;
          if (!bar) continue;
          bar.visible = vBool(v, `has${key[0]!.toUpperCase()}${key.slice(1)}`);
          setSpriteSize(bar, STORAGE_SPEC.barWidth, Math.max(vNum(v, `${key}H`), 0.001));
        }
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, STORAGE_SPEC.glowBase.width, STORAGE_SPEC.glowBase.height, STORAGE_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, STORAGE_SPEC.glowBig.width, STORAGE_SPEC.glowBig.height, STORAGE_SPEC.glowBig.alpha),
      apply: () => {},
    },
  };
}

// ---- factory（factory.metadata.js） ----

export function joinFactory(assets: JoinAssets): PartJoin {
  const size = FACTORY_SPEC.spriteSize;
  return {
    border: {
      create: () => sprite(assets, 'factory-border', size, size),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    body: {
      create: () => sprite(assets, 'factory', size, size),
      apply: () => {},
    },
    highlight: {
      create: () => sprite(assets, 'factory-highlight', size, size, (s) => {
        s.alpha = FACTORY_SPEC.highlightAlpha;
      }),
      apply: (v, node) => {
        (node as Sprite).alpha = vNum(v, 'alpha');
      },
    },
    level0: {
      create: () => sprite(assets, 'factory-lvl0', size, size),
      apply: () => {},
    },
    level: {
      create: () => dynamicSprite(assets, 'factory-lvl1', size, size),
      apply: (v, node) => {
        const s = node as Sprite;
        const level = vNum(v, 'level');
        const tex = assets.frames.get(`factory-lvl${level}`);
        if (tex) s.texture = tex;
        s.visible = true;
      },
    },
    internalBorder: {
      // 条形区深灰底（factory.metadata.js:161-170；评审 A-B3）
      create: () => sprite(assets, 'rectangle', FACTORY_SPEC.containerSize, FACTORY_SPEC.containerSize, (s) => {
        s.tint = FACTORY_SPEC.colors.container;
      }),
      apply: () => {},
    },
    bars: {
      create: () => {
        const root = new Container();
        for (const [key, color] of [
          ['other', FACTORY_SPEC.colors.resource],
          ['power', FACTORY_SPEC.colors.power],
          ['energy', FACTORY_SPEC.colors.energy],
        ] as const) {
          const bar = dynamicSprite(assets, 'rectangle', FACTORY_SPEC.containerSize, 1);
          bar.label = `bar-${key}`;
          bar.tint = color;
          bar.anchor.set(0.5, 1);
          bar.position.set(0, FACTORY_SPEC.barY);
          root.addChild(bar);
        }
        return root;
      },
      apply: (v, node) => {
        for (const key of ['other', 'power', 'energy'] as const) {
          const bar = node.getChildByLabel(`bar-${key}`) as Sprite | null;
          if (!bar) continue;
          bar.visible = vBool(v, `has${key[0]!.toUpperCase()}${key.slice(1)}`);
          setSpriteSize(bar, FACTORY_SPEC.containerSize, Math.max(vNum(v, `${key}H`), 0.001));
        }
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, FACTORY_SPEC.glowBase.width, FACTORY_SPEC.glowBase.height, FACTORY_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, FACTORY_SPEC.glowBig.width, FACTORY_SPEC.glowBig.height, FACTORY_SPEC.glowBig.alpha),
      apply: () => {},
    },
  };
}

// ---- nuker（nuker.metadata.js） ----

export function joinNuker(assets: JoinAssets): PartJoin {
  return {
    border: {
      create: () => sprite(assets, 'nuker-border', NUKER_SPEC.spriteSize, NUKER_SPEC.spriteSize, (s) => {
        s.y = NUKER_SPEC.spriteY;
      }),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    body: {
      create: () => sprite(assets, 'nuker', NUKER_SPEC.spriteSize, NUKER_SPEC.spriteSize, (s) => {
        s.y = NUKER_SPEC.spriteY;
      }),
      apply: () => {},
    },
    triangle: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxPoly(g, vNums(v, 'points'), NUKER_SPEC.triangle.color);
      },
    },
    gRect: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const w = vNum(v, 'width');
        gfxRect(g, -w / 2, NUKER_SPEC.gRect.y, w, NUKER_SPEC.gRect.height, NUKER_SPEC.gRect.color);
      },
    },
    glowSmall: {
      create: () => makeGlow(assets.glow, NUKER_SPEC.glowSmall.width, NUKER_SPEC.glowSmall.height, NUKER_SPEC.glowSmall.alpha),
      apply: () => {},
    },
    glowBig: {
      create: () => makeGlow(assets.glow, NUKER_SPEC.glowBig.width, NUKER_SPEC.glowBig.height, NUKER_SPEC.glowBig.alpha),
      apply: () => {},
    },
  };
}

// ---- spawn（spawn.metadata.js） ----

export function joinSpawn(assets: JoinAssets): PartJoin {
  return {
    core: {
      // static(circle70) → [black59, badge38, arc, resourceCircle]（声明序 58-178）
      create: () => {
        const root = new Container({ label: 'spawn-core' });
        const staticCircle = gfxCircle(new Graphics(), SPAWN_SPEC.ellipse1.radius, { fill: SPAWN_SPEC.ellipse1.color });
        staticCircle.label = 'static';
        const black = gfxCircle(new Graphics(), SPAWN_SPEC.ellipse2.radius, { fill: SPAWN_SPEC.ellipse2.color });
        const badge = gfxCircle(new Graphics(), SPAWN_SPEC.badge.radius, { fill: SPAWN_BADGE_COLOR });
        badge.label = 'badge';
        const arc = new Graphics();
        arc.label = 'arc';
        const resource = gfxCircle(new Graphics(), SPAWN_SPEC.resourceCircle.radius, { fill: SPAWN_SPEC.resourceCircle.color });
        resource.label = 'resource';
        root.addChild(staticCircle, black, badge, arc, resource);
        return root;
      },
      apply: (v, node) => {
        const badge = node.getChildByLabel('badge') as Graphics;
        // badgeColor 重画代价低（值恒 0x555555 于 G9 模式）
        redraw(badge).circle(0, 0, SPAWN_SPEC.badge.radius).fill({ color: vNum(v, 'badgeColor') });
        const arc = node.getChildByLabel('arc') as Graphics;
        redraw(arc);
        gfxArcStroke(arc, SPAWN_SPEC.arc.radius, SPAWN_SPEC.arc.startAngle, vNum(v, 'arcAngle'), SPAWN_SPEC.arc.color, SPAWN_SPEC.arc.strokeWidth);
        const resource = node.getChildByLabel('resource') as Graphics;
        resource.scale.set(vNum(v, 'resourceScale'));
      },
    },
    name: {
      // displayName（action-templates/displayName.js）：Roboto 55、stroke #111 15、
      // anchor (0.5, 1.7)、effects 层
      create: () => makeText({
        text: '',
        style: {
          align: 'center',
          fill: 0xffffff,
          fontFamily: SPAWN_SPEC.name.fontFamily,
          fontSize: SPAWN_SPEC.name.fontSize,
          stroke: { color: SPAWN_SPEC.name.strokeColor, width: SPAWN_SPEC.name.strokeThickness },
        },
        anchorX: 0.5,
        anchorY: SPAWN_SPEC.name.anchorY,
      }),
      apply: (v, node) => {
        const t = node as unknown as Text;
        t.text = vStr(v, 'text');
        t.style.fill = vNum(v, 'fill');
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, SPAWN_SPEC.glowBase.width, SPAWN_SPEC.glowBase.height, SPAWN_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, SPAWN_SPEC.glowBig.width, SPAWN_SPEC.glowBig.height, SPAWN_SPEC.glowBig.alpha),
      apply: () => {},
    },
  };
}

// ---- link（link.metadata.js） ----

export function joinLink(assets: JoinAssets): PartJoin {
  return {
    border: {
      create: () => sprite(assets, 'link-border', LINK_SPEC.size, LINK_SPEC.size),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    body: {
      create: () => sprite(assets, 'link', LINK_SPEC.size, LINK_SPEC.size),
      apply: () => {},
    },
    energy: {
      create: () => sprite(assets, 'link-energy', LINK_SPEC.energySize, LINK_SPEC.energySize),
      apply: (v, node) => {
        node.scale.set(vNum(v, 'scale'));
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, LINK_SPEC.glowBase.width, LINK_SPEC.glowBase.height, LINK_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, LINK_SPEC.glowBig.width, LINK_SPEC.glowBig.height, LINK_SPEC.glowBig.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
  };
}

// ---- observer（observer.metadata.js） ----

export function joinObserver(assets: JoinAssets): PartJoin {
  return {
    disc: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxEllipse(g, OBSERVER_SPEC.border.width, OBSERVER_SPEC.border.height, {
          fill: OBSERVER_SPEC.border.color,
          stroke: vNum(v, 'tint'),
          strokeWidth: OBSERVER_SPEC.border.strokeWidth,
        });
      },
    },
    eye: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxEllipse(g, OBSERVER_SPEC.eye.width, OBSERVER_SPEC.eye.height, { fill: vNum(v, 'tint') });
      },
    },
    glow: {
      create: () => makeGlow(assets.glow, OBSERVER_SPEC.glow.width, OBSERVER_SPEC.glow.height, OBSERVER_SPEC.glow.alpha),
      apply: () => {},
    },
  };
}

// ---- powerBank（powerBank.metadata.js） ----

export function joinPowerBank(assets: JoinAssets): PartJoin {
  return {
    body: {
      create: () => sprite(assets, 'powerBank', POWER_BANK_SPEC.spriteSize, POWER_BANK_SPEC.spriteSize),
      apply: () => {},
    },
    pool: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const r = vNum(v, 'radius');
        if (r > 0) {
          gfxCircle(g, r, {
            fill: POWER_BANK_SPEC.ellipse.color,
            stroke: POWER_BANK_SPEC.ellipse.strokeColor,
            strokeWidth: POWER_BANK_SPEC.ellipse.strokeWidth,
          });
        }
      },
    },
    glow: {
      create: () => makeGlow(assets.glow, POWER_BANK_SPEC.glow.width, POWER_BANK_SPEC.glow.height, POWER_BANK_SPEC.glow.alpha, POWER_BANK_SPEC.glow.tint),
      apply: () => {},
    },
  };
}

// ---- powerSpawn（powerSpawn.metadata.js） ----

export function joinPowerSpawn(assets: JoinAssets): PartJoin {
  return {
    disc: {
      create: () => {
        const g = new Graphics();
        const e1 = POWER_SPAWN_SPEC.ellipse1;
        gfxCircle(g, e1.radius, { fill: e1.color, stroke: e1.strokeColor, strokeWidth: e1.strokeWidth });
        const e2 = POWER_SPAWN_SPEC.ellipse2;
        gfxCircle(g, e2.radius, { fill: e2.color, stroke: e2.strokeColor, strokeWidth: e2.strokeWidth });
        gfxCircle(g, POWER_SPAWN_SPEC.ellipse3.radius, { fill: POWER_SPAWN_SPEC.ellipse3.color });
        return g;
      },
      apply: () => {},
    },
    badge: {
      create: () => gfxCircle(new Graphics(), POWER_SPAWN_SPEC.badge.radius, { fill: POWER_SPAWN_SPEC.badge.color }),
      apply: () => {},
    },
    arc: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxArcStroke(g, POWER_SPAWN_SPEC.arc.radius, POWER_SPAWN_SPEC.arc.startAngle, vNum(v, 'angle'), POWER_SPAWN_SPEC.arc.color, POWER_SPAWN_SPEC.arc.strokeWidth);
      },
    },
    resourceCircle: {
      create: () => gfxCircle(new Graphics(), POWER_SPAWN_SPEC.resourceCircle.radius, { fill: POWER_SPAWN_SPEC.resourceCircle.color }),
      apply: (v, node) => {
        node.scale.set(vNum(v, 'scale'));
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, POWER_SPAWN_SPEC.glowBase.width, POWER_SPAWN_SPEC.glowBase.height, POWER_SPAWN_SPEC.glowBase.alpha),
      apply: () => {},
    },
    glowPulse: {
      create: () => makeGlow(assets.glow, assets.glow?.width ?? 1, undefined, POWER_SPAWN_SPEC.glowPulse.alpha),
      apply: () => {},
    },
  };
}

// ---- tower（tower.metadata.js） ----

export function joinTower(assets: JoinAssets): PartJoin {
  return {
    base: {
      create: () => sprite(assets, TOWER_SPEC.base.texture, TOWER_SPEC.base.width, TOWER_SPEC.base.height),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    rotatable: {
      // 炮塔：wrap 容器承载官方 pivot 净效果（32 world px）+ 能量条随炮转
      // （tower.metadata.js:63-98,142-161）——C-S6：v8 Sprite
      // allowChildren=false（deprecation），枪体+条包进同一 Container，
      // R5 自转时旋转该容器。
      create: () => {
        const root = new Container({ label: 'tower-rotatable-root' });
        const gunWrap = new Container({ label: 'gun' });
        const gun = dynamicSprite(assets, TOWER_SPEC.rotatable.texture, TOWER_SPEC.rotatable.width, TOWER_SPEC.rotatable.height);
        gun.label = 'gun-sprite';
        const bar = new Graphics();
        bar.label = 'bar';
        gunWrap.addChild(gun, bar);
        root.addChild(gunWrap);
        return root;
      },
      apply: (v, node) => {
        const gunWrap = node.getChildByLabel('gun')!;
        const gun = gunWrap.getChildByLabel('gun-sprite') as Sprite;
        const tex = assets.frames.get(vBool(v, 'npc') ? TOWER_SPEC.rotatable.npcTexture : TOWER_SPEC.rotatable.texture);
        if (tex) gun.texture = tex;
        const scaleY = TOWER_SPEC.rotatable.height / gun.texture.height;
        gun.anchor.set(0.5, 0);
        // 官方 pivot 净效果=32 world px（object.js:88-90 已把 payload pivot 除以
        // scale 换算进纹理系再乘回）——wrap scale=1，直接用 world 值（清偿复审 R1）
        gunWrap.pivot.set(0, TOWER_SPEC.rotatable.pivotY);
        gunWrap.rotation = 0; // R3 静态冻结（官方 RotateTo 随机/自转 R5）
        const bar = gunWrap.getChildByLabel('bar') as Graphics;
        redraw(bar);
        // 官方条画在 rotatable 精灵局部空间（×scaleY 才是世界尺寸）——
        // wrap 未缩放，坐标预先乘 scaleY 等价复刻（清偿复审 R1）
        const h = (vBool(v, 'npc') ? 0 : vNum(v, 'height')) * scaleY; // NPC 塔无能量条（A-B6）
        gfxRoundRect(
          bar,
          TOWER_SPEC.barX * scaleY,
          0,
          TOWER_SPEC.barWidth * scaleY,
          h,
          vNum(v, 'radius') * scaleY,
          TOWER_SPEC.barColor,
        );
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, TOWER_SPEC.glowBase.width, TOWER_SPEC.glowBase.height, TOWER_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, TOWER_SPEC.glowBig.width, TOWER_SPEC.glowBig.height, TOWER_SPEC.glowBig.alpha),
      apply: () => {},
    },
  };
}

// ---- lab（lab.metadata.js） ----

export function joinLab(assets: JoinAssets): PartJoin {
  return {
    base: {
      create: () => sprite(assets, LAB_SPEC.base.texture, LAB_SPEC.base.width, undefined),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    highlight: {
      create: () => sprite(assets, LAB_SPEC.highlight.texture, LAB_SPEC.highlight.width, undefined, (s) => {
        s.alpha = LAB_SPEC.highlight.restAlpha;
      }),
      apply: (v, node) => {
        (node as Sprite).alpha = vNum(v, 'alpha');
      },
    },
    mineralRest: {
      create: () => sprite(assets, LAB_SPEC.mineral.texture, 1, 1, (s) => {
        s.anchor.set(0.5, LAB_SPEC.mineral.anchorY);
        s.scale.set(LAB_SPEC.mineral.baseScale);
        s.y = LAB_SPEC.mineral.y;
        s.tint = LAB_SPEC.mineral.restTint;
      }),
      apply: () => {},
    },
    mineralActive: {
      create: () => {
        const s = dynamicSprite(assets, LAB_SPEC.mineral.texture, 1, 1);
        s.anchor.set(0.5, LAB_SPEC.mineral.anchorY);
        s.y = LAB_SPEC.mineral.y;
        return s;
      },
      apply: (v, node) => {
        const s = node as Sprite;
        s.tint = LAB_SPEC.mineral.activeTint;
        const scale = vNum(v, 'scale');
        s.scale.set(scale, scale);
        s.visible = scale > 0;
      },
    },
    energyRect: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const w = vNum(v, 'width');
        gfxRect(g, -w / 2, LAB_SPEC.energyRect.y, w, LAB_SPEC.energyRect.height, LAB_SPEC.energyRect.color);
      },
    },
    glow: {
      create: () => {
        const root = new Container();
        root.addChild(
          makeGlow(assets.glow, LAB_SPEC.glowBase.width, LAB_SPEC.glowBase.height, LAB_SPEC.glowBase.alpha),
          makeGlow(assets.glow, LAB_SPEC.glowBig.width, LAB_SPEC.glowBig.height, LAB_SPEC.glowBig.alpha),
        );
        return root;
      },
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
  };
}

// ---- terminal（terminal.metadata.js） ----

export function joinTerminal(assets: JoinAssets): PartJoin {
  const size = TERMINAL_SPEC.size;
  return {
    border: {
      create: () => sprite(assets, 'terminal-border', size, size),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    body: {
      create: () => sprite(assets, 'terminal', size, size),
      apply: () => {},
    },
    arrows: {
      create: () => sprite(assets, 'terminal-arrows', size, size),
      apply: (v, node) => {
        (node as Sprite).alpha = vNum(v, 'alpha');
      },
    },
    highlight: {
      create: () => sprite(assets, 'terminal-highlight', size, size, (s) => {
        s.blendMode = ADD_BLEND;
        s.alpha = 0;
      }),
      apply: (v, node) => {
        (node as Sprite).alpha = vNum(v, 'alpha');
      },
    },
    rects: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        const draw = (sizeKey: string, color: number): void => {
          const s = vNum(v, sizeKey);
          if (s > 0) g.rect(-s / 2, -s / 2, s, s).fill({ color });
        };
        draw('restSize', TERMINAL_SPEC.colors.rest);
        draw('powerSize', TERMINAL_SPEC.colors.power);
        draw('energySize', TERMINAL_SPEC.colors.energy);
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, TERMINAL_SPEC.glowBase.width, TERMINAL_SPEC.glowBase.height, TERMINAL_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, TERMINAL_SPEC.glowBig.width, TERMINAL_SPEC.glowBig.height, TERMINAL_SPEC.glowBig.alpha),
      apply: () => {},
    },
  };
}

// ---- extractor（extractor.metadata.js；cooldown 自转 R5） ----

export function joinExtractor(assets: JoinAssets): PartJoin {
  return {
    main: {
      create: () => sprite(assets, EXTRACTOR_SPEC.texture, EXTRACTOR_SPEC.width, undefined),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
  };
}

// ---- controller（controller.metadata.js） ----

export function joinController(assets: JoinAssets): PartJoin {
  return {
    status: {
      // 三态圈 r110 α 冻结 0.4（blink 0.05↔0.4；R5 动态）。enemy 官方声明
      // 两份同参 processor（controller.metadata.js:85-116）→ 双圈叠加
      // ≈0.64 视觉不透明度（评审 A-S2），照抄。
      create: () => {
        const root = new Container();
        // enemy 双圈（官方两份同参 processor 叠加）；色键映射避开 'enemy-2'
        const circles: Array<{ label: string; colorKey: 'own' | 'enemy' | 'safe' }> = [
          { label: 'status-own', colorKey: 'own' },
          { label: 'status-enemy', colorKey: 'enemy' },
          { label: 'status-enemy-2', colorKey: 'enemy' },
          { label: 'status-safe', colorKey: 'safe' },
        ];
        for (const { label, colorKey } of circles) {
          const g = gfxCircle(new Graphics(), CONTROLLER_SPEC.status.radius, {
            fill: CONTROLLER_SPEC.status.colors[colorKey],
          });
          g.label = label;
          g.alpha = CONTROLLER_SPEC.status.blinkAlpha;
          g.visible = false;
          root.addChild(g);
        }
        return root;
      },
      apply: (v, node) => {
        const flagFor = (label: string): string =>
          label === 'status-own' ? 'own' : label === 'status-safe' ? 'safe' : 'enemy';
        for (const child of node.children) {
          child.visible = vBool(v, flagFor(child.label));
        }
      },
    },
    white: {
      create: () => gfxCircle(new Graphics(), CONTROLLER_SPEC.whiteCircle.radius, {
        fill: CONTROLLER_SPEC.whiteCircle.color,
        fillAlpha: CONTROLLER_SPEC.whiteCircle.alpha,
      }),
      apply: () => {},
    },
    base: {
      create: () => sprite(assets, CONTROLLER_SPEC.base.texture, CONTROLLER_SPEC.base.width, CONTROLLER_SPEC.base.height, (s) => {
        s.tint = CONTROLLER_SPEC.base.tint;
      }),
      apply: () => {},
    },
    levels: {
      // 8 扇区等级贴片：第 k 级旋转 k×(2π/8)，level≥k+1 可见
      create: () => {
        const root = new Container();
        for (let k = 0; k < CONTROLLER_SPEC.sectorCount; k++) {
          const s = sprite(assets, CONTROLLER_SPEC.levelSprite.texture, CONTROLLER_SPEC.levelSprite.width, CONTROLLER_SPEC.levelSprite.height, (sp) => {
            sp.anchor.set(0.5, CONTROLLER_SPEC.levelSprite.anchorY);
          });
          s.label = `level-${k}`;
          s.rotation = (k * 2 * Math.PI) / CONTROLLER_SPEC.sectorCount;
          s.visible = false;
          root.addChild(s);
        }
        return root;
      },
      apply: (v, node) => {
        const level = vNum(v, 'level');
        for (let k = 0; k < CONTROLLER_SPEC.sectorCount; k++) {
          const s = node.getChildByLabel(`level-${k}`);
          if (s) s.visible = level >= k + 1;
        }
      },
    },
    badge: {
      create: () => gfxCircle(new Graphics(), CONTROLLER_SPEC.badge.radius, { fill: CONTROLLER_SPEC.badge.color }),
      apply: () => {},
    },
    progress: {
      // 白扇形 r37 lineWidth 0（仅饼）——blinking 冻结 0.8
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxSiteProgress(g, vNum(v, 'angle'), CONTROLLER_SPEC.progress.radius, CONTROLLER_SPEC.progress.lineWidth, CONTROLLER_SPEC.progress.color);
        node.alpha = CONTROLLER_SPEC.progress.blinkAlpha;
      },
    },
    ring: {
      // 官方有效色 0x000000：circle.js:13 只解构 `stroke` 键，payload 键名
      // strokeColor（controller.metadata.js:397）不被识别→回退默认黑（A-S3 quirk）
      create: () => gfxCircle(new Graphics(), CONTROLLER_SPEC.ring.radius, {
        stroke: 0x000000,
        strokeWidth: CONTROLLER_SPEC.ring.strokeWidth,
      }),
      apply: () => {},
    },
    downgrade: {
      // effects 层红 controller 贴图闪烁（官方 AlphaTo 脉冲；R3 静态=opacity 常值）
      create: () => sprite(assets, CONTROLLER_SPEC.base.texture, CONTROLLER_SPEC.downgrade.size, CONTROLLER_SPEC.downgrade.size, (s) => {
        s.tint = CONTROLLER_SPEC.downgrade.tint;
        s.alpha = 0;
      }),
      apply: (v, node) => {
        const s = node as Sprite;
        s.alpha = vNum(v, 'alpha');
        s.visible = s.alpha > 0;
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, CONTROLLER_SPEC.glowBase.width, CONTROLLER_SPEC.glowBase.height, CONTROLLER_SPEC.glowBase.alpha),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
      },
    },
    glowBig: {
      create: () => makeGlow(assets.glow, CONTROLLER_SPEC.glowBig.width, CONTROLLER_SPEC.glowBig.height, CONTROLLER_SPEC.glowBig.alpha),
      apply: () => {},
    },
  };
}

// ---- constructedWall（constructedWall.metadata.js） ----

export function joinConstructedWall(assets: JoinAssets): PartJoin {
  return {
    body: {
      create: () => sprite(assets, 'constructedWall', CONSTRUCTED_WALL_SPEC.size, CONSTRUCTED_WALL_SPEC.size, (s) => {
        s.blendMode = ADD_BLEND;
      }),
      apply: () => {},
    },
    flash: {
      create: () => {
        const g = new Graphics();
        g.blendMode = ADD_BLEND;
        g.visible = false;
        return g;
      },
      apply: (v, node) => {
        const g = node as Graphics;
        const visible = vBool(v, 'visible');
        if (visible) {
          redraw(g).rect(
            -CONSTRUCTED_WALL_SPEC.flashSize / 2,
            -CONSTRUCTED_WALL_SPEC.flashSize / 2,
            CONSTRUCTED_WALL_SPEC.flashSize,
            CONSTRUCTED_WALL_SPEC.flashSize,
          ).fill({ color: CONSTRUCTED_WALL_SPEC.flashColor });
        }
        g.visible = visible;
        g.alpha = vNum(v, 'alpha');
      },
    },
  };
}

// ---- constructionSite（constructionSite.metadata.js；effects 层饼+环） ----

export function joinConstructionSite(): PartJoin {
  return {
    progress: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxSiteProgress(g, vNum(v, 'angle'), CONSTRUCTION_SITE_SPEC.progress.radius, CONSTRUCTION_SITE_SPEC.progress.lineWidth, vNum(v, 'color'));
        node.alpha = vNum(v, 'alpha');
      },
    },
  };
}

// ---- rampart 公有叠加（rampart.metadata.js；effects 层） ----

export function joinRampart(assets: JoinAssets): PartJoin {
  return {
    public: {
      create: () => sprite(assets, RAMPART_SPRITE_SPEC.texture, RAMPART_SPRITE_SPEC.size, RAMPART_SPRITE_SPEC.size, (s) => {
        s.alpha = RAMPART_SPRITE_SPEC.alpha;
      }),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
  };
}

// ---- invaderCore（invaderCore.metadata.js） ----

export function joinInvaderCore(assets: JoinAssets): PartJoin {
  return {
    body: {
      create: () => sprite(assets, 'invaderCore', INVADER_CORE_SPEC.spriteSize, INVADER_CORE_SPEC.spriteSize),
      apply: () => {},
    },
    progress: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxSiteProgress(g, vNum(v, 'angle'), INVADER_CORE_SPEC.progress.radius, INVADER_CORE_SPEC.progress.lineWidth, INVADER_CORE_SPEC.progress.color);
      },
    },
    arc: {
      create: () => new Graphics(),
      apply: (v, node) => {
        const g = redraw(node as Graphics);
        gfxArcStroke(g, INVADER_CORE_SPEC.arc.radius, INVADER_CORE_SPEC.arc.startAngle, vNum(v, 'angle'), INVADER_CORE_SPEC.arc.color, INVADER_CORE_SPEC.arc.strokeWidth);
      },
    },
    glowBase: {
      create: () => makeGlow(assets.glow, INVADER_CORE_SPEC.glowBase.width, INVADER_CORE_SPEC.glowBase.height, INVADER_CORE_SPEC.glowBase.alpha),
      apply: () => {},
    },
    glowBig: {
      create: () => makeGlow(assets.glow, INVADER_CORE_SPEC.glowBig.width, INVADER_CORE_SPEC.glowBig.height, INVADER_CORE_SPEC.glowBig.alpha, INVADER_CORE_SPEC.glowBig.tint),
      apply: () => {},
    },
  };
}

// ---- flag（flag.metadata.js；effects 层；名字 R6） ----

export function joinFlag(assets: JoinAssets): PartJoin {
  return {
    primary: {
      create: () => sprite(assets, 'flag', FLAG_SPEC.size, FLAG_SPEC.size),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
    secondary: {
      create: () => sprite(assets, 'flag-secondary', FLAG_SPEC.size, FLAG_SPEC.size),
      apply: (v, node) => {
        (node as Sprite).tint = vNum(v, 'tint');
      },
    },
  };
}

// ---- 公共 _isDisabled（_all.metadata.js:78-95；effects 层红方块） ----

export function joinDisabled(): PartJoin {
  return {
    disabled: {
      create: () => {
        const g = new Graphics();
        g.blendMode = ADD_BLEND;
        g.visible = false;
        return g;
      },
      apply: (v, node) => {
        const g = node as Graphics;
        const visible = vBool(v, 'visible');
        if (visible) {
          redraw(g).rect(-DISABLED_SPEC.size / 2, -DISABLED_SPEC.size / 2, DISABLED_SPEC.size, DISABLED_SPEC.size)
            .fill({ color: DISABLED_SPEC.color });
        }
        g.visible = visible;
        g.alpha = DISABLED_SPEC.alpha;
      },
    },
  };
}
