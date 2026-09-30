/**
 * objects 层 z 序全表（〔官拆〕§2；逐类型对 metadata/src/objects/*.metadata.js
 * 顶层 zIndex 取证，file:line 见各行注释）。静态槽位表——objects 层内每类型
 * 一个固定 zIndex 子容器，零每帧排序（方案 §2.2）。
 *
 * 本文件必须零 pixi 导入（单测直采断言全表）。
 */
import type { RoomObjectState } from '@kkscreeps/shared';

/** 对象类型键 → objects 层 zIndex。官方 rootContainer.zIndex = metadata.zIndex || 0
 * （engine/src/lib/GameObject.js:28）。 */
export const Z_TABLE: Record<string, number> = {
  /** road.metadata.js:12——z=-1。road 在我方走 ground 层（方案 §2.2），
   * 表项保留作权威对照与测试锚。 */
  's:road': -1,
  /** rampart.metadata.js:25——z=0（公有 rampart 精灵路由 effects 层；
   * 私有 rampart 由 terrain 预处理器画顶罩，同 effects）。 */
  's:rampart': 0,
  /** constructedWall.metadata.js:38 */
  's:constructedWall': 0,
  /** extractor.metadata.js:33 */
  's:extractor': 0,
  /** flag.metadata.js 无 zIndex → ||0（GameObject.js:28）。 */
  flag: 0,
  /** energy.metadata.js:19（掉落资源，我方 type:"resource"）。 */
  resource: 1,
  /** deposit.metadata.js:124 */
  's:deposit': 1,
  /** source.metadata.js:175 */
  's:source': 2,
  /** mineral.metadata.js:121 */
  's:mineral': 2,
  /** portal.metadata.js:187 */
  's:portal': 3,
  /** keeperLair.metadata.js:118 */
  's:keeperLair': 3,
  /** controller.metadata.js:466 */
  's:controller': 4,
  /** container.metadata.js:109 */
  's:container': 4,
  /** tombstone.metadata.js:70 */
  tombstone: 5,
  /** ruin.metadata.js:70 */
  ruin: 5,
  /** creep.metadata.js（z=6）——creep 本体 R4 落位，槽位先占。 */
  creep: 6,
  /** extension.metadata.js:159 */
  's:extension': 7,
  /** factory.metadata.js:241 */
  's:factory': 7,
  /** storage.metadata.js:128 */
  's:storage': 7,
  /** nuker.metadata.js:125 */
  's:nuker': 7,
  /** spawn.metadata.js:270 */
  spawn: 8,
  /** link.metadata.js:100 */
  's:link': 9,
  /** observer.metadata.js:128 */
  's:observer': 10,
  /** powerBank.metadata.js:132 */
  's:powerBank': 11,
  /** powerSpawn.metadata.js:267 */
  's:powerSpawn': 12,
  /** tower.metadata.js:250 */
  's:tower': 13,
  /** powerCreep z=13（〔官拆〕§2 表）——R4 落位，槽位先占。 */
  powerCreep: 13,
  /** lab.metadata.js:236 */
  's:lab': 15,
  /** terminal.metadata.js:183 */
  's:terminal': 16,
  /** invaderCore.metadata.js:102 */
  's:invaderCore': 17,
  /** constructionSite.metadata.js:26 */
  constructionSite: 18,
} as const;

/**
 * 契约对象 → 类型键（registry 主键）。
 * 投影形态（shared/feed.ts 头注）：结构类 type:"structure"+structureType 二级；
 * spawn/constructionSite 自带 structureType 但 type 不是 "structure"。
 */
/** structureType → typeKey 微缓存（键=state 自有串，命中零分配；键空间
 * ≤ 契约 structureType 组合表——评审 C建议2 及二轮更正：首版以 `s|${x}`
 * 模板串作键仍逐次分配，未达成宣称）。 */
const typeKeyCache = new Map<string, string>();

export function resolveTypeKey(state: RoomObjectState): string {
  const { type } = state;
  if (type === 'structure') {
    const structureType = strType(state.structureType);
    let resolved = typeKeyCache.get(structureType);
    if (resolved === undefined) {
      resolved = `s:${structureType}`;
      typeKeyCache.set(structureType, resolved);
    }
    return resolved;
  }
  return strType(type);
}

function strType(v: unknown): string {
  return typeof v === 'string' ? v : '';
}
