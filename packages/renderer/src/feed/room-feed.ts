/**
 * RoomFeed 契约的渲染器侧实现：房间状态仓库（id 索引）。
 * R1 只承载状态机（快照置换/增量对账/地形与视觉存储）；场景图消费
 * （snapshot 对账→场景增量）随 R3 描述符框架落位。
 *
 * 失败语义（fail-fast，v0 唯一驱动者是回放 harness）：applyDelta 命中未
 * 初始化房间、或时间回退，直接抛错——回放器 bug 不静默吞掉。
 */
import type {
  RoomDelta,
  RoomEvent,
  RoomFeed,
  RoomObjectState,
  RoomSnapshot,
  RoomVisual,
  TerrainData,
} from '@kkscreeps/shared';

interface RoomState {
  time: number;
  terrain?: TerrainData;
  objectsById: Map<string, RoomObjectState>;
  visuals: RoomVisual[];
}

/** 房间状态只读摘要（HUD/未来场景层消费）。 */
export interface RoomSummary {
  room: string;
  time: number;
  objects: number;
  hasTerrain: boolean;
  visuals: number;
}

/**
 * 单次应用记录（场景对账输入；R4 起携带事件面——动效第一通道）。快照应用
 * events 恒空（契约快照无事件字段）；setTerrain 触发的通知同样空（防重放）。
 * kind 区分快照/增量：快照=全量置换语义，场景侧应重建视图（跨快照的位置
 * 跳变不得做移动补间——切夹具/重放的幻影移动根因）。
 */
export interface FeedChange {
  /** 房间对象快照（数组副本，与 FeedView.objects 同款——元素勿深改，评审 B建议7）。 */
  objects: RoomObjectState[];
  time: number;
  events: RoomEvent[];
  kind: 'snapshot' | 'delta';
}

/**
 * feed 公共面：RoomFeed 契约 + 只读摘要（评审 B 建议 2——消费方对接口编程，
 * FeedStore 是实现细节）。普通数据类，无 Pixi 依赖（ADR-009）。
 */
export interface FeedView extends RoomFeed {
  /** 房间摘要；房间不存在返回 undefined。 */
  summary(room: string): RoomSummary | undefined;
  /** 房间内对象快照（数组副本；R3 场景对账的读取面，元素勿深改）。 */
  objects(room: string): RoomObjectState[];
  /** 房间地形；未设置返回 undefined。 */
  terrain(room: string): TerrainData | undefined;
}

export class FeedStore implements FeedView {
  private readonly rooms = new Map<string, RoomState>();
  private readonly listeners = new Set<(room: string, change: FeedChange) => void>();

  /** 状态变更订阅（快照/增量/地形注入后触发；返回退订函数）。
   * 渲染器内部接线用（feed→scene），非契约面。 */
  onChange(listener: (room: string, change: FeedChange) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify(room: string, events: RoomEvent[], kind: FeedChange['kind']): void {
    const state = this.rooms.get(room);
    if (!state) return;
    const change: FeedChange = {
      objects: [...state.objectsById.values()],
      time: state.time,
      events,
      kind,
    };
    for (const listener of this.listeners) listener(room, change);
  }

  setTerrain(room: string, terrain: TerrainData): void {
    this.room(room).terrain = terrain;
    // kind='delta'：地形注入非对象变更（复用增量语义——场景侧 valuesEqual
    // 门控为 no-op；不引入第三 kind，评审 B建议4）
    this.notify(room, [], 'delta');
  }

  applySnapshot(snapshot: RoomSnapshot): void {
    this.rooms.set(snapshot.room, {
      time: snapshot.time,
      terrain: snapshot.terrain,
      objectsById: new Map(snapshot.objects.map((o) => [o.id, o])),
      visuals: this.rooms.get(snapshot.room)?.visuals ?? [],
    });
    this.notify(snapshot.room, [], 'snapshot');
  }

  applyDelta(delta: RoomDelta): void {
    const state = this.rooms.get(delta.room);
    if (!state) throw new Error(`applyDelta 于未初始化房间 ${delta.room}`);
    if (delta.time <= state.time) {
      throw new Error(`applyDelta 时间回退：${delta.time} <= ${state.time}（房间 ${delta.room}）`);
    }
    state.time = delta.time;
    for (const o of delta.upsert) state.objectsById.set(o.id, o);
    for (const id of delta.removedIds) state.objectsById.delete(id);
    this.notify(delta.room, delta.events ?? [], 'delta');
  }

  setVisuals(room: string, visuals: RoomVisual[]): void {
    this.room(room).visuals = [...visuals];
  }

  /** 房间摘要；房间不存在返回 undefined。 */
  summary(room: string): RoomSummary | undefined {
    const state = this.rooms.get(room);
    if (!state) return undefined;
    return {
      room,
      time: state.time,
      objects: state.objectsById.size,
      hasTerrain: state.terrain !== undefined,
      visuals: state.visuals.length,
    };
  }

  /** 房间内对象快照（数组副本；R3 场景对账的读取面）。 */
  objects(room: string): RoomObjectState[] {
    return [...(this.rooms.get(room)?.objectsById.values() ?? [])];
  }

  /** 房间地形；未设置返回 undefined。 */
  terrain(room: string): TerrainData | undefined {
    return this.rooms.get(room)?.terrain;
  }

  private room(name: string): RoomState {
    let state = this.rooms.get(name);
    if (!state) {
      state = { time: 0, objectsById: new Map(), visuals: [] };
      this.rooms.set(name, state);
    }
    return state;
  }
}
