/**
 * 渲染半件 join（creep/powerCreep 家族）。规格常量直引 specs.ts；body 环
 * 算法在 body-ring.ts（纯逻辑），此处只按值重画。运动/受击罩/死亡淡出由
 * ObjectsStage 的 tween 层编排（anim/tweens.ts），join 面只管静态件。
 *
 * 节点构造纪律沿用：共享 atlas/standalone 纹理绝不带 texture 销毁；Text
 * 精确销毁由 ObjectsStage 统一处理。v8 逐段 arc→stroke 语义（GraphicsContext
 * .stroke 每次克隆活动路径开新路径）——多色弧段互不污染，已源码取证；
 * 已知良性 quirk（pixi 8.21.0 实测，评审 C）：arc 后 stroke 经
 * GraphicsPath.getLastPoint 取 data[5]/[6]（arc 指令仅 6 元）产生 NaN 前导
 * 顶点——被光栅器丢弃、弧段无弦线，仅 2 NaN 顶点/段的退化上传，勿"修复"
 * 成显式 moveTo（同样产生零长度 NaN 段，无更优写法）。
 *
 * 官方运行时 z 序振荡 quirk（评审 A-S2 登记）：官方 creepBuildBody/circle
 * processor 重跑即 destroy+尾部 addChild（object.js:39-43,112-114）——掉血
 * 后弧环+tough 跳到资源圆之上、store 变化又翻回。我方固定声明序（tough 在
 * 资源圆上）对应官方"掉血后"稳态，与官方首帧态（资源圆在 tough 上）不同。
 */
