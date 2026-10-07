import { describe, expect, it } from 'vitest';
import type { RoomObjectState } from '@kkscreeps/shared';
import { convertWorldJsonTicks, type WorldJson, type WorldJsonTick } from '../src/feed/differ.js';

// differ 对账：world_json tick 序列 → {snapshot, deltas}（方案 §3 夹具形）。

let idSeq = 0;
function obj(room: string, x: number, extra: Record<string, unknown> = {}): RoomObjectState {
  idSeq += 1;
  return { id: `kk${String(idSeq).padStart(16, '0')}`, type: 'creep', room, x, y: 10, ...extra };
}

function world(
  time: number,
  room: string,
  objects: RoomObjectState[],
  events?: unknown[],
): WorldJson {
  return {
    time,
    rooms: {
      [room]: {
        name: room,
        terrain: new Array<number>(2500).fill(0),
        energyAvailable: 300,
        energyCapacityAvailable: 300,
        ...(events && events.length > 0 ? { events } : {}),
      },
    },
    objects,
  };
}

const tick = (n: number, w: WorldJson): WorldJsonTick => ({ tick: n, world: w });
const ROOM = 'W1N1';

describe('convertWorldJsonTicks', () => {
  it('首条转快照（terrain/能量面直通），后续按 id 对账转增量', () => {
    const a = obj(ROOM, 20);
    const b = obj(ROOM, 22);
    const ticks = [tick(0, world(0, ROOM, [a, b])), tick(1, world(1, ROOM, [a, b])), tick(2, world(2, ROOM, [b]))];
    const f = convertWorldJsonTicks(ticks);
    expect(f.room).toBe(ROOM);
    expect(f.ticks).toHaveLength(3);
    const [snapshot, ...deltas] = f.ticks;
    expect(snapshot!.time).toBe(0);
    expect(snapshot!.objects).toHaveLength(2);
    expect(snapshot!.terrain.codes).toHaveLength(2500);
    expect(snapshot!.energyAvailable).toBe(300);
    expect(deltas[0]).toMatchObject({ time: 1, upsert: [], removedIds: [] });
    expect(deltas[1]).toMatchObject({ time: 2, upsert: [], removedIds: [a.id] });
  });

  it('字段变化进 upsert、新对象进 upsert', () => {
    const a = obj(ROOM, 20, { name: 'a' });
    const aMoved = { ...a, x: 21 };
    const c = obj(ROOM, 30, { name: 'c' });
    const ticks = [tick(0, world(0, ROOM, [a])), tick(1, world(1, ROOM, [aMoved, c]))];
    const [, d1] = convertWorldJsonTicks(ticks).ticks;
    expect(d1!.upsert.map((o) => o.id).sort()).toEqual([a.id, c.id].sort());
  });

  it('事件从该 tick 的 rooms[room].events 透传（空则缺省键）', () => {
    const a = obj(ROOM, 20);
    const ev = { event: 1, objectId: a.id, data: { x: 20, y: 10 } };
    const ticks = [tick(0, world(0, ROOM, [a])), tick(1, world(1, ROOM, [a], [ev])), tick(2, world(2, ROOM, [a]))];
    const [, d1, d2] = convertWorldJsonTicks(ticks).ticks;
    expect(d1!.events).toEqual([ev]);
    expect('events' in d2!).toBe(false);
  });
  it('他房对象不入账（objects 按房间过滤）', () => {
    const a = obj(ROOM, 20);
    const foreign = obj('E9S9', 5);
    const ticks = [tick(0, world(0, ROOM, [a, foreign]))];
    const f = convertWorldJsonTicks(ticks);
    expect(f.ticks[0]!.objects.map((o) => o.id)).toEqual([a.id]);
  });

  it('多房世界与空序列直接抛错（v0 单房纪律）', () => {
    const twoRooms: WorldJson = {
      time: 0,
      rooms: {
        W1N1: { name: 'W1N1', terrain: [], },
        E9S9: { name: 'E9S9', terrain: [], },
      },
      objects: [],
    };
    expect(() => convertWorldJsonTicks([tick(0, twoRooms)])).toThrow('单房');
    expect(() => convertWorldJsonTicks([])).toThrow('空 tick');
  });
});
