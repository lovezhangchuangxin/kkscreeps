/**
 * 拾取命中解析（R6 D4，纯逻辑零 pixi——数据面命中，ADR-009 不遍历 Pixi 树）。
 *
 * 命中语义（契约，pickAt/onPick/onHover TSDoc 同源）：
 *   - 格坐标量化：对象按其**数据格**命中（移动中 creep 按目标格，非插值
 *     渲染位）；
 *   - 同格多对象命中优先级 = objects 层 z 序高者（Z_TABLE 全表，官方 zIndex
 *     语义同源），平手取 id 字典序（JS `<` 即 UTF-16 码元序；`kk%016x` ASCII 域内与码点序一致）。
 */
import type { RoomObjectState } from '@kkscreeps/shared';
import { resolveTypeKey, Z_TABLE } from '../objects/z-table.js';

export function resolvePickObject(cell: { x: number; y: number }, objects: readonly RoomObjectState[]): string | undefined {
  let bestId: string | undefined;
  let bestZ = -Infinity;
  for (const o of objects) {
    if (o.x !== cell.x || o.y !== cell.y) continue;
    const z = Z_TABLE[resolveTypeKey(o)] ?? 0;
    if (z > bestZ || (z === bestZ && bestId !== undefined && o.id < bestId)) {
      bestId = o.id;
      bestZ = z;
    }
  }
  return bestId;
}
