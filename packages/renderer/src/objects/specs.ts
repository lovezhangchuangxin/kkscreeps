/**
 * 静态对象渲染规格常量——逐类型对官方 metadata/src/objects/*.metadata.js 取证
 * （file:line 见各段头注；〔官拆〕§2/§5 为索引）。terrain-spec.ts 同纪律：
 * 测试直接断言本表，实现只许引用本表不许内联魔法数。零 pixi 导入。
 *
 * 通用语义（engine/src/lib/processors/object.js:54-104）：
 *   - sprite width/height 应用为 scale=目标/纹理自然尺寸（只给 width 时等比）；
 *   - payload 未给 pivot/anchor 时 anchor=(0.5,0.5)；
 *   - blendMode 1 = ADD（pixi v7 BLEND_MODES.ADD）；
 *   - lighting 层成员经 display 钩子统一 SCREEN 混合（metadata/src/index.js:116-121）。
 * 常驻动画（呼吸/自转/闪烁）R3 静态冻结于 blink 模板可见相位（§12 登记），
 * R5 动效系统接管时序。
 */

// ---- 公共 ----

/** glow 精灵的默认混合：lighting 层成员=SCREEN（index.js:116-121 display 钩子）。 */
export const LIGHTING_MEMBER_BLEND = 'screen' as const;
/** effects 层 ADD（blendMode 1）。 */
export const ADD_BLEND = 'add' as const;

// ---- road（road.metadata.js + processors/road.js:8-9,77-80） ----

export const ROAD_SPEC = {
  color: 0xaaaaaa,
  /** r = 0.15×CELL。 */
  radiusFactor: 0.15,
  /** 邻接连通四向：左上对角/正上/右上对角/正左（road.js:65-68）。 */
} as const;

// ---- source（source.metadata.js:7-23） ----

export const SOURCE_SPEC = {
  rect: {
    fillColor: 0x111111,
    strokeColor: 0x595026,
    strokeBlinkingColor: 0x0e0c04,
    strokeWidth: 15,
    size: 40,
    radius: 15,
  },
  energy: {
    fillColor: 0xffe56d,
    size: 60,
    radius: 15,
    blinkingColors: [0xffffff, 0xffcd6d, 0xffde84],
  },
  /** lighting glow：800² tint 0xFFFF50 α0.5（flicker 0.6/0.4/0.8/0.4 R5）。 */
  glowBig: { width: 800, height: 800, tint: 0xffff50, alpha: 0.5 },
  /** energy>0 时第二个 glow 150² 白。 */
  glowSmall: { width: 150, height: 150, tint: 0xffffff, alpha: 1 },
} as const;

// ---- mineral（mineral.metadata.js:7-36,66-95） ----

export const MINERAL_SPEC = {
  radius: 54,
  strokeWidth: 10,
  /** 化学符号字色表（L/U/K/Z/X/O/H fg/bg）。 */
  colors: {
    L: { foreground: 0x89f4a5, background: 0x3f6147 },
    U: { foreground: 0x88d6f7, background: 0x1b617f },
    K: { foreground: 0x9370ff, background: 0x331a80 },
    Z: { foreground: 0xf2d28b, background: 0x594d33 },
    X: { foreground: 0xff7a7a, background: 0x4f2626 },
    O: { foreground: 0xcccccc, background: 0x4d4d4d },
    H: { foreground: 0xcccccc, background: 0x4d4d4d },
  } as Record<string, { foreground: number; background: number }>,
  text: { fontSize: 82, fontWeight: 'bold', fontFamily: 'Roboto, serif' },
  glowBig: { width: 700, height: 700, alpha: 0.7 },
  glowSmall: { width: 200, height: 200, alpha: 1 },
} as const;

// ---- 掉落资源（energy.metadata.js + resourceCircle.js:8-26） ----

export const RESOURCE_SPEC = {
  /** METADATA.energy：黄 r30；power r45 红（energy.metadata.js 消费面）。 */
  colors: { energy: 0xffe56d, power: 0xf41f33 },
  radii: { energy: 30, power: 45 },
  defaultRadius: 45,
  /** resourceCircle 容量缺省（resourceCircle.js:34）。 */
  defaultCapacity: 1250,
  // lighting 层圆色=资源色（METADATA.color）——官方第二 processor 的 payload
  // tint:0xFFFFFF 被 resourceCircle.js 重建 payload 时静默丢弃（:50-56 只传
  // {color,radius}），运行时实为资源色。保真运行时行为，非 payload 字面。
} as const;

