/**
 * 房间场景（50×50）：固定层栈（方案 §2.2，零每帧排序）——
 *   terrain(烘焙地形) → ground(road 连通,R3) → objects(R3) →
 *   lighting display(MULTIPLY 输出) → effects(罩/文字/effects 件)
 * 官方几何（对照 terrain.js/_all.metadata.js）：世界坐标系=官方 world 坐标，
 * **格 (x,y) 的中心在 (x*cell, y*cell)、范围 ±cell/2**（对象锚点 x*CELL 与
 * 地形 path M x*100 经 -cell/2 精灵偏移后同相位）；floor 自 -cell/2 起
 * （半格边距带为出口条）。相机数学/拾取共用此相位。
 *
 * R3：objects 舞台（ObjectsStage）挂 ground/objects/effects 三层；墙烘焙的
 * 格集 = 地形码 ∪ constructedWall 对象格（官方 wall path 过滤含
 * constructedWall，terrain.js:264-266）——对象侧变更经 applyObjects 返回值
 * 触发重烘焙（terrain 内部 hash 门控，未变零成本）。
 */
import { Container } from 'pixi.js';
import { LightingPass } from '../core/lighting.js';
import type { ActionLogEntry, SayEntry, RoomEvent, RoomObjectState } from '@kkscreeps/shared';
import type { Viewport } from '../anim/ambient-scheduler.js';
import { DEFAULT_CELL_SIZE, ROOM_SIZE } from './terrain-geometry.js';
import { TERRAIN_SPEC } from '../core/terrain-spec.js';
import { TerrainStage, type TerrainTextures } from './terrain.js';
import { ObjectsStage } from '../objects/objects-stage.js';
import type { JoinAssets } from '../objects/joins-naturals.js';

export { DEFAULT_CELL_SIZE, ROOM_SIZE } from './terrain-geometry.js';

/** 对象舞台装配参数（缺省=不装对象层——纯地形模式）。 */
export interface ObjectsSetup {
  readonly assets: JoinAssets;
  /** 本玩家 id（userOwnerColor 色板 own 判定）。 */
  readonly player: string | null;
  /** tick 时长 ms（运动插值/受击罩/死亡淡出时序；G6 缺省 1000）。 */
  readonly tickDurationMS: number;
}

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
  /** 对象舞台（无装配时为 null——纯地形模式）。 */
  readonly objects: ObjectsStage | null;
  /** 世界尺寸（ROOM_SIZE×cell）。 */
  readonly view: number;
  readonly cellSize: number;

  private baseCodes: ArrayLike<number> | null = null;
  /** 上次墙烘焙的输入引用（syncWalls 双引用门控）。 */
  private walledBaseCodes: ArrayLike<number> | null = null;
  private walledExtras: ReadonlySet<number> | null = null;

  constructor(textures: TerrainTextures, cellSize: number = DEFAULT_CELL_SIZE, objects?: ObjectsSetup) {
    this.cellSize = cellSize;
    this.view = ROOM_SIZE * cellSize;

    this.lighting = new LightingPass(this.view, cellSize);
    this.terrain = new TerrainStage(textures, this.lighting, this.view, cellSize);

    const terrain = this.terrain.root;
    terrain.label = 'layer-terrain';
    const ground = new Container({ label: 'layer-ground' });
    const objectsLayer = new Container({ label: 'layer-objects' });
    const lightingDisplay = new Container({ label: 'layer-lighting' });
    lightingDisplay.addChild(this.lighting.display);
    const effects = new Container({ label: 'layer-effects' });
    this.root.addChild(terrain, ground, objectsLayer, lightingDisplay, effects);
    this.layers = { terrain, ground, objects: objectsLayer, lightingDisplay, effects };

    if (objects) {
      const stage = new ObjectsStage({
        cell: cellSize,
        lighting: this.lighting,
        assets: objects.assets,
        player: objects.player,
        tickDurationMS: objects.tickDurationMS,
        objectsLayer,
      });
      ground.addChild(stage.roadRoot);
      // 顶罩在 effects 内先于对象 effects 件（官方地形预处理器先跑=插入序在前）
      effects.addChild(stage.coverRoot, stage.effectsRoot);
      this.objects = stage;
    } else {
      this.objects = null;
    }
  }

  /** 地形码注入（hash 门控；仅 wall 层实际重建时 lighting 才 dirty——
   * 回放每 tick 的增量不触发离屏重渲，评审 C2/S1）。 */
  setTerrain(codes: ArrayLike<number>): void {
    this.baseCodes = codes;
    this.syncWalls();
  }

  /** 对象列表对账（feed.onChange 驱动）；constructedWall 格集变化时重烘焙墙。
   * events=本 tick 房间事件（R4：攻击/治疗罩）；kind='snapshot' 清瞬态并即时同步。 */
  applyObjects(
    objects: readonly RoomObjectState[],
    time: number,
    events: readonly RoomEvent[] = [],
    kind: 'snapshot' | 'delta' = 'delta',
    input: { actionLogs?: readonly ActionLogEntry[]; says?: readonly SayEntry[] } = {},
  ): void {
    if (!this.objects) return;
    if (this.objects.apply(objects, time, events, kind, input)) this.syncWalls();
  }

  /** 墙烘焙格集 = 地形码 ∪ constructedWall 格。双引用门控（C-S1）：地形码与
   * extras 引用均未变时直接短路（extras 引用由 ObjectsStage integerSetEquals
   * 稳定化）——含 constructedWall 房间的每 tick 增量零拷贝零 FNV。 */
  private syncWalls(): void {
    if (this.baseCodes === null) return;
    const extras = this.objects?.constructedWallCells;
    const extrasRef = extras && extras.size > 0 ? extras : null;
    if (this.baseCodes === this.walledBaseCodes && extrasRef === this.walledExtras) return;
    this.walledBaseCodes = this.baseCodes;
    this.walledExtras = extrasRef;
    const combined = extrasRef ? withExtraWalls(this.baseCodes, extrasRef) : this.baseCodes;
    const { wallRebuilt } = this.terrain.setTerrain(combined);
    if (wallRebuilt) this.lighting.markDirty();
  }

  /** ticker 驱动（elapsedMS=未经 ticker 截断的真实帧时间）：swamp 滚动 + lighting 离屏按需重渲
   * + 对象运动 tween（R4——数据断流动画照跑）。 */
  update(elapsedMS: number, renderer: Parameters<LightingPass['update']>[0], viewport?: Viewport): void {
    if (elapsedMS > 0) this.terrain.update(elapsedMS / TERRAIN_SPEC.frameBaseMs);
    // objects 先于 lighting：运动 tween 的 markDirty 同帧进 RT 重渲
    //（消一帧滞后，评审 C建议4）
    if (elapsedMS > 0) this.objects?.update(elapsedMS, viewport);
    this.lighting.update(renderer, this.terrain.renderSize, this.view);
  }

  destroy(): void {
    this.objects?.destroy();
    this.lighting.destroy();
    this.terrain.destroy();
    this.root.destroy({ children: true });
  }
}

/** 地形码副本 + constructedWall 格置墙位（wall=1 位）。 */
function withExtraWalls(codes: ArrayLike<number>, extras: ReadonlySet<number>): ArrayLike<number> {
  const copy = Array.from(codes);
  for (const idx of extras) {
    if (idx >= 0 && idx < copy.length) copy[idx] = (copy[idx] ?? 0) | 1;
  }
  return copy;
}
