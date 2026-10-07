import { describe, expect, it } from 'vitest';
import type { FeedChange } from '../src/feed/room-feed.js';
import { ActiveRoomController } from '../src/feed/active-room.js';

// 活动房间编排（R6 D3）：过滤/自动激活/切房重建/离房零触达。

function change(kind: FeedChange['kind'], time: number): FeedChange {
  return { objects: [], time, events: [], kind, actionLogs: [], says: [], visuals: [] };
}

function setup(known: string[] = ['A', 'B']) {
  const applied: string[] = [];
  const rebuilt: string[] = [];
  const host = {
    apply: (room: string) => applied.push(room),
    rebuild: (room: string) => rebuilt.push(room),
  };
  const ctrl = new ActiveRoomController(host, (name) => known.includes(name));
  return { ctrl, applied, rebuilt };
}

describe('ActiveRoomController', () => {
  it('首个快照自动激活一次，此后非活动房变更零触达（离房零写入）', () => {
    const { ctrl, applied } = setup();
    expect(ctrl.activeRoom).toBeNull();
    ctrl.onChange('A', change('snapshot', 0));
    expect(ctrl.activeRoom).toBe('A');
    expect(applied).toEqual(['A']);
    ctrl.onChange('B', change('snapshot', 0));
    ctrl.onChange('B', change('delta', 1));
    expect(applied).toEqual(['A']); // B 的快照与增量都被过滤
  });

  it('首个快照前的非快照变更不触发自动激活', () => {
    const { ctrl, applied } = setup();
    ctrl.onChange('A', change('delta', 1));
    expect(ctrl.activeRoom).toBeNull();
    expect(applied).toEqual([]);
  });

  it('setActive 未知房抛错；同房幂等；切房走 rebuild 不走 apply', () => {
    const { ctrl, applied, rebuilt } = setup(['A', 'B']);
    ctrl.onChange('A', change('snapshot', 0));
    expect(() => ctrl.setActive('Z')).toThrow('未知房间');
    ctrl.setActive('A');
    expect(rebuilt).toEqual([]); // 同房幂等
    ctrl.setActive('B');
    expect(rebuilt).toEqual(['B']);
    expect(ctrl.activeRoom).toBe('B');
    ctrl.onChange('A', change('delta', 5));
    expect(applied).toEqual(['A']); // 此前 A 激活期的记录；切走后 A 不再触达
  });

  it('切换后原活动房不再触达，新活动房增量照常', () => {
    const { ctrl, applied } = setup(['A', 'B']);
    ctrl.onChange('A', change('snapshot', 0));
    ctrl.setActive('B');
    ctrl.onChange('B', change('delta', 1));
    ctrl.onChange('A', change('delta', 2));
    expect(applied).toEqual(['A', 'B']);
  });
});
