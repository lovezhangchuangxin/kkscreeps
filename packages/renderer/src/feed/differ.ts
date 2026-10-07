/**
 * 夹具 differ：引擎 world_json 逐 tick 序列 → RoomFeed 契约的
 * {snapshot, deltas[]} 夹具形（F2 方案 §3/§4）。
 *
 * 纯函数、零运行时依赖（契约类型为 type-only import）——build-fixtures.mjs
 * 以 node 原生 TS 直跑、Vitest 单测、未来 M3 投影同形替换三方共用。
 * 对账口径：按对象 `id` 增删、按 JSON 序列化值判变化（夹具工具的确定性
 * 简化；渲染运行时的字段级脏标记在描述符层，与此互不替代）。
 */
import type {
  FixtureFile,
  RoomDelta,
  RoomEvent,
  RoomObjectState,
  RoomSnapshot,
} from '@kkscreeps/shared';

/** 引擎 world_json 投影的读取视图（只取 differ 所需字段；宽松索引签名）。 */
export interface WorldJson {
  time: number;
  rooms: Record<
    string,
    {
      name: string;
      terrain: number[];
      energyAvailable?: number;
      energyCapacityAvailable?: number;
      events?: unknown[];
    }
  >;
  objects: Array<RoomObjectState & { room?: string }>;
}

/** 导出器 JSONL 的一行（`kkscreeps export-fixtures` 子命令产物）。 */
export interface WorldJsonTick {
  tick: number;
  world: WorldJson;
}

const stableKey = (o: unknown): string => JSON.stringify(o);

/**
 * world_json tick 序列 → 夹具。首条转快照，其余按 id 对账转增量；
 * 事件取自该 tick 结算产出的 `rooms[room].events`（空则缺省）。
 * 单房断言：v0 夹具均为单房世界（多房切房=多夹具，方案 §3）。
 */
export function convertWorldJsonTicks(ticks: readonly WorldJsonTick[]): FixtureFile {
  if (ticks.length === 0) throw new Error('空 tick 序列');
  const first = ticks[0]!;
  const roomNames = Object.keys(first.world.rooms);
  if (roomNames.length !== 1) {
    throw new Error(`v0 夹具应为单房世界，实际 ${roomNames.length} 房`);
  }
  const room = roomNames[0]!;
  const roomOf = (world: WorldJson) => {
    const state = world.rooms[room];
    if (!state) throw new Error(`tick ${world.time} 缺房间 ${room}`);
    return state;
  };
  const objectsInRoom = (world: WorldJson): RoomObjectState[] =>
    world.objects.filter((o) => o.room === room);

  const firstState = roomOf(first.world);
  const snapshot: RoomSnapshot = {
    time: first.world.time,
    room,
    terrain: { codes: firstState.terrain },
    objects: objectsInRoom(first.world),
    ...(firstState.energyAvailable !== undefined ? { energyAvailable: firstState.energyAvailable } : {}),
    ...(firstState.energyCapacityAvailable !== undefined
      ? { energyCapacityAvailable: firstState.energyCapacityAvailable }
      : {}),
  };

  const deltas: RoomDelta[] = [];
  let prevByKey = new Map<string, string>(
    objectsInRoom(first.world).map((o) => [o.id, stableKey(o)]),
  );
  for (const entry of ticks.slice(1)) {
    const state = roomOf(entry.world);
    const objects = objectsInRoom(entry.world);
    const upsert: RoomObjectState[] = [];
    const nowByKey = new Map<string, string>();
    for (const o of objects) {
      const key = stableKey(o);
      nowByKey.set(o.id, key);
      if (prevByKey.get(o.id) !== key) upsert.push(o);
    }
    const removedIds = [...prevByKey.keys()].filter((id) => !nowByKey.has(id));
    const events = (state.events ?? []).filter((e): e is RoomEvent => isEventShape(e));
    deltas.push({
      time: entry.world.time,
      room,
      upsert,
      removedIds,
      ...(events.length > 0 ? { events } : {}),
    });
    prevByKey = nowByKey;
  }

  return { room, ticks: [snapshot, ...deltas] };
}

/** 事件形状校验（与 shared isRoomEvent 同口径；differ 保持零运行时依赖）。 */
function isEventShape(value: unknown): value is RoomEvent {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as RoomEvent).event === 'number' &&
    typeof (value as RoomEvent).objectId === 'string'
  );
}