// ---- deposit（deposit.metadata.js:25-33,78-98） ----

export const DEPOSIT_SPEC = {
  size: 160,
  colors: {
    biomass: 0x84b012,
    metal: 0x956f5c,
    mist: 0xda6bf5,
    silicon: 0x4ca7e5,
  } as Record<string, number>,
  /** fill alpha = max(0, 0.6 − harvested/1e5)（deposit.metadata.js:53-58）。 */
  fillBase: 0.6,
  fillDivisor: 100000,
  glow: { width: 700, height: 700, alpha: 1 },
} as const;

// ---- container（container.metadata.js:8-26,29-49,59-107） ----

export const CONTAINER_SPEC = {
  border: { width: 60, height: 70, tint: 0x181818 },
  internalBorder: { width: 40, height: 50, tint: 0x555555 },
  barWidth: 40,
  /** 高度基准：×50/(storeCapacity||resourcesTotal)（:13-26）。 */
  heightScale: 50,
  barY: 25,
  colors: { other: 0xffffff, power: 0xf41f33, energy: 0xffe56d },
  glow: { width: 100, height: 100, alpha: 1 },
} as const;

// ---- tombstone / ruin（tombstone.metadata.js / ruin.metadata.js） ----

export const DECAY_SPEC = {
  size: 100,
  resourceAlpha: 0.8,
  /** resourceColor = resourcesTotal>0 ? playerColor : 0x000000（tombstone:11-14）。 */
  emptyColor: 0x000000,
} as const;

// ---- extension（extension.metadata.js:5-9,68-90） ----

export const EXTENSION_SPEC = {
  sizes: { small: 68, medium: 80, large: 100 },
  /** size 阈值：cap≥200→large，≥100→medium，否则 small（:14-24）。
   * border 贴图判定官方为**精确相等**（:37,48,59：!cap||<100→border50、
   * ===100→border100、===200→border200，其余容量无 border 件）。 */
  tierLarge: 200,
  tierMedium: 100,
  borders: { small: 'extension-border50', medium: 'extension-border100', large: 'extension-border200' },
  base: { texture: 'extension', width: 100, height: 100 },
  /** 资源圆：r=size×0.32，scale=min(1, energy/cap)（:82-87）。 */
  circleFactor: 0.32,
  circleColor: 0xffe56d,
  glowBase: { width: 100, height: 100, alpha: 1 },
  /** 分档附加 glow（cap=50/100/200 → 200/220/250²，α0.7，energy>0 时）。 */
  glowTiers: [
    { cap: 50, width: 200, height: 200, alpha: 0.7 },
    { cap: 100, width: 220, height: 220, alpha: 0.7 },
    { cap: 200, width: 250, height: 250, alpha: 0.7 },
  ] as const,
} as const;

// ---- storage（storage.metadata.js:8-9,34-46,104-126） ----

export const STORAGE_SPEC = {
  size: 200,
  barWidth: 110,
  totalHeight: 140,
  colors: { other: 0xffffff, power: 0xf41f33, energy: 0xffe56d },
  /** 容量缺省=constants.STORAGE_CAPACITY（官方 1e6）。 */
  defaultCapacity: 1000000,
  glowBase: { width: 200, height: 200, alpha: 1 },
  glowBig: { width: 800, height: 800, alpha: 0.5 },
} as const;

// ---- factory（factory.metadata.js:10-16,54-116,161-239） ----

export const FACTORY_SPEC = {
  spriteSize: 200,
  containerSize: 50,
  barY: 25,
  colors: {
    container: 0x555555,
    energy: 0xfadf7e,
    power: 0xf41f33,
    resource: 0xffffff,
  },
  /** 常显 highlight α0.4；cooldown 闪到 1（0.2/0.8 tick，R5 动态）。 */
  highlightAlpha: 0.4,
  highlightFlashAlpha: 1,
  glowBase: { width: 200, height: 200, alpha: 1 },
  glowBig: { width: 800, height: 800, alpha: 0.5 },
  /** 官方 powerHeight 表达式优先级 bug（factory.metadata.js:46：
   * `store['power']||0 + store['energy']||0` 解析为 power || (0+energy) || 0
   * ——保真复刻，灰区 G10 盯梢）。 */
} as const;

