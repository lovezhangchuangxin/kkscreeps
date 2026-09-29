/**
 * 房间场景（50×50）。R0 只有底色房；固定层栈（terrain→ground→objects→
 * lighting→effects，方案 §2.2）与地形烘焙在 R2 落位——root 预留为层栈地基。
 *
 * 世界坐标约定（取证官方 demo worldConfigs.js：CELL_SIZE=100）：
 *   格 (cx,cy) 占世界 [cx*CELL,(cx+1)*CELL)，格心 +CELL/2；
 *   房间世界范围 [0, CELL*50]。官方 stage.pivot=-CELL_SIZE/2 的半格补偿
 *   在 R2 对照官方截图时同构复刻（〔官拆〕§3）。
 */
import { Container, Graphics } from 'pixi.js';

/** 官方 plain 底色（〔官拆〕§6，terrain.js）。 */
export const PLAIN_COLOR = 0x555555;

export const ROOM_SIZE = 50;
export const DEFAULT_CELL_SIZE = 100;

export class RoomScene {
  readonly root: Container = new Container({ label: 'room' });
  readonly cellSize: number;

  constructor(cellSize: number = DEFAULT_CELL_SIZE) {
    this.cellSize = cellSize;
    const extent = ROOM_SIZE * cellSize;
    const base = new Graphics().rect(0, 0, extent, extent).fill(PLAIN_COLOR);
    base.label = 'room-base';
    this.root.addChild(base);
  }
}
