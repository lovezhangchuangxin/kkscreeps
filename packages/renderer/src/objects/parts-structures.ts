/**
 * 部件声明表（结构家族）——纯逻辑半件（values），规格常量引 specs.ts。
 * 语义注释附官方 metadata file:line。零 pixi 导入。
 *
 * cooldown 判定沿官方：isCooldown = cooldownTime && cooldownTime >= gameTime
 * （lab 用 >，其余 >=——保真逐项保留，见各件注释）。
 */
import type { RoomObjectState } from '@kkscreeps/shared';
import {
  energyCapacityOf,
  num,
  optNum,
  ownerOf,
  storeCapacityOf,
  storeOf,
  type PartDecl,
  type PartEnv,
  type PartValues,
} from './part-decl.js';
import { playerColor, SPAWN_BADGE_COLOR } from './player-color.js';
import { resourcesTotal } from './parts-naturals.js';
import {
  CONSTRUCTED_WALL_SPEC,
  CONSTRUCTION_SITE_SPEC,
  CONTROLLER_DOWNGRADE,
  CONTROLLER_LEVELS,
  EXTENSION_SPEC,
  FACTORY_SPEC,
  FLAG_SPEC,
  INVADER_CORE_SPEC,
  LAB_SPEC,
  NUKER_SPEC,
  POWER_BANK_SPEC,
  STORAGE_SPEC,
  TERMINAL_SPEC,
  TOWER_SPEC,
} from './specs.js';

function str(v: unknown): string {
  return typeof v === 'string' ? v : '';
}
function bool(v: unknown): boolean {
  return v === true;
}

/** 官方 spawningAngle（spawn.metadata.js:31-39 / invaderCore.metadata.js:11-20）：
 * −π/2 + 2π×(needTime−remaining)/needTime，needTime=0 → 0。 */
export function spawningAngle(
  spawning: unknown,
  time: number,
): number {
  if (typeof spawning !== 'object' || spawning === null) return SPAWN_START_ANGLE;
  const s = spawning as Record<string, unknown>;
  const needTime = optNum(s.needTime) ?? 0;
  const spawnTime = optNum(s.spawnTime) ?? time + 0.01;
  const remaining = spawnTime - time;
  if (!needTime) return SPAWN_START_ANGLE;
  return SPAWN_START_ANGLE + ((2 * Math.PI) * (needTime - remaining)) / needTime;
}

const SPAWN_START_ANGLE = -Math.PI / 2;

/** cooldown 判定（官方 isCooldown 族）。lab 用严格 >（lab.metadata.js:29-30），
 * 其余 >=（factory/terminal/factory highlight）。 */
export function isCooldown(s: RoomObjectState, time: number, strict = false): boolean {
  const t = optNum(s.cooldownTime);
  if (t === undefined || t === 0) return false;
  return strict ? t > time : t >= time;
}

// ---- extension（extension.metadata.js） ----

export function extensionTier(capacityEnergy: number): 'small' | 'medium' | 'large' {
  if (capacityEnergy >= EXTENSION_SPEC.tierLarge) return 'large';
  if (capacityEnergy >= EXTENSION_SPEC.tierMedium) return 'medium';
  return 'small';
}

/** border 贴图档（精确相等判定，extension.metadata.js:37,48,59）；无匹配档
 * （如 cap=150/300）返回 null——官方三个 when 全不命中=无边框件。 */
export function extensionBorderTier(capacityEnergy: number): 'small' | 'medium' | 'large' | null {
  if (!capacityEnergy || capacityEnergy < EXTENSION_SPEC.tierMedium) return 'small';
  if (capacityEnergy === EXTENSION_SPEC.tierMedium) return 'medium';
  if (capacityEnergy === EXTENSION_SPEC.tierLarge) return 'large';
  return null;
}