// ---- nuker（nuker.metadata.js:5-19,46-102） ----

export const NUKER_SPEC = {
  spriteSize: 300,
  spriteY: -40,
  triangle: { color: 0xffe56d, x: 0, y: 10, width: 86, height: 118 },
  gRect: { color: 0xffffff, x: -40, y: 35, width: 80, height: 15 },
  glowSmall: { width: 100, height: 100, alpha: 1 },
  glowBig: { width: 800, height: 800, alpha: 0.5 },
} as const;

// ---- spawn（spawn.metadata.js:7-27,58-87,136-178） ----

export const SPAWN_SPEC = {
  ellipse1: { color: 0xcccccc, radius: 70 },
  ellipse2: { color: 0x181818, radius: 59 },
  badge: { radius: 38 },
  arc: { color: 0xcccccc, radius: 50, startAngle: -Math.PI / 2, strokeWidth: 10 },
  resourceCircle: { radius: 38, color: 0xffe56d },
  glowBase: { width: 100, height: 100, alpha: 1 },
  glowBig: { width: 600, height: 600, alpha: 0.5 },
  name: {
    fontSize: 55,
    fontFamily: 'Roboto, sans-serif',
    strokeColor: '#111111',
    strokeThickness: 15,
    anchorY: 1.7,
  },
} as const;

// ---- link（link.metadata.js:14-51,73-97） ----

export const LINK_SPEC = {
  size: 100,
  energySize: 50,
  glowBase: { width: 100, height: 100, alpha: 1 },
  glowBig: { width: 400, height: 400, alpha: 0.5 },
} as const;

// ---- observer（observer.metadata.js:5-17,19-70） ----

export const OBSERVER_SPEC = {
  border: { color: 0x111111, strokeWidth: 5, width: 45, height: 40 },
  eye: { strokeWidth: 0, width: 20, height: 20 },
  glow: { width: 800, height: 800, alpha: 0.5 },
} as const;

// ---- powerBank（powerBank.metadata.js:7-11,24-58） ----

export const POWER_BANK_SPEC = {
  spriteSize: 200,
  ellipse: { color: 0xf41f33, strokeColor: 0x8d000d, strokeWidth: 10 },
  /** r=sqrt((power/CAPACITY_MAX×3000)/π)（:16-22）。 */
  areaFactor: 3000,
  capacityMax: 5000,
  glow: { width: 800, height: 800, alpha: 1, tint: 0xff8080 },
} as const;

// ---- powerSpawn（powerSpawn.metadata.js:4-32,50-116,164-202） ----

export const POWER_SPAWN_SPEC = {
  ellipse1: { color: 0x222222, strokeColor: 0xcccccc, strokeWidth: 7, radius: 75 },
  ellipse2: { color: 0x222222, strokeColor: 0xf41f33, strokeWidth: 10, radius: 68 },
  ellipse3: { color: 0x181818, radius: 59 },
  badge: { color: 0x555555, radius: 38 },
  arc: { color: 0xf41f33, radius: 50, startAngle: -Math.PI / 2, strokeWidth: 10 },
  resourceCircle: { radius: 38, color: 0xffe56d },
  glowBase: { width: 150, height: 150, alpha: 1 },
  glowPulse: { alpha: 0.5 },
} as const;

// ---- tower（tower.metadata.js:7,52-73,142-161） ----

export const TOWER_SPEC = {
  base: { texture: 'tower-base', width: 200, height: 200 },
  rotatable: { texture: 'tower-rotatable', npcTexture: 'tower-rotatable-npc', width: 115, height: 115, pivotY: 32 },
  barFullHeight: 66.7,
  barWidth: 90,
  barX: -45,
  barColor: 0xffe56d,
  barRadiusCap: 12,
  glowBase: { width: 100, height: 100, alpha: 1 },
  glowBig: { width: 600, height: 600, alpha: 0.5 },
  flare: { texture: 'flare1', width: 400, height: 400 },
} as const;

// ---- lab（lab.metadata.js:6-27,59-135） ----

