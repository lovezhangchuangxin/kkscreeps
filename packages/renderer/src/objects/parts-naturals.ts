/**
 * 部件声明表（自然物家族）——纯逻辑半件（values），pixi 半件在
 * descriptors/pixi-join.ts 按 partId 挂接。规格常量一律引 specs.ts，
 * 语义注释附官方 metadata file:line。零 pixi 导入。
 */
import type { RoomObjectState } from '@kkscreeps/shared';
import {
  ownerOf,
  storeCapacityOf,
  storeOf,
  type PartDecl,
  type PartValues,
} from './part-decl.js';
import { playerColor } from './player-color.js';
import {
  DECAY_SPEC,
  DEPOSIT_SPEC,
  MINERAL_SPEC,
  RESOURCE_SPEC,
} from './specs.js';

function num(v: unknown): number {
  return typeof v === 'number' ? v : 0;
}

/** resourcesTotal = Σstore（calculation-templates/resourceTotal.js）。
 * 值域防御：非 number 值计 0（官方直接 `cur + sum`，脏值会串成字符串拼接）。 */
export function resourcesTotal(store: Record<string, number>): number {
  let sum = 0;
  for (const k in store) {
    const v = store[k];
    if (typeof v === 'number') sum += v;
  }
  return sum;
}

const SOURCE_ENERGY_SIZE = 60;

// ---- source（source.metadata.js） ----

export const PARTS_SOURCE: PartDecl[] = [
  {
    id: 'frame',
    values: () => ({}),
  },
  {
    id: 'energyRect',
    keys: ['energy', 'energyCapacity'],
    values: (s): PartValues | null => {
      // energySize = 60×energy/energyCapacity（source.metadata.js:27-33）
      const capacity = num(s.energyCapacity);
      const size = capacity > 0 ? (SOURCE_ENERGY_SIZE * num(s.energy)) / capacity : 0;
      return { size };
    },
  },
  {
    id: 'glowBig',
    values: () => ({}),
  },
  {
    id: 'glowSmall',
    keys: ['energy'],
    values: (s): PartValues | null => ({ visible: num(s.energy) > 0 }),
  },
];

// ---- mineral（mineral.metadata.js） ----

export function mineralColors(mineralType: unknown): { foreground: number; background: number } {
  // 官方 COLORS[mineralType] 未类型缺失即崩——防御性退 O 灰对（§12 分歧登记）
  return MINERAL_SPEC.colors[typeof mineralType === 'string' ? mineralType : ''] ??
    MINERAL_SPEC.colors.O!;
}

export const PARTS_MINERAL: PartDecl[] = [
  {
    id: 'disc',
    keys: ['mineralType'],
    values: (s) => {
      const c = mineralColors(s.mineralType);
      return { fg: c.foreground, bg: c.background };
    },
  },
  {
    id: 'symbol',
    keys: ['mineralType'],
    values: (s) => {
      const c = mineralColors(s.mineralType);
      return { fg: c.foreground, text: typeof s.mineralType === 'string' ? s.mineralType : '' };
    },
  },
  { id: 'glowSmall', values: () => ({}) },
  {
    id: 'glowBig',
    keys: ['mineralType'],
    values: (s) => ({ tint: mineralColors(s.mineralType).foreground }),
  },
];

// ---- 掉落资源（energy.metadata.js + resourceCircle.js） ----

export function resourceCircleValues(
  s: RoomObjectState,
): { color: number; radius: number; radiusLighting: number } {
  const resourceType = typeof s.resourceType === 'string' ? s.resourceType : 'energy';
  // 官方读 state[resourceType]（legacy 同名字段）；kk 投影为 amount（R1 §12）
  const amount = num(s[resourceType] ?? s.amount);
  const capacity = num(s[`${resourceType}Capacity`]) || RESOURCE_SPEC.defaultCapacity;
  const color = (RESOURCE_SPEC.colors as Record<string, number | undefined>)[resourceType] ??
    0xffffff;
  const baseRadius = (RESOURCE_SPEC.radii as Record<string, number | undefined>)[resourceType] ??
    RESOURCE_SPEC.defaultRadius;
  const radius = baseRadius * Math.min(1, amount / capacity);
  // lighting 圆半径整数化（C-B1 防退化：radiusLighting 是 lighting 层唯一连续量，
  // 亚像素变化会被值门控放行造成逐 tick 2048² 离屏重渲；整数量化后视觉差不可辨）
  return { color, radius, radiusLighting: Math.round(radius) };
}

export const PARTS_RESOURCE: PartDecl[] = [
  {
    id: 'circle',
    keys: ['resourceType', 'amount'],
    values: (s) => resourceCircleValues(s),
  },
  {
    id: 'glow',
    keys: ['resourceType', 'amount'],
    values: (s) => resourceCircleValues(s),
  },
];

// ---- deposit（deposit.metadata.js） ----