export const PARTS_EXTENSION: PartDecl[] = [
  {
    id: 'border',
    keys: ['storeCapacityResource', 'user'],
    values: (s, env) => {
      const capacity = energyCapacityOf(s);
      return {
        tier: extensionTier(capacity),
        border: extensionBorderTier(capacity) ?? '',
        tint: playerColor(ownerOf(s), env.isOwn),
      };
    },
  },
  {
    id: 'circle',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => {
      const capacity = energyCapacityOf(s);
      const scale = capacity > 0
        ? Math.min(1, (storeOf(s).energy ?? 0) / capacity)
        : 0;
      // 圆半径随容量档位（size×0.32，extension.metadata.js:82）
      const tier = extensionTier(capacity);
      return { scale, radius: EXTENSION_SPEC.sizes[tier] * EXTENSION_SPEC.circleFactor };
    },
  },
  {
    id: 'glowBase',
    keys: ['store'],
    values: (s) => ({ visible: (storeOf(s).energy ?? 0) > 0 }),
  },
  {
    id: 'glowTier',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => {
      const capacity = energyCapacityOf(s);
      const tier = EXTENSION_SPEC.glowTiers.find((t) => t.cap === capacity);
      if (!tier || (storeOf(s).energy ?? 0) <= 0) return null;
      return { width: tier.width, height: tier.height, alpha: tier.alpha };
    },
  },
];

// ---- storage（storage.metadata.js） ----

export function storageBarHeights(s: RoomObjectState): {
  hasOther: boolean;
  hasPower: boolean;
  hasEnergy: boolean;
  otherH: number;
  powerH: number;
  energyH: number;
} {
  // 分母 = max(storeCapacity||STORAGE_CAPACITY, resourcesTotal)（storage.metadata.js:17-31）
  const store = storeOf(s);
  const capacity = storeCapacityOf(s) || STORAGE_SPEC.defaultCapacity;
  const total = resourcesTotal(store);
  const denom = Math.max(capacity, total) || 1;
  const energy = store.energy ?? 0;
  const power = store.power ?? 0;
  const scale = STORAGE_SPEC.totalHeight / denom;
  return {
    hasOther: energy + power < total,
    hasPower: power > 0,
    hasEnergy: energy > 0,
    otherH: total * scale,
    powerH: (power + energy) * scale,
    energyH: energy * scale,
  };
}