export const LAB_SPEC = {
  base: { texture: 'lab', width: 200 },
  highlight: { texture: 'lab-highlight', width: 200, restAlpha: 0, cooldownAlpha: 0.5 },
  mineral: {
    texture: 'lab-mineral',
    restTint: 0x777777,
    activeTint: 0xffffff,
    anchorY: 1,
    baseScale: 0.6875,
    y: 25,
  },
  energyRect: { color: 0xffe56d, x: -40, y: 32, width: 67, height: 10 },
  glowBase: { width: 150, height: 150, alpha: 1 },
  glowBig: { width: 500, height: 500, alpha: 0.3 },
} as const;

// ---- terminal（terminal.metadata.js:8,53-70,119-158） ----

export const TERMINAL_SPEC = {
  size: 200,
  rectFull: 76,
  /** 容量缺省=constants.TERMINAL_CAPACITY（@screeps/common@2.15.5 constants.js:332=300000）。 */
  defaultCapacity: 300000,
  arrowsDimAlpha: 0.1,
  highlightAlpha: 1,
  colors: { rest: 0xffffff, power: 0xf41f33, energy: 0xffe56d },
  glowBase: { width: 200, height: 200, alpha: 1 },
  glowBig: { width: 800, height: 800, alpha: 0.5 },
} as const;

// ---- invaderCore（invaderCore.metadata.js:3-8,27-72） ----

export const INVADER_CORE_SPEC = {
  spriteSize: 200,
  progress: { color: 0xff0d39, radius: 23, lineWidth: 0 },
  arc: { color: 0xcccccc, radius: 40, startAngle: -Math.PI / 2, strokeWidth: 10 },
  /** constants.STRONGHOLD_DECAY_TICKS（@screeps/common@2.15.5 constants.js:849=75000）。 */
  decayTicks: 75000,
  glowBase: { width: 100, height: 100, alpha: 1 },
  glowBig: { width: 800, height: 800, alpha: 1, tint: 0xff8080 },
} as const;

// ---- controller（controller.metadata.js:74-135,254-298,374-400,402-439,440-464） ----

export const CONTROLLER_SPEC = {
  status: { alpha: 0.05, blinkAlpha: 0.4, radius: 110, colors: { own: 0x33ff33, enemy: 0xff3333, safe: 0xffd180 } },
  whiteCircle: { alpha: 0.05, color: 0xffffff, radius: 92 },
  base: { texture: 'controller', width: 200, height: 200, tint: 0x000000 },
  levelSprite: { texture: 'controller-level', width: 100, height: 100, anchorY: 1 },
  /** 8 扇区：第 k 级旋转 k×(2π/8)。 */
  sectorCount: 8,
  badge: { radius: 37, color: 0x222222 },
  progress: { color: 0xffffff, radius: 37, lineWidth: 0, blinkAlpha: 0.8, restAlpha: 0.3 },
  ring: {
    radius: 40,
    /** payload 字面 0x080808（controller.metadata.js:397），但 circle.js 只认
     * `stroke` 键名→有效色=默认黑 0x000000（评审 A-S3 quirk，取有效值）。 */
    strokeColor: 0x000000,
    strokeWidth: 10,
  },
  downgrade: { size: 200, tint: 0xff3333 },
  glowBig: { width: 1200, height: 1200, alpha: 0.5 },
  glowBase: { width: 500, height: 500, alpha: 1 },
} as const;

/** constants.CONTROLLER_LEVELS（@screeps/common@2.15.5 constants.js:212；对象表
 * {1:200..7:10935000}——**无 level 8**：满级 progressTotal=undefined 不画扇形，
 * 我方以缺位表达。index=level；夹具 level5→progressTotal 1215000 与表值一致佐证）。 */
export const CONTROLLER_LEVELS: readonly number[] = [
  0, 200, 45000, 135000, 405000, 1215000, 3645000, 10935000,
];

/** constants.CONTROLLER_DOWNGRADE（降级剩余秒数/level）。 */
export const CONTROLLER_DOWNGRADE: readonly number[] = [
  0, 20000, 10000, 20000, 40000, 80000, 120000, 150000, 200000,
];

// ---- constructedWall（constructedWall.metadata.js:8-36） ----

export const CONSTRUCTED_WALL_SPEC = {
  size: 100,
  /** hits===1 && hitsMax>1 红闪（blink 0↔0.2；冻结可见相位 0.2）。 */
  flashAlpha: 0.2,
  flashColor: 0xff0000,
  flashSize: 100,
} as const;

