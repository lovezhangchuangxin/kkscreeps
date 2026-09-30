import { describe, expect, it } from 'vitest';
import { FeedStore } from '../src/feed/room-feed.js';
import type { RoomDelta, RoomSnapshot } from '@kkscreeps/shared';

// feed 状态机：快照置换 / 增量对账 / fail-fast 语义 / 读取面。

const snapshot: RoomSnapshot = {
  time: 0,
  room: 'W1N1',
  terrain: { codes: [1, 2, 0] },
  objects: [
    { id: 'a', type: 'creep', x: 1, y: 1 },
    { id: 'b', type: 'structure', structureType: 'spawn', x: 2, y: 2 },
  ],
  energyAvailable: 300,
  energyCapacityAvailable: 300,
};

function delta(time: number, upsert: RoomSnapshot['objects'], removedIds: string[]): RoomDelta {
  return { time, room: 'W1N1', upsert, removedIds };
}

describe('FeedStore', () => {
  it('快照置换整个房间状态', () => {
    const feed = new FeedStore();
    feed.applySnapshot(snapshot);
    expect(feed.summary('W1N1')).toEqual({
      room: 'W1N1',
      time: 0,
      objects: 2,
      hasTerrain: true,
      visuals: 0,
    });
    feed.applySnapshot({ ...snapshot, time: 5, objects: [{ id: 'c', type: 'creep' }] });
    expect(feed.summary('W1N1')!.objects).toBe(1);
    expect(feed.objects('W1N1').map((o) => o.id)).toEqual(['c']);
  });

  it('增量 upsert/removed 对账，时间推进', () => {
    const feed = new FeedStore();
    feed.applySnapshot(snapshot);
    feed.applyDelta(delta(1, [{ id: 'a', type: 'creep', x: 2, y: 1 }], ['b']));
    // 同 id 置换 + 移除 b → 只剩新 a
    expect(feed.summary('W1N1')).toMatchObject({ time: 1, objects: 1 });
    expect(feed.objects('W1N1').find((o) => o.id === 'a')!.x).toBe(2);
    expect(feed.objects('W1N1').some((o) => o.id === 'b')).toBe(false);
  });

  it('fail-fast：未初始化房间与时间回退都抛错', () => {
    const feed = new FeedStore();
    expect(() => feed.applyDelta(delta(1, [], []))).toThrow('未初始化');
    feed.applySnapshot(snapshot);
    expect(() => feed.applyDelta(delta(0, [], []))).toThrow('时间回退');
    expect(() => feed.applyDelta(delta(1, [], []))).not.toThrow();
  });

  it('setTerrain/setVisuals 可先于快照（自动建房），快照保留 visuals', () => {
    const feed = new FeedStore();
    feed.setTerrain('W1N1', { codes: [1] });
    expect(feed.terrain('W1N1')).toEqual({ codes: [1] });
    feed.setVisuals('W1N1', [{ type: 'circle', x: 1, y: 1 }]);
    feed.applySnapshot(snapshot);
    expect(feed.summary('W1N1')!.visuals).toBe(1);
    expect(feed.terrain('W1N1')).toEqual({ codes: [1, 2, 0] }); // 快照地形覆盖
  });

  it('summary 对未知房间返回 undefined', () => {
    const feed = new FeedStore();
    expect(feed.summary('E9S9')).toBeUndefined();
    expect(feed.objects('E9S9')).toEqual([]);
    expect(feed.terrain('E9S9')).toBeUndefined();
  });

  it('onChange 携带变更记录：快照/地形空事件，增量透传 events（R4 罩通道）', () => {
    const feed = new FeedStore();
    const seen: Array<{ room: string; time: number; objects: number; events: number; kind: string }> = [];
    feed.onChange((room, change) => {
      seen.push({ room, time: change.time, objects: change.objects.length, events: change.events.length, kind: change.kind });
    });
    feed.setTerrain('W1N1', { codes: [1] });
    feed.applySnapshot(snapshot);
    feed.applyDelta({
      time: 1,
      room: 'W1N1',
      upsert: [],
      removedIds: [],
      events: [{ event: 1, objectId: 'atk', data: { targetId: 'a', damage: 30 } }],
    });
    expect(seen).toEqual([
      { room: 'W1N1', time: 0, objects: 0, events: 0, kind: 'delta' },
      { room: 'W1N1', time: 0, objects: 2, events: 0, kind: 'snapshot' },
      { room: 'W1N1', time: 1, objects: 2, events: 1, kind: 'delta' },
    ]);
  });
});
