/**
 * creep/powerCreep 部件声明（纯逻辑半件；官方 creep.metadata.js /
 * powerCreep.metadata.js 的 calculations+when 对应物）。
 *
 * 本体视觉（黑圆/body 环/徽标/资源圆/tough/NPC）收敛在 `main` 一件——官方
 * 全部是 mainContainer 子件、随 mainContainer 旋转（actions.rotateTo 的
 * targetId）；R4 运动系统对 main 节点做朝向插值。lighting（mask/glow）与
 * 名字（effects）按官方 layer 声明分件。
 */
import type { RoomObjectState } from '@kkscreeps/shared';
import type { PartDecl } from './part-decl.js';
import { num, optNum, ownerOf, storeCapacityOf, storeOf, str } from './part-decl.js';
import { bodyRingArcs, hasLivingTough } from './body-ring.js';
import { playerColor } from './player-color.js';
import { CREEP_SPEC } from './specs.js';

/** 运动/动效系统挂接的对象类型（官方有 actions.moveTo 族 + disappearProcessor）。 */
export const MOTION_TYPE_KEYS: ReadonlySet<string> = new Set(['creep', 'powerCreep']);

/** NPC 判定（官方 isNpc：user==='3'||'2'，creep.metadata.js:12）。 */
function isNpcUser(user: string | null): boolean {
  return user === '3' || user === '2';
}

/** 敌方 safemode α（官方 calculations.safeMode，creep.metadata.js:49-62）。 */
function safeModeAlpha(s: RoomObjectState, env: Parameters<PartDecl['values']>[1]): number {
  const ctl = env.controller;
  const user = ownerOf(s);
  return ctl && ctl.safeModeUntil > env.time && ctl.user !== user
    ? CREEP_SPEC.safeModeAlpha
    : 1;
}

/** body 字段防御性读取（契约 body: {type,hits}[]）。 */
function bodyOf(s: RoomObjectState): readonly { type?: unknown; hits?: unknown }[] {
  return Array.isArray(s.body) ? (s.body as never) : [];
}

/** 资源圆半径族（官方 calculations resource*Radius，creep.metadata.js:33-47；
 * when 门折叠进半径 0——白圆 when 的 || 优先级 quirk 见 specs G12 注）。 */
function resourceRadii(s: RoomObjectState): { total: number; power: number; energy: number } {
  const store = storeOf(s);
  const cap = storeCapacityOf(s);
  let total = 0;
  for (const v of Object.values(store)) {
    if (typeof v === 'number') total += v;
  }
  const energy = num(store.energy);
  const power = num(store.power);
  const r = CREEP_SPEC.resourceRadius;
  return {
    // cap=0：除出 Infinity→min 钳 1（官方 JS 同语义；total=0 被 when 挡住不触发 NaN）
    total: total > 0 ? Math.min(1, total / cap) * r : 0,
    power: power > 0 ? Math.min(1, (energy + power) / cap) * r : 0,
    energy: energy > 0 ? Math.min(1, energy / cap) * r : 0,
  };
}

/** name 件公共值面（displayName 模板；v0 只显示己方——R3 §12 决策⑥）。 */
function displayNameValues(
  s: RoomObjectState,
  env: Parameters<PartDecl['values']>[1],
): { text: string; fill: number } | null {
  const user = ownerOf(s);
  if (!user || !env.isOwn(user)) return null;
  if (s.spawning === true) return null;
  return { text: str(s.name), fill: playerColor(user, env.isOwn) };
}

// ---- creep（creep.metadata.js:15-341） ----