// ---- extractor（extractor.metadata.js:5-31） ----

export const EXTRACTOR_SPEC = {
  texture: 'extractor',
  width: 200,
} as const;

// ---- rampart 公有精灵（rampart.metadata.js:10-23） ----

export const RAMPART_SPRITE_SPEC = {
  texture: 'rampart',
  size: 100,
  alpha: 0.5,
} as const;

// ---- rampart 私有顶罩（terrain.js:183-247） ----

export const RAMPART_COVER_SPEC = {
  alpha: 0.4,
  strokeWidth: 25,
  blend: ADD_BLEND,
} as const;

// ---- constructionSite（constructionSite.metadata.js:7-23 + siteProgress.js） ----

export const CONSTRUCTION_SITE_SPEC = {
  progress: { radius: 20, lineWidth: 10 },
  /** blinking(0.8, 0.3, 1, 1)——冻结可见相位 0.8。 */
  blinkAlpha: 0.8,
} as const;

// ---- portal（portal.metadata.js:6-16,54-59） ----

export const PORTAL_SPEC = {
  baseCircle: { radius: 45, color: 0x111133, alpha: 0.5 },
  wave: { radius: 40, color: 0x61c0ed, alpha: 0.5 },
  wave2: { radius: 40, color: 0x111133, alpha: 0.5 },
  glowBig: { width: 700, height: 700, alpha: 0.7, tint: 0x9999ff },
  glowSmall: { width: 150, height: 150, alpha: 1, tint: 0x7777ff },
} as const;

// ---- keeperLair（keeperLair.metadata.js:5-13,50-57） ----

export const KEEPER_LAIR_SPEC = {
  baseCircle: { radius: 60, color: 0x000000, alpha: 1 },
  wave: { radius: 50, color: 0x780207, alpha: 1 },
  wave2: { radius: 50, color: 0x000000, alpha: 1 },
  glowBig: { width: 800, height: 800, alpha: 0.5, tint: 0xff0000 },
  glowSmall: { width: 150, height: 150, alpha: 1 },
} as const;

// ---- 公共 _isDisabled（_all.metadata.js:78-95） ----

export const DISABLED_SPEC = {
  size: 100,
  color: 0xff0000,
  /** blinking(0, 0.5, 0.3, 1.5)——冻结可见相位 0.5。 */
  alpha: 0.5,
} as const;

// ---- flag（flag.metadata.js:3-14,34-61；z=0||0） ----

export const FLAG_SPEC = {
  size: 200,
  colors: {
    1: 0xf44336,
    2: 0x9c27b0,
    3: 0x2196f3,
    4: 0x00bcd4,
    5: 0x4caf50,
    6: 0xffeb3b,
    7: 0xff9800,
    8: 0x795548,
    9: 0x9e9e9e,
    10: 0xffffff,
  } as Record<number, number>,
} as const;

// ---- creep（creep.metadata.js + creepBuildBody.js + creepActions.js） ----

