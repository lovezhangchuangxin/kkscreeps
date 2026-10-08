/**
 * 玩家色（〔官拆〕§2/_all.metadata.js；灰区 G9）。
 *
 * 官方 playerColor 有两模式（_all.metadata.js:24-52）：
 *   - users[user].color 远程用户色（真实客户端）；
 *   - userOwnerColor 模式：own #8fbb93 / enemy #FF7777 / 未知 #BBBBBB(#ffffff hex)。
 * 官方 demo 基准即 userOwnerColor:true（demo/src/config/worldConfigs.js:32）。
 * 我方契约 v0 无 users 映射（引擎不投影用户色）——v0 采用 userOwnerColor 模式
 * 色板（G9 挂账，M3 users 落位后切换）。
 *
 * rampart 顶罩色（terrain.js:205-214 userOwnerColor 分支）：own #105010/#44ff44、
 * enemy #501010/#ff4444。官方非 userOwnerColor 分支用用户色×0.3 填充+用户色
 * 描边 25——users 缺位下不可达，G9 一并挂账。
 *
 * 零 pixi 导入。
 */

/** userOwnerColor 模式色板（_all.metadata.js:26-52 逐值取证）。 */
export const PLAYER_COLOR = {
  own: 0x8fbb93,
  enemy: 0xff7777,
  /** isOwner===undefined（无 user 字段）。 */
  unknown: 0xbbbbbb,
} as const;

export const PLAYER_COLOR_HEX = {
  own: '#8fbb93',
  enemy: '#FF7777',
  unknown: '#ffffff',
} as const;

/** rampart 顶罩（terrain.js:207-208；fill/stroke 对）。 */
export const RAMPART_COVER = {
  own: { fill: 0x105010, stroke: 0x44ff44 },
  enemy: { fill: 0x501010, stroke: 0xff4444 },
} as const;

export type OwnFn = (user: string | null | undefined) => boolean;

/**
 * playerColor（_all.metadata.js:38-52 userOwnerColor 分支的语义等价）：
 * user 缺位 → unknown；user===本玩家 → own；否则 enemy。
 */
export function playerColor(user: string | null | undefined, isOwn: OwnFn): number {
  if (user === null || user === undefined) return PLAYER_COLOR.unknown;
  return isOwn(user) ? PLAYER_COLOR.own : PLAYER_COLOR.enemy;
}

/** playerColorHex（_all.metadata.js:24-36；文字面消费）。 */
export function playerColorHex(user: string | null | undefined, isOwn: OwnFn): string {
  if (user === null || user === undefined) return PLAYER_COLOR_HEX.unknown;
  return isOwn(user) ? PLAYER_COLOR_HEX.own : PLAYER_COLOR_HEX.enemy;
}

/** rampartColor（rampart.metadata.js:4-8 userOwnerColor 分支）。 */
export function rampartCoverColor(
  user: string | null | undefined,
  isOwn: OwnFn,
): {
  fill: number;
  stroke: number;
} {
  return isOwn(user ?? null) ? RAMPART_COVER.own : RAMPART_COVER.enemy;
}

/** spawn 徽标色（spawn.metadata.js:52-57：userOwnerColor ? 0x555555 : playerColor）。 */
export const SPAWN_BADGE_COLOR = 0x555555;
