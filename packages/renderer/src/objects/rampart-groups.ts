/**
 * rampart 私有顶罩的纯逻辑半件（terrain.js:183-247）：
 * 按 user 分组的合并 path 格集 + 每组重建门控键。几何烘焙（canvas）在
 * objects-stage.ts。零 pixi 导入。
 */
import type { RoomObjectState } from '@kkscreeps/shared';
import { ownerOf, type PartEnv } from './part-decl.js';
import { rampartCoverColor } from './player-color.js';

/** 房间格数（地形码 2500 = 50²）。 */
export const ROOM_GRID = 50;

export interface RampartGroup {
  /** 分组用户 id（ownerOf 消费面：user ?? owner）。 */
  readonly user: string | null;
  /** 该组 rampart 格（行主序索引 y×50+x，升序）。 */
  readonly cells: number[];
  /** fill/stroke 色（G9：userOwnerColor 模式色板）。 */
  readonly fill: number;
  readonly stroke: number;
}

/** 由对象列表提取私有 rampart（!isPublic）按 user 分组的格集（升序稳定）。 */
export function rampartGroups(
  objects: readonly RoomObjectState[],
  env: Pick<PartEnv, 'isOwn'>,
): RampartGroup[] {
  const byUser = new Map<string, number[]>();
  for (const o of objects) {
    if (o.type !== 'structure' || o.structureType !== 'rampart') continue;
    if (o.isPublic === true) continue;
    const user = ownerOf(o) ?? '';
    const x = typeof o.x === 'number' ? o.x : 0;
    const y = typeof o.y === 'number' ? o.y : 0;
    let cells = byUser.get(user);
    if (!cells) {
      cells = [];
      byUser.set(user, cells);
    }
    cells.push(y * ROOM_GRID + x);
  }
  const groups: RampartGroup[] = [];
  for (const [user, cells] of byUser) {
    cells.sort((a, b) => a - b);
    const color = rampartCoverColor(user || null, env.isOwn);
    groups.push({ user: user || null, cells, fill: color.fill, stroke: color.stroke });
  }
  // 码点比较（B-S6：localeCompare 有 ICU 环境差异，违背夹具确定性纪律）
  return groups.sort((a, b) => (a.user ?? '') < (b.user ?? '') ? -1 : (a.user ?? '') > (b.user ?? '') ? 1 : 0);
}

/** 组重建门控键（cells + 色；FNV 门控语义同地形）。 */
export function rampartGroupHash(group: RampartGroup): string {
  let hash = 0x811c9dc5;
  const input = `${group.fill.toString(16)},${group.stroke.toString(16)},${group.cells.join(',')}`;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, '0');
}

/** Full-room pixel bounds, including stroke/AA; room-edge clipping stays identical. */
export function rampartRasterBounds(cells: readonly number[], cell: number, view: number, renderSize: number, strokeWidth: number) {
  const scale = renderSize / view;
  cells = cells.filter(i => Number.isInteger(i) && i >= 0 && i < ROOM_GRID * ROOM_GRID);
  if (cells.length === 0) return { x: 0, y: 0, width: 1, height: 1 };
  const xs = cells.map(i => i % ROOM_GRID), ys = cells.map(i => Math.floor(i / ROOM_GRID));
  const margin = strokeWidth * scale / 2 + 1;
  const x = Math.max(0, Math.floor(Math.min(...xs) * cell * scale - margin));
  const y = Math.max(0, Math.floor(Math.min(...ys) * cell * scale - margin));
  const right = Math.min(renderSize, Math.ceil((Math.max(...xs) + 1) * cell * scale + margin));
  const bottom = Math.min(renderSize, Math.ceil((Math.max(...ys) + 1) * cell * scale + margin));
  return { x, y, width: Math.max(1, right - x), height: Math.max(1, bottom - y) };
}