export const PARTS_DEPOSIT: PartDecl[] = [
  {
    id: 'sprites',
    keys: ['depositType', 'harvested'],
    values: (s): PartValues | null => {
      const kind = typeof s.depositType === 'string' ? s.depositType : '';
      const color = DEPOSIT_SPEC.colors[kind] ?? 0xffffff;
      // fill alpha = max(0, 0.6 − harvested/1e5)（deposit.metadata.js:53-58）
      const fillAlpha = Math.max(0, DEPOSIT_SPEC.fillBase - num(s.harvested) / DEPOSIT_SPEC.fillDivisor);
      return { color, fillAlpha, kind };
    },
  },
  {
    id: 'glow',
    keys: ['depositType'],
    values: (s) => {
      const kind = typeof s.depositType === 'string' ? s.depositType : '';
      return { tint: DEPOSIT_SPEC.colors[kind] ?? 0xffffff, kind };
    },
  },
];

// ---- container（container.metadata.js） ----

export interface BarHeights {
  hasOther: boolean;
  hasPower: boolean;
  hasEnergy: boolean;
  otherH: number;
  powerH: number;
  energyH: number;
}

/** container/factory 族条高：×H/(storeCapacity||resourcesTotal)
 * （container.metadata.js:13-26）。 */
export function containerBarHeights(s: RoomObjectState, heightScale: number): BarHeights {
  const store = storeOf(s);
  const capacity = storeCapacityOf(s);
  const total = resourcesTotal(store);
  const denom = capacity || total || 1;
  const energy = store.energy ?? 0;
  const power = store.power ?? 0;
  const otherH = (total * heightScale) / denom;
  const powerH = ((power + energy) * heightScale) / denom;
  const energyH = (energy * heightScale) / denom;
  return {
    hasOther: energy + power < total,
    hasPower: power > 0,
    hasEnergy: energy > 0,
    otherH,
    powerH,
    energyH,
  };
}

export const PARTS_CONTAINER: PartDecl[] = [
  { id: 'frame', values: () => ({}) },
  {
    id: 'bars',
    keys: ['store', 'storeCapacity'],
    values: (s) => {
      const b = containerBarHeights(s, CONTAINER_BAR_SCALE);
      return {
        hasOther: b.hasOther,
        hasPower: b.hasPower,
        hasEnergy: b.hasEnergy,
        otherH: b.otherH,
        powerH: b.powerH,
        energyH: b.energyH,
      };
    },
  },
  {
    id: 'glow',
    keys: ['store'],
    values: (s) => ({ visible: resourcesTotal(storeOf(s)) > 0 }),
  },
];

const CONTAINER_BAR_SCALE = 50;

// ---- tombstone / ruin（tombstone/ruin.metadata.js） ----

/** decayAlpha = 1 − (time−start)/(decayTime−start)，分母≤0 → 0。
 * tombstone 用 deathTime（tombstone.metadata.js:16-19）、ruin 用 destroyTime
 * （ruin.metadata.js:16-19）——键名官方即不同。 */
export function decayAlpha(
  s: RoomObjectState,
  time: number,
  startKey: 'deathTime' | 'destroyTime' = 'deathTime',
): number {
  const start = num(s[startKey]);
  const end = num(s.decayTime);
  const denom = end - start;
  if (denom <= 0) return 0;
  return Math.min(1, Math.max(0, 1 - (time - start) / denom));
}

function decayPart(textureKey: 'tombstone' | 'ruin'): PartDecl[] {
  return [
    {
      id: 'sprites',
      keys: ['store', 'user', 'deathTime', 'decayTime', 'destroyTime'],
      values: (s, env) => {
        const total = resourcesTotal(storeOf(s));
        return {
          borderAlpha: decayAlpha(s, env.time, textureKey === 'ruin' ? 'destroyTime' : 'deathTime'),
          // resourceColor = resourcesTotal>0 ? playerColor : 0x000000（tombstone:11-14）
          resourceTint: total > 0
            ? playerColor(ownerOf(s), env.isOwn)
            : DECAY_SPEC.emptyColor,
        };
      },
    },
    { id: 'glow', values: () => ({}) },
  ];
}

export const PARTS_TOMBSTONE: PartDecl[] = decayPart('tombstone');
export const PARTS_RUIN: PartDecl[] = decayPart('ruin');

// ---- portal（portal.metadata.js：静态底盘 + 双呼吸波 R5） ----

export const PARTS_PORTAL: PartDecl[] = [
  { id: 'disc', values: () => ({}) },
  { id: 'wave1', values: () => ({}) },
  { id: 'wave2', values: () => ({}) },
  { id: 'glowBig', values: () => ({}) },
  { id: 'glowSmall', values: () => ({}) },
];

// ---- keeperLair（keeperLair.metadata.js：同 portal 形态） ----

export const PARTS_KEEPER_LAIR: PartDecl[] = [
  { id: 'disc', values: () => ({}) },
  { id: 'wave1', values: () => ({}) },
  { id: 'wave2', values: () => ({}) },
  { id: 'glowBig', values: () => ({}) },
  { id: 'glowSmall', values: () => ({}) },
];
