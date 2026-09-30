/**
 * 部件声明汇总表（typeKey → PartDecl[]；官方 objects 索引 metadata/src/index.js
 * 的静态表对应物）+ _all 公共件追加（官方每对象合并 _all.metadata processors）。
 * 零 pixi 导入——单测与本表直采，pixi join 层（descriptors/pixi-join.ts）
 * 按 partId 挂渲染半件，join 不全在 registry 构建期抛错。
 */
import type { PartDecl } from './part-decl.js';
import { PARTS_CREEP, PARTS_POWER_CREEP } from './parts-creeps.js';
import {
  PARTS_CONTAINER,
  PARTS_DEPOSIT,
  PARTS_KEEPER_LAIR,
  PARTS_MINERAL,
  PARTS_PORTAL,
  PARTS_RESOURCE,
  PARTS_RUIN,
  PARTS_SOURCE,
  PARTS_TOMBSTONE,
} from './parts-naturals.js';
import {
  PART_DISABLED,
  PARTS_CONSTRUCTED_WALL,
  PARTS_CONSTRUCTION_SITE,
  PARTS_CONTROLLER,
  PARTS_EXTRACTOR,
  PARTS_EXTENSION,
  PARTS_FACTORY,
  PARTS_FLAG,
  PARTS_INVADER_CORE,
  PARTS_LAB,
  PARTS_LINK,
  PARTS_NUKER,
  PARTS_OBSERVER,
  PARTS_POWER_BANK,
  PARTS_POWER_SPAWN,
  PARTS_RAMPART,
  PARTS_SPAWN,
  PARTS_STORAGE,
  PARTS_TERMINAL,
  PARTS_TOWER,
} from './parts-structures.js';

function withDisabled(parts: PartDecl[]): PartDecl[] {
  return [...parts, PART_DISABLED];
}

/** typeKey（resolveTypeKey）→ 部件表。官方 _all 公共件合并到**全部**对象类型
 * （engine/src/lib/World.js:36-53 前置合并；评审 A-S1）——disabled 件全表追加
 * （creep/powerCreep 同受 _all 合并）。road/s:road 不在本表（road 走 ground
 * 层连通绘制）。 */
export const PART_DECL_TABLE: Readonly<Record<string, readonly PartDecl[]>> = {
  's:source': withDisabled(PARTS_SOURCE),
  's:mineral': withDisabled(PARTS_MINERAL),
  resource: withDisabled(PARTS_RESOURCE),
  's:deposit': withDisabled(PARTS_DEPOSIT),
  's:container': withDisabled(PARTS_CONTAINER),
  tombstone: withDisabled(PARTS_TOMBSTONE),
  ruin: withDisabled(PARTS_RUIN),
  's:portal': withDisabled(PARTS_PORTAL),
  's:keeperLair': withDisabled(PARTS_KEEPER_LAIR),
  's:extension': withDisabled(PARTS_EXTENSION),
  's:storage': withDisabled(PARTS_STORAGE),
  's:factory': withDisabled(PARTS_FACTORY),
  's:nuker': withDisabled(PARTS_NUKER),
  spawn: withDisabled(PARTS_SPAWN),
  's:link': withDisabled(PARTS_LINK),
  's:observer': withDisabled(PARTS_OBSERVER),
  's:powerBank': withDisabled(PARTS_POWER_BANK),
  's:powerSpawn': withDisabled(PARTS_POWER_SPAWN),
  's:tower': withDisabled(PARTS_TOWER),
  's:lab': withDisabled(PARTS_LAB),
  's:terminal': withDisabled(PARTS_TERMINAL),
  's:extractor': withDisabled(PARTS_EXTRACTOR),
  's:controller': withDisabled(PARTS_CONTROLLER),
  's:constructedWall': withDisabled(PARTS_CONSTRUCTED_WALL),
  constructionSite: withDisabled(PARTS_CONSTRUCTION_SITE),
  's:rampart': withDisabled(PARTS_RAMPART),
  's:invaderCore': withDisabled(PARTS_INVADER_CORE),
  flag: withDisabled(PARTS_FLAG),
  creep: withDisabled(PARTS_CREEP),
  powerCreep: withDisabled(PARTS_POWER_CREEP),
};

/** 部件表内 id 唯一性校验（registry 构建期调用）。 */
export function assertUniquePartIds(parts: readonly PartDecl[], typeKey: string): void {
  const seen = new Set<string>();
  for (const p of parts) {
    if (seen.has(p.id)) throw new Error(`部件 id 重复：${typeKey}.${p.id}`);
    seen.add(p.id);
  }
}

/**
 * 层路由表（官方 processor.layer → 我方固定层栈）：缺省=objects 层类型槽内
 * （slot）；lighting=离屏内容（SCREEN 成员）；effects=顶层。键=`typeKey.partId`。
 * registry 构建期校验键指向存在的部件（防拼写漂移）。
 */
export const PART_LAYERS: Readonly<Record<string, 'lighting' | 'effects'>> = {
  's:source.glowBig': 'lighting',
  's:source.glowSmall': 'lighting',
  's:mineral.glowSmall': 'lighting',
  's:mineral.glowBig': 'lighting',
  'resource.glow': 'lighting',
  's:deposit.glow': 'lighting',
  's:container.glow': 'lighting',
  'tombstone.glow': 'lighting',
  'ruin.glow': 'lighting',
  's:portal.glowBig': 'lighting',
  's:portal.glowSmall': 'lighting',
  's:keeperLair.glowBig': 'lighting',
  's:keeperLair.glowSmall': 'lighting',
  's:extension.glowBase': 'lighting',
  's:extension.glowTier': 'lighting',
  's:storage.glowBase': 'lighting',
  's:storage.glowBig': 'lighting',
  's:factory.glowBase': 'lighting',
  's:factory.glowBig': 'lighting',
  's:nuker.glowSmall': 'lighting',
  's:nuker.glowBig': 'lighting',
  'spawn.glowBase': 'lighting',
  'spawn.glowBig': 'lighting',
  'spawn.name': 'effects',
  's:link.glowBase': 'lighting',
  's:link.glowBig': 'lighting',
  's:observer.glow': 'lighting',
  's:powerBank.glow': 'lighting',
  's:powerSpawn.glowBase': 'lighting',
  's:powerSpawn.glowPulse': 'lighting',
  's:tower.glowBase': 'lighting',
  's:tower.glowBig': 'lighting',
  's:lab.glow': 'lighting',
  's:terminal.glowBase': 'lighting',
  's:terminal.glowBig': 'lighting',
  's:controller.glowBase': 'lighting',
  's:controller.glowBig': 'lighting',
  's:controller.downgrade': 'effects',
  's:constructedWall.flash': 'effects',
  'constructionSite.progress': 'effects',
  's:rampart.public': 'effects',
  's:invaderCore.glowBase': 'lighting',
  's:invaderCore.glowBig': 'lighting',
  'creep.mask': 'lighting',
  'creep.glow': 'lighting',
  'creep.glowNpc': 'lighting',
  'creep.name': 'effects',
  'powerCreep.light': 'lighting',
  'powerCreep.glow': 'lighting',
  'powerCreep.name': 'effects',
  'flag.primary': 'effects',
  'flag.secondary': 'effects',
  // _isDisabled 公共件（全部 withDisabled 类型）
  ...Object.keys(PART_DECL_TABLE)
    .filter((k) => PART_DECL_TABLE[k]!.some((p) => p.id === 'disabled'))
    .reduce<Record<string, 'effects'>>((acc, k) => {
      acc[`${k}.disabled`] = 'effects';
      return acc;
    }, {}),
};
