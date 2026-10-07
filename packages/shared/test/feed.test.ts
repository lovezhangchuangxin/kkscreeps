import { describe, expect, it } from 'vitest';
import {
  isDelta,
  isRoomEvent,
  isSnapshot,
  type FixtureFile,
  type RoomDelta,
  type RoomSnapshot,
} from '../src/feed.js';

// 契约守卫的结构测试：夹具/回放/未来网络面的入口校验面。

const snapshot: RoomSnapshot = {
  time: 0,
  room: 'W1N1',
  terrain: { codes: new Array<number>(2500).fill(0) },
  objects: [
    {
      id: 'kk0000000000000001',
      type: 'creep',
      room: 'W1N1',
      x: 25,
      y: 26,
      name: 'worker',
      owner: 'p1',
    },
    {
      id: 'kk0000000000000002',
      type: 'structure',
      structureType: 'source',
      room: 'W1N1',
      x: 20,
      y: 20,
    },
  ],
  energyAvailable: 300,
  energyCapacityAvailable: 300,
};

const delta: RoomDelta = {
  time: 1,
  room: 'W1N1',
  upsert: [],
  removedIds: [],
  events: [{ event: 1, objectId: 'kk0000000000000001', data: { x: 25, y: 25 } }],
};

describe('契约守卫', () => {
  it('快照/增量互判（快照有 terrain.codes，增量有 upsert/removedIds）', () => {
    expect(isSnapshot(snapshot)).toBe(true);
    expect(isDelta(snapshot)).toBe(false);
    expect(isSnapshot(delta)).toBe(false);
    expect(isDelta(delta)).toBe(true);
  });

  it('事件守卫：event 数值 + objectId 字符串', () => {
    expect(isRoomEvent(delta.events![0])).toBe(true);
    expect(isRoomEvent({ event: 'attack' })).toBe(false);
    expect(isRoomEvent(null)).toBe(false);
  });

  it('夹具形：首元素快照、其余增量', () => {
    const fixture: FixtureFile = { room: 'W1N1', ticks: [snapshot, delta] };
    expect(isSnapshot(fixture.ticks[0])).toBe(true);
    expect(fixture.ticks.slice(1).every((t) => isDelta(t))).toBe(true);
  });

  it('缺 terrain 或 upsert 都不通过对应守卫', () => {
    expect(isSnapshot({ time: 0, room: 'W1N1' })).toBe(false);
    expect(isDelta({ time: 0, room: 'W1N1' })).toBe(false);
  });
});
