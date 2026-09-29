/**
 * 房间场景（50×50）：固定层栈（方案 §2.2，零每帧排序）——
 *   terrain(烘焙地形) → ground(路连通,R3) → objects(R3) →
 *   lighting display(MULTIPLY 输出) → effects(R3+ 罩/文字)
 * 官方几何（对照 terrain.js/_all.metadata.js）：世界坐标系=官方 world 坐标，
 * **格 (x,y) 的中心在 (x*cell, y*cell)、范围 ±cell/2**（对象锚点 x*CELL 与
 * 地形 path M x*100 经 -cell/2 精灵偏移后同相位）；floor 自 -cell/2 起
 * （半格边距带为出口条）。相机数学/拾取共用此相位。
 */
import { Container } from 'pixi.js';
import { LightingPass } from '../core/lighting.js';
import { DEFAULT_CELL_SIZE, ROOM_SIZE } from './terrain-geometry.js';
import { TerrainStage, type TerrainTextures } from './terrain.js';

export { DEFAULT_CELL_SIZE, ROOM_SIZE } from './terrain-geometry.js';

export class RoomScene {
  readonly root: Container = new Container({ label: 'room' });
  /** 固定层槽位（子容器按序 addChild；静态层栈不排序）。 */
  readonly layers: {
    terrain: Container;
    ground: Container;
    objects: Container;
    lightingDisplay: Container;
    effects: Container;
  };
  readonly lighting: LightingPass;
  readonly terrain: TerrainStage;
  /** 世界尺寸（ROOM_SIZE×cell）。 */
  readonly view: number;
  readonly cellSize: number;

  constructor(textures: TerrainTextures, cellSize: number = DEFAULT_CELL_SIZE) {
    this.cellSize = cellSize;
    this.view = ROOM_SIZE * cellSize;

    this.lighting = new LightingPass(this.view, cellSize);
    this.terrain = new TerrainStage(textures, this.lighting, this.view, cellSize);

    const terrain = this.terrain.root;
    terrain.label = 'layer-terrain';
    const ground = new Container({ label: 'layer-ground' });
    const objects = new Container({ label: 'layer-objects' });
    const lightingDisplay = new Container({ label: 'layer-lighting' });
    lightingDisplay.addChild(this.lighting.display);
    const effects = new Container({ label: 'layer-effects' });
    this.root.addChild(terrain, ground, objects, lightingDisplay, effects);
    this.layers = { terrain, ground, objects, lightingDisplay, effects };
  }

  /** 地形码注入（hash 门控；仅 wall 层实际重建时 lighting 才 dirty——
   * 回放每 tick 的增量不触发离屏重渲，评审 C2/S1）。 */
  setTerrain(codes: ArrayLike<number>): void {
    const { wallRebuilt } = this.terrain.setTerrain(codes);
    if (wallRebuilt) this.lighting.markDirty();
  }

  /** ticker 驱动：swamp 滚动 + lighting 离屏按需重渲。 */
  update(framesDelta: number, renderer: Parameters<LightingPass['update']>[0]): void {
    this.terrain.update(framesDelta);
    this.lighting.update(renderer, this.terrain.renderSize, this.view);
  }

  destroy(): void {
    this.lighting.destroy();
    this.terrain.destroy();
    this.root.destroy({ children: true });
  }
}