export const PARTS_STORAGE: PartDecl[] = [
  {
    id: 'border',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  { id: 'body', values: () => ({}) },
  {
    id: 'bars',
    keys: ['store', 'storeCapacity'],
    values: (s) => storageBarHeights(s),
  },
  {
    id: 'glowBase',
    keys: ['store'],
    values: (s) => ({ visible: resourcesTotal(storeOf(s)) > 0 }),
  },
  { id: 'glowBig', values: () => ({}) },
];

// ---- factory（factory.metadata.js） ----

export const PARTS_FACTORY: PartDecl[] = [
  {
    id: 'border',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  { id: 'body', values: () => ({}) },
  {
    id: 'highlight',
    keys: ['cooldownTime'],
    values: (s, env) => ({
      // 常显 0.4，cooldown 闪到 1（factory.metadata.js:75-116；R5 动态时序）
      alpha: isCooldown(s, env.time) ? FACTORY_SPEC.highlightFlashAlpha : FACTORY_SPEC.highlightAlpha,
    }),
  },
  {
    id: 'level0',
    values: () => ({}),
  },
  {
    id: 'level',
    keys: ['level'],
    values: (s): PartValues | null => {
      const level = num(s.level);
      if (level <= 0) return null;
      return { level, tint: 0xffffff };
    },
  },
  {
    id: 'internalBorder',
    // 条形区深灰底（factory.metadata.js:161-170：rectangle 50² tint 0x555555）
    values: () => ({}),
  },
  {
    id: 'bars',
    keys: ['store', 'storeCapacity'],
    values: (s) => {
      // 保真复刻官方 powerHeight 优先级 bug（factory.metadata.js:44-47：
      // store['power']||0 + store['energy']||0 ⇒ power || (0+energy) || 0）——G10
      const store = storeOf(s);
      const capacity = storeCapacityOf(s);
      const total = resourcesTotal(store);
      const denom = capacity || total || 1;
      const scale = FACTORY_SPEC.containerSize / denom;
      const power = store.power ?? 0;
      const energy = store.energy ?? 0;
      return {
        // hasOther 同族优先级 quirk（factory.metadata.js:175-176：
        // energy||0 + power||0 < total ⇒ energy || power || (0<total)，
        // total>0 恒 true=白条恒满高）——G10 并表保真
        hasOther: energy > 0 || power > 0 || 0 < total,
        hasPower: power > 0,
        hasEnergy: energy > 0,
        otherH: total * scale,
        // powerHeight（bug 保真）：power || (0 + energy) || 0
        powerH: (power || 0 + energy || 0) * scale,
        energyH: energy * scale,
      };
    },
  },
  {
    id: 'glowBase',
    keys: ['store'],
    values: (s) => ({ visible: resourcesTotal(storeOf(s)) > 0 }),
  },
  { id: 'glowBig', values: () => ({}) },
];

// ---- nuker（nuker.metadata.js） ----

export function nukerTrianglePoints(store: Record<string, number>, capacity: number): number[] {
  const t = NUKER_SPEC.triangle;
  const f = capacity > 0 ? (store.energy ?? 0) / capacity : 0;
  const w = t.width * f;
  const h = t.height * f;
  return [t.x - w / 2, t.y, t.x, t.y - h, t.x + w / 2, t.y];
}

export const PARTS_NUKER: PartDecl[] = [
  {
    id: 'border',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  { id: 'body', values: () => ({}) },
  {
    id: 'triangle',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => ({
      points: nukerTrianglePoints(storeOf(s), energyCapacityOf(s)),
    }),
  },
  {
    id: 'gRect',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => {
      // 分母=storeCapacityResource['G']（nuker.metadata.js:40-44）
      const capRes = s.storeCapacityResource as Record<string, unknown> | undefined;
      const capacityG = capRes ? num(capRes.G) : 0;
      const g = capacityG > 0 ? (NUKER_SPEC.gRect.width * (storeOf(s).G ?? 0)) / capacityG : 0;
      return { width: g };
    },
  },
  { id: 'glowSmall', values: () => ({}) },
  { id: 'glowBig', values: () => ({}) },
];

// ---- spawn（spawn.metadata.js） ----

export const PARTS_SPAWN: PartDecl[] = [
  {
    id: 'core',
    keys: ['store', 'storeCapacityResource', 'spawning', 'user'],
    values: (s, env) => {
      const capacity = energyCapacityOf(s);
      // 官方 spawn.resourceScale 无 min(1,·) 钳制（spawn.metadata.js:41-46；
      // link/extension 有——逐件保真）
      const resourceScale = capacity > 0 ? (storeOf(s).energy ?? 0) / capacity : 0;
      return {
        // badgeColor = userOwnerColor ? 0x555555 : playerColor（spawn.metadata.js:52-57）
        // ——G9 下恒 SPAWN_BADGE_COLOR；M3 切 users 色时此处改活值（随动点）
        badgeColor: SPAWN_BADGE_COLOR,
        resourceScale,
        arcAngle: spawningAngle(s.spawning, env.time),
      };
    },
  },
  {
    id: 'name',
    keys: ['user', 'name'],
    values: (s, env): PartValues | null => {
      // displayName：own 显 name（官方 showMyNames/showEnemyNames 门控——v0 只
      // 显示己方（demo 配置同款），开关面 R6）。字色=playerColor(user) 数值面。
      if (!ownerOf(s) || !env.isOwn(ownerOf(s))) return null;
      return { text: str(s.name), fill: playerColor(ownerOf(s), env.isOwn) };
    },
  },
  {
    id: 'glowBase',
    keys: ['store'],
    values: (s) => ({ visible: (storeOf(s).energy ?? 0) > 0 }),
  },
  { id: 'glowBig', values: () => ({}) },
];

// ---- link（link.metadata.js） ----

export const PARTS_LINK: PartDecl[] = [
  {
    id: 'border',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  { id: 'body', values: () => ({}) },
  {
    id: 'energy',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => {
      const capacity = energyCapacityOf(s);
      return { scale: capacity > 0 ? Math.min(1, (storeOf(s).energy ?? 0) / capacity) : 0 };
    },
  },
  {
    id: 'glowBase',
    keys: ['store'],
    values: (s) => ({ visible: (storeOf(s).energy ?? 0) > 0 }),
  },
  {
    id: 'glowBig',
    keys: ['store'],
    values: (s) => ({ visible: (storeOf(s).energy ?? 0) > 0 }),
  },
];

// ---- observer（observer.metadata.js） ----

export const PARTS_OBSERVER: PartDecl[] = [
  {
    id: 'disc',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  {
    id: 'eye',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  { id: 'glow', values: () => ({}) },
];

// ---- powerBank（powerBank.metadata.js） ----

export function powerBankRadius(power: number): number {
  // r = sqrt((power/CAPACITY_MAX × 3000)/π)（powerBank.metadata.js:16-22）
  return Math.sqrt(
    ((power / POWER_BANK_SPEC.capacityMax) * POWER_BANK_SPEC.areaFactor) / Math.PI,
  );
}

export const PARTS_POWER_BANK: PartDecl[] = [
  { id: 'body', values: () => ({}) },
  {
    id: 'pool',
    keys: ['store'],
    values: (s) => ({ radius: powerBankRadius(storeOf(s).power ?? 0) }),
  },
  { id: 'glow', values: () => ({}) },
];

// ---- powerSpawn（powerSpawn.metadata.js） ----

export const PARTS_POWER_SPAWN: PartDecl[] = [
  { id: 'disc', values: () => ({}) },
  { id: 'badge', values: () => ({}) },
  {
    id: 'arc',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => {
      // 分母=storeCapacityResource.power（powerSpawn.metadata.js:36-42）
      const capRes = s.storeCapacityResource as Record<string, unknown> | undefined;
      const capacityPower = capRes ? num(capRes.power) : 0;
      const power = num((storeOf(s) as Record<string, unknown>).power);
      const angle = SPAWN_START_ANGLE +
        (capacityPower > 0 ? ((2 * Math.PI) * power) / capacityPower : 0);
      return { angle };
    },
  },
  {
    id: 'resourceCircle',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => {
      const cap = s.storeCapacityResource as Record<string, unknown> | undefined;
      const capacityEnergy = cap ? num(cap.energy) : 0;
      const scale = capacityEnergy > 0
        ? Math.min(1, (storeOf(s).energy ?? 0) / capacityEnergy)
        : 0;
      return { scale };
    },
  },
  { id: 'glowBase', values: () => ({}) },
  { id: 'glowPulse', values: () => ({}) },
];

// ---- tower（tower.metadata.js） ----

export const PARTS_TOWER: PartDecl[] = [
  {
    id: 'base',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  {
    id: 'rotatable',
    // 炮塔贴图（NPC 分支）与能量条同件——官方条为炮塔子件（随炮转），
    // 跨件挂载并入一件（tower.metadata.js:63-98,142-161）；NPC 塔无能量条
    // （官方 when: isNotNpc，:8,144）
    keys: ['user', 'store', 'storeCapacityResource'],
    values: (s) => {
      const capacity = energyCapacityOf(s);
      const npc = ownerOf(s) === '2' || ownerOf(s) === '3';
      const height = npc
        ? 0
        : capacity > 0
          ? Math.min(TOWER_SPEC.barFullHeight, (TOWER_SPEC.barFullHeight * (storeOf(s).energy ?? 0)) / capacity)
          : 0;
      return {
        npc,
        height,
        radius: Math.min(TOWER_SPEC.barRadiusCap, height / 2),
      };
    },
  },
  {
    id: 'glowBase',
    keys: ['store'],
    values: (s) => ({ visible: (storeOf(s).energy ?? 0) > 0 }),
  },
  { id: 'glowBig', values: () => ({}) },
];

// ---- lab（lab.metadata.js） ----

/** lab 矿物存量：首个非 energy 非零键与其容量（lab.metadata.js:36-50）。 */
export function labMineralLevel(s: RoomObjectState): { type: string; scale: number } {
  const store = storeOf(s);
  const type = Object.keys(store).find((k) => k !== 'energy' && (store[k] ?? 0) > 0);
  if (!type) return { type: '', scale: 0 };
  const capRes = s.storeCapacityResource as Record<string, unknown> | undefined;
  const sumCapRes = capRes
    ? Object.values(capRes).reduce((a: number, v) => a + (typeof v === 'number' ? v : 0), 0)
    : 0;
  const capacity = (capRes ? num(capRes[type]) : 0) ||
    (storeCapacityOf(s) ? storeCapacityOf(s) - sumCapRes : 0);
  if (!capacity) return { type, scale: 0 };
  return { type, scale: (LAB_SPEC.mineral.baseScale * (store[type] ?? 0)) / capacity };
}

export const PARTS_LAB: PartDecl[] = [
  {
    id: 'base',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  {
    id: 'highlight',
    keys: ['cooldownTime'],
    values: (s, env) => ({
      // lab isCooldown 用严格 >（lab.metadata.js:29-30）
      alpha: isCooldown(s, env.time, true) ? LAB_SPEC.highlight.cooldownAlpha : LAB_SPEC.highlight.restAlpha,
    }),
  },
  { id: 'mineralRest', values: () => ({}) },
  {
    id: 'mineralActive',
    keys: ['store', 'storeCapacity', 'storeCapacityResource'],
    values: (s) => {
      const level = labMineralLevel(s);
      return { scale: level.scale };
    },
  },
  {
    id: 'energyRect',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => {
      const capacity = energyCapacityOf(s);
      const width = capacity > 0
        ? (LAB_SPEC.energyRect.width * (storeOf(s).energy ?? 0)) / capacity
        : 0;
      return { width };
    },
  },
  {
    id: 'glow',
    keys: ['store', 'storeCapacityResource'],
    values: (s) => ({
      // resourcesTotal − energy > 0（非 energy 资源在场）触发双 glow
      // （lab.metadata.js:141-165）
      visible: resourcesTotal(storeOf(s)) - (storeOf(s).energy ?? 0) > 0,
    }),
  },
];

// ---- terminal（terminal.metadata.js） ----

export function terminalRectSizes(s: RoomObjectState): {
  restSize: number;
  powerSize: number;
  energySize: number;
} {
  // ×76/(storeCapacity||TERMINAL_CAPACITY)，cap 76（terminal.metadata.js:15-46）
  const store = storeOf(s);
  const capacity = storeCapacityOf(s) || TERMINAL_SPEC.defaultCapacity;
  const total = resourcesTotal(store);
  const energy = store.energy ?? 0;
  const power = store.power ?? 0;
  const full = TERMINAL_SPEC.rectFull;
  const clamped = (v: number) => Math.min(full, v);
  const restSize = total <= energy + power ? 0 : clamped((full * total) / capacity);
  const powerSize = clamped((full * (energy + power)) / capacity);
  const energySize = clamped((full * energy) / capacity);
  return { restSize, powerSize, energySize };
}

export const PARTS_TERMINAL: PartDecl[] = [
  {
    id: 'border',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
  { id: 'body', values: () => ({}) },
  {
    id: 'arrows',
    keys: ['cooldownTime'],
    values: (s, env) => ({
      alpha: isCooldown(s, env.time) ? TERMINAL_SPEC.arrowsDimAlpha : 1,
    }),
  },
  {
    id: 'highlight',
    keys: ['cooldownTime'],
    values: (s, env) => ({
      alpha: isCooldown(s, env.time) ? TERMINAL_SPEC.highlightAlpha : 0,
    }),
  },
  {
    id: 'rects',
    keys: ['store', 'storeCapacity'],
    values: (s) => terminalRectSizes(s),
  },
  {
    id: 'glowBase',
    keys: ['store'],
    values: (s) => ({ visible: resourcesTotal(storeOf(s)) > 0 }),
  },
  { id: 'glowBig', values: () => ({}) },
];

// ---- extractor（extractor.metadata.js；cooldown 自转 R5） ----

export const PARTS_EXTRACTOR: PartDecl[] = [
  {
    id: 'main',
    keys: ['user'],
    values: (s, env) => ({ tint: playerColor(ownerOf(s), env.isOwn) }),
  },
];

// ---- controller（controller.metadata.js） ----

export function controllerDowngradeOpacity(s: RoomObjectState, time: number): number {
  // (CONTROLLER_DOWNGRADE[level] − (downgradeTime−time)) / CONTROLLER_DOWNGRADE[level]
  // （controller.metadata.js:53-63；!gameTime 守卫同官方——time=0 恒 0）
  const level = num(s.level);
  const downgradeTime = optNum(s.downgradeTime);
  if (!downgradeTime || !level || !time) return 0;
  const total = CONTROLLER_DOWNGRADE[level] ?? 0;
  if (!total) return 0;
  return (total - (downgradeTime - time)) / total;
}

export function controllerStatusValues(
  s: RoomObjectState,
  env: PartEnv,
): { own: boolean; enemy: boolean; safe: boolean } {
  const time = env.time;
  const reservation = s.reservation as Record<string, unknown> | null | undefined;
  const upgradeBlocked = optNum(s.upgradeBlocked);
  const safeMode = optNum(s.safeMode);
  const resUser = reservation && typeof reservation.user === 'string' ? reservation.user : null;
  return {
    // 官方 reservation.user === player（controller.metadata.js:81-99）
    own: !!(reservation && resUser !== null && env.isOwn(resUser)),
    enemy:
      !!(upgradeBlocked && upgradeBlocked > time) ||
      !!(reservation && (resUser === null || !env.isOwn(resUser))),
    safe: !!(safeMode && safeMode > time),
  };
}

export const PARTS_CONTROLLER: PartDecl[] = [
  {
    id: 'status',
    keys: ['reservation', 'upgradeBlocked', 'safeMode'],
    values: (s, env) => {
      const v = controllerStatusValues(s, env);
      return { own: v.own, enemy: v.enemy, safe: v.safe };
    },
  },
  { id: 'white', values: () => ({}) },
  { id: 'base', values: () => ({}) },
  {
    id: 'levels',
    keys: ['level'],
    values: (s) => ({ level: num(s.level) }),
  },
  { id: 'badge', values: () => ({}) },
  {
    id: 'progress',
    keys: ['progress', 'level'],
    values: (s): PartValues | null => {
      const progress = num(s.progress);
      if (progress <= 0) return null;
      // progressTotal = CONTROLLER_LEVELS[level]（controller.metadata.js:64-70）
      const total = CONTROLLER_LEVELS[num(s.level)] ?? 0;
      if (total <= 0) return null;
      const angle = 2 * Math.PI * (Math.min(progress, total) / total);
      return { angle };
    },
  },
  { id: 'ring', values: () => ({}) },
  {
    id: 'downgrade',
    keys: ['level', 'downgradeTime'],
    values: (s, env) => ({ alpha: controllerDowngradeOpacity(s, env.time) }),
  },
  {
    id: 'glowBase',
    keys: ['user'],
    values: (s) => ({ visible: !!ownerOf(s) }),
  },
  { id: 'glowBig', values: () => ({}) },
];

// ---- constructedWall（constructedWall.metadata.js） ----

export const PARTS_CONSTRUCTED_WALL: PartDecl[] = [
  { id: 'body', values: () => ({}) },
  {
    id: 'flash',
    keys: ['hits', 'hitsMax'],
    values: (s) => ({
      visible: num(s.hits) === 1 && num(s.hitsMax) > 1,
      alpha: CONSTRUCTED_WALL_SPEC.flashAlpha,
    }),
  },
];

// ---- constructionSite（constructionSite.metadata.js + siteProgress.js） ----

export const PARTS_CONSTRUCTION_SITE: PartDecl[] = [
  {
    id: 'progress',
    keys: ['progress', 'progressTotal', 'user'],
    values: (s, env) => {
      const progress = num(s.progress);
      const total = num(s.progressTotal);
      return {
        angle: total > 0 ? 2 * Math.PI * (Math.min(progress, total) / total) : 0,
        color: playerColor(ownerOf(s), env.isOwn),
        alpha: CONSTRUCTION_SITE_SPEC.blinkAlpha,
      };
    },
  },
];

// ---- rampart 公有叠加（rampart.metadata.js；私有顶罩在 objects-stage） ----

export const PARTS_RAMPART: PartDecl[] = [
  {
    id: 'public',
    keys: ['isPublic', 'user'],
    values: (s, env): PartValues | null => {
      if (!bool(s.isPublic)) return null;
      // 公有精灵 tint=userOwnerColor 分支的 playerColor（rampart.metadata.js:5-7
      // ——0x44FF44/0xFF4444 只是非 userOwnerColor 分支；G9 色板）
      return { tint: playerColor(ownerOf(s), env.isOwn) };
    },
  },
];

// ---- invaderCore（invaderCore.metadata.js） ----

export const PARTS_INVADER_CORE: PartDecl[] = [
  { id: 'body', values: () => ({}) },
  {
    id: 'progress',
    keys: ['decayTime'],
    values: (s, env): PartValues | null => {
      // ttl = floor((decayTime−time)/100)×100（invaderCore.metadata.js:21-25）；
      // siteProgress 仅 progress>0 画扇形（siteProgress.js:38）
      const ttl = Math.floor((num(s.decayTime) - env.time) / 100) * 100;
      if (ttl <= 0) return null;
      const total = INVADER_CORE_SPEC.decayTicks;
      return { angle: 2 * Math.PI * (Math.min(ttl, total) / total) };
    },
  },
  {
    id: 'arc',
    keys: ['spawning'],
    values: (s, env) => ({ angle: spawningAngle(s.spawning, env.time) }),
  },
  { id: 'glowBase', values: () => ({}) },
  { id: 'glowBig', values: () => ({}) },
];

// ---- flag（flag.metadata.js；名字文字 R6） ----

export function flagColors(s: RoomObjectState): { primary: number | null; secondary: number | null } {
  const c = num(s.color);
  const sc = num(s.secondaryColor);
  return {
    primary: FLAG_SPEC.colors[c] ?? null,
    secondary: FLAG_SPEC.colors[sc] ?? null,
  };
}

export const PARTS_FLAG: PartDecl[] = [
  {
    id: 'primary',
    keys: ['color'],
    values: (s): PartValues | null => {
      const tint = flagColors(s).primary;
      if (tint === null) return null;
      return { tint };
    },
  },
  {
    id: 'secondary',
    keys: ['secondaryColor'],
    values: (s): PartValues | null => {
      const tint = flagColors(s).secondary;
      if (tint === null) return null;
      return { tint };
    },
  },
];

// ---- 公共 _isDisabled（_all.metadata.js:78-95；每类型追加） ----

export const PART_DISABLED: PartDecl = {
  id: 'disabled',
  keys: ['_isDisabled'],
  values: (s) => (bool(s._isDisabled) ? { visible: true } : null),
};
