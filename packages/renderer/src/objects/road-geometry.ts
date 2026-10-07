/**
 * road 连通几何（官方 processors/road.js 移植的纯逻辑半件）。
 *
 * 官方每路一格画一个圆心圆 + 朝四个方向的连接四边形（road.js:98-141）：
 *   mm=(x-1,y-1) 左上对角 / zm=(x,y-1) 正上 / pm=(x+1,y-1) 右上对角 /
 *   mz=(x-1,y) 正左——四向覆盖全部 8 邻接（每对相邻路由其中一方画出连接）。
 * 判定集只含 road 自身（fillRoads 过滤 type==='road'）。
 * 零 pixi 导入——几何清单由 pixi 侧转 Graphics。
 */
import type { RoomObjectState } from '@kkscreeps/shared';

/** 单格连通状态（键名沿官方 knownNodes：mm/zm/pm/mz）。 */
export interface RoadNode {
  readonly x: number;
  readonly y: number;
  readonly mm: boolean;
  readonly zm: boolean;
  readonly pm: boolean;
  readonly mz: boolean;
}

/** 由对象列表提取全部 road 格的连通状态（位置稳定排序，便于快照迭代）。 */
export function roadNodes(objects: readonly RoomObjectState[]): RoadNode[] {
  const cells = new Set<string>();
  for (const o of objects) {
    if (o.type === 'structure' && o.structureType === 'road') {
      cells.add(`${num(o.x)},${num(o.y)}`);
    }
  }
  const nodes: RoadNode[] = [];
  for (const key of cells) {
    const [xs, ys] = key.split(',');
    const x = Number(xs);
    const y = Number(ys);
    nodes.push({
      x,
      y,
      mm: cells.has(`${x - 1},${y - 1}`),
      zm: cells.has(`${x},${y - 1}`),
      pm: cells.has(`${x + 1},${y - 1}`),
      mz: cells.has(`${x - 1},${y}`),
    });
  }
  return nodes;
}

function num(v: unknown): number {
  return typeof v === 'number' ? v : 0;
}
