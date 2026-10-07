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
  ActionLogEntry,
  SayEntry,
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
 * 单次应用记录（场景对账输入；R4 起携带事件面——动效第一通道，R6 起
 * visuals 全量替换面）。快照应用 events 恒空（契约快照无事件字段）且
 * visuals 清空（快照=新 tick 状态起点，官方每 tick 全量下发——R6 D2 拍板）；
 * setTerrain 触发的通知同样空（防重放）。kind 区分快照/增量：快照清除瞬态
 * 并即时同步位置，同 id/type 可复用视图。跨快照跳变不得做移动补间，防止
 * 切夹具/重放产生幻影移动。
 */
export interface FeedChange {
  /** 房间对象快照（数组副本，与 FeedView.objects 同款——元素勿深改，评审 B建议7）。 */
  objects: RoomObjectState[];
  time: number;
  events: RoomEvent[];
  kind: 'snapshot' | 'delta';
  actionLogs: ActionLogEntry[];
  says: SayEntry[];
  /** 该房当前全量 visuals（**仓库原引用**——跨增量保持不变，setVisuals 才
   * 换新引用；readonly=消费方不得改动，场景端以引用相等做重建短路）。 */
  visuals: readonly RoomVisual[];
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
  /** 房间当前 visuals（数组副本；readonly=元素勿深改）。 */
  visuals(room: string): readonly RoomVisual[];
  /** 已知房间名（插入序）。 */
  roomNames(): string[];
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

  private notify(room: string, events: RoomEvent[], kind: FeedChange['kind'], actionLogs: ActionLogEntry[] = [], says: SayEntry[] = []): void {
    const state = this.rooms.get(room);
    if (!state) return;
    const change: FeedChange = {
      objects: [...state.objectsById.values()],
      time: state.time,
      events,
      kind,
      actionLogs,
      says,
      // 仓库原引用：跨增量保持不变（场景端引用相等短路，见 FeedChange.visuals 注）
      visuals: state.visuals,
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
      // R6 D2 拍板：快照清空 visuals（快照=新 tick 状态起点，官方每 tick
      // 全量下发；防陈旧残留——R1 登记的保留旧行为就此销账）
      visuals: [],
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
    this.notify(delta.room, delta.events ?? [], 'delta', delta.actionLogs ?? [], delta.says ?? []);
  }

  /** 玩家绘制全量替换（官方 RoomVisual 每 tick 全量下发语义）并通知场景。 */
  setVisuals(room: string, visuals: readonly RoomVisual[]): void {
    this.room(room).visuals = [...visuals];
    this.notify(room, [], 'delta');
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

  /** 房间当前 visuals（数组副本；readonly=元素勿深改）。 */
  visuals(room: string): readonly RoomVisual[] {
    return [...(this.rooms.get(room)?.visuals ?? [])];
  }

  /** 已知房间名（插入序）。 */
  roomNames(): string[] {
    return [...this.rooms.keys()];
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