import { Container, Graphics, Sprite, Text } from 'pixi.js';
import { CREEP_SPEC, LIGHTING_MEMBER_BLEND, POWER_CREEP_SPEC } from './specs.js';
import {
  gfxCircle,
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

function frameSprite(assets: JoinAssets, name: string, width: number, height: number): Container {
  const tex = assets.frames.get(name);
  if (!tex) return new Container(); // 缺帧降级：空件
  const sp = new Sprite(tex);
  sp.anchor.set(0.5, 0.5);
  setSpriteSize(sp, width, height);
  return sp;
}

// ---- creep（creep.metadata.js:88-226 声明序） ----

export function joinCreep(assets: JoinAssets): PartJoin {
  const ring = CREEP_SPEC.ring;
  return {
    main: {
      // 黑圆 r50 → body 环 → 黑圆 r32 → 徽标 r26 → 资源圆×3 → tough → npc
      // （官方声明序；NPC 分支互斥由 apply 显隐）
      create: () => {
        const root = new Container({ label: 'creep-main' });
        const outer = gfxCircle(new Graphics(), CREEP_SPEC.outerCircle.radius, {
          fill: CREEP_SPEC.outerCircle.color,
        });
        outer.label = 'outer';
        const ringG = new Graphics();
        ringG.label = 'ring';
        const inner = gfxCircle(new Graphics(), CREEP_SPEC.innerCircle.radius, {
          fill: CREEP_SPEC.innerCircle.color,
        });
        inner.label = 'inner';
        const badge = gfxCircle(new Graphics(), CREEP_SPEC.badge.radius, {
          fill: CREEP_SPEC.badge.color,
        });
        badge.label = 'badge';
        const resourceTotal = new Graphics();
        resourceTotal.label = 'resource-total';
        const resourcePower = new Graphics();
        resourcePower.label = 'resource-power';
        const resourceEnergy = new Graphics();
        resourceEnergy.label = 'resource-energy';
        const tough = frameSprite(assets, 'tough', CREEP_SPEC.tough.size, CREEP_SPEC.tough.size);
        tough.label = 'tough';
        const npc = frameSprite(assets, 'creep-npc', CREEP_SPEC.npc.size, CREEP_SPEC.npc.size);
        npc.label = 'npc';
        root.addChild(
          outer,
          ringG,
          inner,
          badge,
          resourceTotal,
          resourcePower,
          resourceEnergy,
          tough,
          npc,
        );
        return root;
      },
      apply: (v, node) => {
        const npc = vNum(v, 'npc') === 1;
        // body 环：一对镜像弧/部件（body-ring.ts 展开序），逐段 arc→stroke
        const ringG = node.getChildByLabel('ring') as Graphics;
        redraw(ringG);
        const flat = vNums(v, 'ring');
        const drawRadius = ring.radius - ring.lineWidth / 2;
        for (let i = 0; i + 3 < flat.length; i += 4) {
          ringG
            .arc(0, 0, drawRadius, flat[i + 1]!, flat[i + 2]!, flat[i + 3] === 1)
            .stroke({ width: ring.lineWidth, color: flat[i]!, alpha: 1 });
        }
        // 资源圆三层（白 total 底 → 红 power → 黄 energy；r=0 不画）
        const drawResource = (label: string, radius: number, color: number): void => {
          const g = redraw(node.getChildByLabel(label) as Graphics);
          g.visible = radius > 0;
          if (radius > 0) g.circle(0, 0, radius).fill({ color });
        };
        drawResource('resource-total', vNum(v, 'totalR'), CREEP_SPEC.resourceColors.total);
        drawResource('resource-power', vNum(v, 'powerR'), CREEP_SPEC.resourceColors.power);
        drawResource('resource-energy', vNum(v, 'energyR'), CREEP_SPEC.resourceColors.energy);
        // NPC 互斥：本体件全隐，NPC 精灵独显（官方 when isNpc/isNotNpc）
        for (const label of [
          'outer',
          'ring',
          'inner',
          'badge',
          'resource-total',
          'resource-power',
          'resource-energy',
          'tough',
        ] as const) {
          node.getChildByLabel(label)!.visible = !npc;
        }
        node.getChildByLabel('tough')!.visible = !npc && vNum(v, 'tough') === 1;
        node.getChildByLabel('npc')!.visible = npc;
        // 敌方 safemode α（官方 AlphaTo(mainContainer, 0)）
        node.alpha = vNum(v, 'alpha');
      },
    },
    mask: {
      // creep-mask.png 100×100（lighting 层 SCREEN 成员）；holder α 传导
      // safemode 暗化（官方 mainContainer 子件语义等价，评审 A-B3）
      create: () =>
        makeGlow(
          assets.creepMask,
          CREEP_SPEC.mask.width,
          CREEP_SPEC.mask.height,
          CREEP_SPEC.mask.alpha,
        ),
      apply: (v, node) => {
        node.alpha = vNum(v, 'alpha');
      },
    },
    glow: {
      create: () =>
        makeGlow(assets.glow, CREEP_SPEC.glow.width, CREEP_SPEC.glow.height, CREEP_SPEC.glow.alpha),
      apply: (v, node) => {
        node.alpha = vNum(v, 'alpha');
      },
    },
    glowNpc: {
      create: () =>
        makeGlow(
          assets.glow,
          CREEP_SPEC.npcGlow.width,
          CREEP_SPEC.npcGlow.height,
          CREEP_SPEC.npcGlow.alpha,
        ),
      apply: (v, node) => {
        node.alpha = vNum(v, 'alpha');
      },
    },
    name: {
      // displayName（action-templates/displayName.js）：同 spawn 名款
      create: () =>
        makeText({
          text: '',
          style: {
            align: 'center',
            fill: 0xffffff,
            fontFamily: CREEP_SPEC.name.fontFamily,
            fontSize: CREEP_SPEC.name.fontSize,
            stroke: { color: CREEP_SPEC.name.strokeColor, width: CREEP_SPEC.name.strokeThickness },
          },
          anchorX: 0.5,
          anchorY: CREEP_SPEC.name.anchorY,
        }),
      apply: (v, node) => {
        const t = node as unknown as Text;
        t.text = vStr(v, 'text');
        t.style.fill = vNum(v, 'fill');
      },
    },
  };
}

// ---- powerCreep（powerCreep.metadata.js:75-162 声明序） ----

export function joinPowerCreep(assets: JoinAssets): PartJoin {
  return {
    main: {
      // 本体精灵（className-lvlN 180×180 tint 0xcc3d3e）+ 徽标圆 r26
      // （userBadge circle 退化面——G3）
      create: () => {
        const root = new Container({ label: 'pcreep-main' });
        const body = frameSprite(
          assets,
          'operator-lvl0',
          POWER_CREEP_SPEC.spriteSize,
          POWER_CREEP_SPEC.spriteSize,
        );
        body.label = 'body';
        (body as Sprite).tint = POWER_CREEP_SPEC.spriteTint;
        const badge = gfxCircle(new Graphics(), POWER_CREEP_SPEC.badge.radius, {
          fill: POWER_CREEP_SPEC.badge.color,
        });
        badge.label = 'badge';
        root.addChild(body, badge);
        return root;
      },
      apply: (v, node) => {
        const body = node.getChildByLabel('body') as Sprite;
        const texture = vStr(v, 'texture');
        const tex = texture ? assets.frames.get(texture) : undefined;
        body.texture = tex ?? body.texture;
        body.visible = tex !== undefined; // className 缺位/缺帧防御性隐藏
        node.alpha = vNum(v, 'alpha');
      },
    },
    light: {
      // lighting 层同纹理 180×180 α1（:138-149）
      create: () => new Container(),
      apply: (v, node) => {
        node.visible = vBool(v, 'visible');
        node.alpha = vNum(v, 'alpha');
        const tex = assets.frames.get(vStr(v, 'texture'));
        if (tex && node.children.length === 0) {
          const sp = new Sprite(tex);
          sp.anchor.set(0.5, 0.5);
          setSpriteSize(sp, POWER_CREEP_SPEC.spriteSize, POWER_CREEP_SPEC.spriteSize);
          sp.blendMode = LIGHTING_MEMBER_BLEND;
          node.addChild(sp);
        }
        const sp = node.children[0] as Sprite | undefined;
        if (sp && tex) sp.texture = tex;
      },
    },
    glow: {
      // glow 400×400 α1.0 tint 0xFF5555（:151-162）；α 传导 safemode
      create: () =>
        makeGlow(
          assets.glow,
          POWER_CREEP_SPEC.glow.width,
          POWER_CREEP_SPEC.glow.height,
          POWER_CREEP_SPEC.glow.alpha,
          POWER_CREEP_SPEC.glow.tint,
        ),
      apply: (v, node) => {
        node.alpha = vNum(v, 'alpha');
      },
    },
    name: {
      create: () =>
        makeText({
          text: '',
          style: {
            align: 'center',
            fill: 0xffffff,
            fontFamily: POWER_CREEP_SPEC.name.fontFamily,
            fontSize: POWER_CREEP_SPEC.name.fontSize,
            stroke: {
              color: POWER_CREEP_SPEC.name.strokeColor,
              width: POWER_CREEP_SPEC.name.strokeThickness,
            },
          },
          anchorX: 0.5,
          anchorY: POWER_CREEP_SPEC.name.anchorY,
        }),
      apply: (v, node) => {
        const t = node as unknown as Text;
        t.text = vStr(v, 'text');
        t.style.fill = vNum(v, 'fill');
      },
    },
  };
}