export const CREEP_SPEC = {
  /** body 环（creepBuildBody.js:7-12 逐常量）。 */
  ring: {
    maxParts: 50,
    /** ANGLE_SHIFT = −π/2（12 点钟起点）。 */
    angleShift: -Math.PI / 2,
    /** PART_ANGLE = 2π/50/2——每 100 hits 的弧长。 */
    partAngle: (2 * Math.PI) / 50 / 2,
    maxPartHits: 100,
    radius: 50,
    lineWidth: 18,
    /** 色表（creepBuildBody.js:13-43）。carry 色值存在但被过滤不画
     * （:108-109）；order 字段是死代码——排序按合并 hits 升序（:121-129）。 */
    colors: {
      move: 0xaab7c5,
      work: 0xfde574,
      carry: 0x666666,
      attack: 0xf72e41,
      ranged_attack: 0x7fa7e5,
      heal: 0x56cf5e,
      claim: 0xb99cfb,
    } as Record<string, number>,
  },
  /** 层叠（creep.metadata.js:88-176）：黑圆 r50(0x202020)→body 环→黑圆
   * r32(0x000000)→徽标 r26→资源圆。 */
  outerCircle: { color: 0x202020, radius: 50 },
  innerCircle: { color: 0x000000, radius: 32 },
  /** 徽标（userBadge :117-125 userOwnerColor 分支 payload 色 0x222222；
   * G3 无 badgeUrl——官方失败退化路径=纯色圆）。 */
  badge: { radius: 26, color: 0x222222 },
  /** 中心资源圆（:137-166）：r=min(1,total|energy|power/storeCapacity)×20。
   * 白圆 when 表达式 `store['energy']||0 + store['power']||0 < resourcesTotal`
   * 系官方 || 优先级 bug（:140）——解析为 `store.energy||store.power||(0<total)`
   * ⇒ 资源>0 恒真（灰区 G12）；保真运行时行为=resourcesTotal>0 即显示。 */
  resourceRadius: 20,
  resourceColors: { total: 0xffffff, power: 0xf41f33, energy: 0xffe56d },
  /** 任一 tough 存活→中心 tough.svg 120×120（creepBuildBody.js:62-71,154-156）。 */
  tough: { size: 120 },
  /** NPC（user '2'/'3'）整只 creep-npc.svg 100×100（:168-177）。 */
  npc: { size: 100 },
  mask: { width: 100, height: 100, alpha: 1 },
  glow: { width: 400, height: 400, alpha: 0.2 },
  /** NPC glow（:201-213）——注意 400 大 glow 无 when 门（shouldRun 仅
   * !spawning），NPC 两只 glow 并存（官方 once 语义下的实际运行时行为）。 */
  npcGlow: { width: 100, height: 100, alpha: 0.5 },
  /** 敌方 safemode 期间整只 α0.5（calculations.safeMode :49-62；AlphaTo
   * 时长 0=瞬时）。 */
  safeModeAlpha: 0.5,
  name: {
    fontSize: 55,
    fontFamily: 'Roboto, sans-serif',
    strokeColor: '#111111',
    strokeThickness: 15,
    anchorY: 1.7,
  },
  /** 受击罩（creepActions.js:26-78,149-171,299-306）：cover.svg ADD、
   * α0→0.5→0 关键帧（createChildCoverSpriteAction :163-171 的 /4、3/4 分数）；
   * 色表 :13-24。**时长系数 0.9**：三处 call-site（:300/:303/:305）传
   * tickDuration*0.9——峰值在 0.225td、总长 0.9td（R4 评审 A-B2 勘误，
   * 〔官拆〕§3 的 "1/4 tick" 系漏看 call-site 系数）。 */
  cover: {
    alpha: 0.5,
    fadeIn: 0.25,
    fadeOut: 0.75,
    durationScale: 0.9,
    colors: { attack: 0xff3333, heal: 0x2ce328, both: 0xffff33 },
  },
} as const;

// ---- powerCreep（powerCreep.metadata.js；引擎 G5 家族——合成夹具取材） ----

export const POWER_CREEP_SPEC = {
  spriteSize: 180,
  /** tint 0xcc3d3e（:93）；纹理=className-lvl{min(4,ceil(level/6))}（:41-44）。
   * :46-48 的 texture-light 计算（className-lvl4）是死代码——lighting 精灵
   * 实际用 texture 计算（:143），保真运行时行为。 */
  spriteTint: 0xcc3d3e,
  badge: { radius: 26, color: 0x222222 },
  glow: { width: 400, height: 400, alpha: 1, tint: 0xff5555 },
  name: {
    fontSize: 55,
    fontFamily: 'Roboto, sans-serif',
    strokeColor: '#111111',
    strokeThickness: 15,
    anchorY: 1.7,
  },
  safeModeAlpha: 0.5,
} as const;

// ---- 房间事件码（kk-core/constants.rs:502-514，与官方 constants.js 实测逐项
// 相等——R1 §12；R4 只消费攻击/治疗，R5 动效系统扩全表）。
// 落点说明：shared 常量生成管线（feed.ts:57 申明）落地前的渲染器本地替身，
// R5 扩全表时随管线统一处置（R4 评审 B建议1）。 ----

export { EVENT_ATTACK, EVENT_HEAL } from '@kkscreeps/shared';

// ---- tick 时长缺省（G6：官方闭源壳传入值不可知——契约参数化；index.ts 与
// ObjectsStageOptions 注释单源引用，R4 评审 B建议5） ----

export const DEFAULT_TICK_DURATION_MS = 1000;