export const PARTS_CREEP: PartDecl[] = [
  {
    id: 'main',
    keys: ['body', 'store', 'storeCapacity', 'user', 'spawning'],
    values: (s, env) => {
      // body 环 → 扁平弧段 [color, a1, a2, ccw?1:0]×n（值门控：数组逐元素）
      const flat: number[] = [];
      for (const a of bodyRingArcs(bodyOf(s))) {
        flat.push(a.color, a.startAngle, a.endAngle, a.anticlockwise ? 1 : 0);
      }
      const radii = resourceRadii(s);
      return {
        ring: flat,
        tough: hasLivingTough(bodyOf(s)) ? 1 : 0,
        npc: isNpcUser(ownerOf(s)) ? 1 : 0,
        totalR: radii.total,
        powerR: radii.power,
        energyR: radii.energy,
        alpha: safeModeAlpha(s, env),
      };
    },
  },
  {
    // creep-mask 100×100（:186-200：when !spawning && isNotNpc）。官方是
    // mainContainer 子件（α 随其传导）——我方独立 overlay 节点以 alpha 值
    // 传导 safemode 暗化（评审 A-B3）
    id: 'mask',
    keys: ['spawning', 'user'],
    values: (s, env) => {
      if (s.spawning === true) return null;
      if (isNpcUser(ownerOf(s))) return null;
      return { alpha: safeModeAlpha(s, env) };
    },
  },
  {
    // glow 400×400 α0.2（:214-226：once+shouldRun !spawning——无 NPC 门，
    // NPC 两只 glow 并存是官方运行时行为）
    id: 'glow',
    keys: ['spawning', 'user'],
    values: (s, env) => (s.spawning === true ? null : { alpha: safeModeAlpha(s, env) }),
  },
  {
    // NPC glow 100×100 α0.5（:201-213：when isNpc）
    id: 'glowNpc',
    keys: ['user'],
    values: (s, env) => (isNpcUser(ownerOf(s)) ? { alpha: safeModeAlpha(s, env) } : null),
  },
  {
    // displayName（action-templates/displayName.js + additionalWhen !spawning）
    id: 'name',
    keys: ['user', 'name', 'spawning'],
    values: (s, env) => displayNameValues(s, env),
  },
];

// ---- powerCreep（powerCreep.metadata.js:25-241；引擎 G5 家族——合成取材） ----

export const PARTS_POWER_CREEP: PartDecl[] = [
  {
    id: 'main',
    keys: ['className', 'level', 'user'],
    values: (s, env) => {
      // texture = className-lvl{min(4, ceil(level/6))}（:41-44）；className
      // 缺位防御性隐藏（官方 BADGE_GEOMETRY[undefined] 即崩——脏数据面）
      const className = str(s.className);
      const level = num(s.level);
      return {
        texture: className ? `${className}-lvl${Math.min(4, Math.ceil(level / 6))}` : '',
        alpha: safeModeAlpha(s, env),
      };
    },
  },
  {
    // lighting 层同纹理精灵 180×180 α1（:138-149 用 texture 计算——:46-48 的
    // texture-light（lvl4）是死代码，保真运行时行为）
    id: 'light',
    keys: ['className', 'level', 'user'],
    values: (s, env) => {
      const className = str(s.className);
      if (!className) return null;
      const level = num(s.level);
      return { texture: `${className}-lvl${Math.min(4, Math.ceil(level / 6))}`, visible: true, alpha: safeModeAlpha(s, env) };
    },
  },
  {
    // glow 400×400 α1.0 tint 0xFF5555（:151-162）——alpha 传导 safemode
    id: 'glow',
    keys: ['user'],
    values: (s, env) => ({ alpha: safeModeAlpha(s, env) }),
  },
  {
    id: 'name',
    keys: ['user', 'name'],
    values: (s, env) => {
      // powerCreep displayName 无 additionalWhen（:124）
      const user = ownerOf(s);
      if (!user || !env.isOwn(user)) return null;
      return { text: str(s.name), fill: playerColor(user, env.isOwn) };
    },
  },
];

/** powerCreep 等级→纹理档（单测锚；官方 :41-44）。 */
export function powerCreepTexture(className: string, level: number): string {
  return `${className}-lvl${Math.min(4, Math.ceil(level / 6))}`;
}

/** controller 摘要扫描（官方 stateExtra.controller=objects.find 首个）。 */
export function controllerSummary(
  objects: readonly RoomObjectState[],
): { user: string | null; safeModeUntil: number } | null {
  for (const o of objects) {
    if (o.type === 'structure' && o.structureType === 'controller') {
      const user = ownerOf(o);
      return { user, safeModeUntil: optNum(o.safeMode) ?? 0 };
    }
  }
  return null;
}
