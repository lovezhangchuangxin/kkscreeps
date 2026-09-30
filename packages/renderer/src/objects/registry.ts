/**
 * 对象渲染 registry：纯部件声明表（PART_DECL_TABLE）× pixi 渲染半件
 * （joins-*）× z 槽位（Z_TABLE）× 层路由（PART_LAYERS）的装配点。
 * 构建期强校验（join 缺件/层键漂移即抛错——防静默缺件）。
 */
import type { PartDecl, PartRenderer } from './part-decl.js';
import { PART_DECL_TABLE, PART_LAYERS, assertUniquePartIds } from './parts-table.js';
import {
  joinContainer,
  joinDeposit,
  joinKeeperLair,
  joinMineral,
  joinPortal,
  joinResource,
  joinRuin,
  joinSource,
  joinTombstone,
  type JoinAssets,
  type PartJoin,
} from './joins-naturals.js';
import {
  joinConstructedWall,
  joinConstructionSite,
  joinController,
  joinExtractor,
  joinExtension,
  joinFactory,
  joinFlag,
  joinInvaderCore,
  joinLab,
  joinLink,
  joinNuker,
  joinObserver,
  joinPowerBank,
  joinPowerSpawn,
  joinRampart,
  joinSpawn,
  joinStorage,
  joinTerminal,
  joinTower,
  joinDisabled,
} from './joins-structures.js';
import { Z_TABLE } from './z-table.js';

/** 装配后的对象描述符（pixi 侧消费）。 */
export interface RegisteredDescriptor {
  readonly typeKey: string;
  readonly z: number;
  readonly parts: readonly PartDecl[];
  readonly renderers: ReadonlyMap<string, PartRenderer>;
  /** 部件层路由（缺省 slot）。 */
  layerOf(partId: string): 'slot' | 'lighting' | 'effects';
}

export function buildRegistry(assets: JoinAssets): Map<string, RegisteredDescriptor> {
  const joins: Record<string, PartJoin> = {
    's:source': joinSource(assets),
    's:mineral': joinMineral(assets),
    resource: joinResource(assets),
    's:deposit': joinDeposit(assets),
    's:container': joinContainer(assets),
    tombstone: joinTombstone(assets),
    ruin: joinRuin(assets),
    's:portal': joinPortal(assets),
    's:keeperLair': joinKeeperLair(assets),
    's:extension': joinExtension(assets),
    's:storage': joinStorage(assets),
    's:factory': joinFactory(assets),
    's:nuker': joinNuker(assets),
    spawn: joinSpawn(assets),
    's:link': joinLink(assets),
    's:observer': joinObserver(assets),
    's:powerBank': joinPowerBank(assets),
    's:powerSpawn': joinPowerSpawn(assets),
    's:tower': joinTower(assets),
    's:lab': joinLab(assets),
    's:terminal': joinTerminal(assets),
    's:extractor': joinExtractor(assets),
    's:controller': joinController(assets),
    's:constructedWall': joinConstructedWall(assets),
    constructionSite: joinConstructionSite(),
    's:rampart': joinRampart(assets),
    's:invaderCore': joinInvaderCore(assets),
    flag: joinFlag(assets),
  };
  const disabledJoin = joinDisabled();

  const registry = new Map<string, RegisteredDescriptor>();
  for (const [typeKey, parts] of Object.entries(PART_DECL_TABLE)) {
    assertUniquePartIds(parts, typeKey);
    const own = joins[typeKey];
    if (!own) throw new Error(`registry 缺 join：${typeKey}`);
    const renderers = new Map<string, PartRenderer>();
    for (const p of parts) {
      const renderer = p.id === 'disabled' ? disabledJoin.disabled : own[p.id];
      if (!renderer) throw new Error(`registry join 缺件：${typeKey}.${p.id}`);
      renderers.set(p.id, renderer);
    }
    const z = Z_TABLE[typeKey];
    if (z === undefined) throw new Error(`registry Z_TABLE 缺槽位：${typeKey}`);
    registry.set(typeKey, {
      typeKey,
      z,
      parts,
      renderers,
      layerOf(partId: string): 'slot' | 'lighting' | 'effects' {
        return PART_LAYERS[`${typeKey}.${partId}`] ?? 'slot';
      },
    });
  }
  // 层路由键反查（指向不存在部件的键=拼写漂移）
  for (const key of Object.keys(PART_LAYERS)) {
    const dot = key.lastIndexOf('.');
    const typeKey = key.slice(0, dot);
    const partId = key.slice(dot + 1);
    const desc = registry.get(typeKey);
    if (!desc || !desc.renderers.has(partId)) {
      throw new Error(`registry PART_LAYERS 键无主：${key}`);
    }
  }
  return registry;
}
