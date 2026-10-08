import { describe, it, expect } from 'vitest';
import { isRoomVisual } from '@kkscreeps/shared';
import full from '../fixtures/perf-full-room.json';
import showcase from '../fixtures/showcase.json';
import { makeFullRoom, fullDelta, fullVisuals } from '../src/perf/fixture.js';

describe('R7 full-room measurement input', () => {
  it('reproduces the committed synthetic input and its frozen dimensions', () => {
    const f = makeFullRoom(showcase.ticks[0]!.objects!);
    expect(f).toEqual(full);
    expect(f.ticks[0]!.objects).toHaveLength(300);
    expect(f.ticks[0]!.objects.filter((o) => o.type === 'creep')).toHaveLength(50);
    expect(
      f.ticks[0]!.objects.filter((o) => o.type === 'creep').every(
        (o) => (o.body as unknown[]).length === 50,
      ),
    ).toBe(true);
    expect(new Set(f.ticks[0]!.objects.map((o) => o.id)).size).toBe(300);
    expect([...new Set(f.ticks[0]!.terrain.codes)].sort()).toEqual([0, 1, 2, 4]);
    expect(f.visuals).toHaveLength(100);
    expect(f.visuals.every(isRoomVisual)).toBe(true);
    expect(f.visuals.filter((v) => v.type === 'text')).toHaveLength(20);
  });
  it('moves all 50, restores transient removals, and bounds every 100-tick cycle', () => {
    const snapshot = makeFullRoom(showcase.ticks[0]!.objects!).ticks[0]!;
    const states = new Map(snapshot.objects.map((o) => [o.id, o]));
    for (let t = 1; t <= 300; t++) {
      const d = fullDelta(snapshot, t, true, true);
      d.upsert.forEach((o) => states.set(o.id, o));
      d.removedIds.forEach((id) => states.delete(id));
      expect(states.size).toBe(t % 100 >= 50 && t % 100 < 55 ? 295 : 300);
      expect(d.actionLogs).toHaveLength(d.upsert.length);
      expect(d.events).toHaveLength(d.upsert.length);
    }
    expect([...states.keys()].sort()).toEqual(snapshot.objects.map((o) => o.id).sort());
    expect(fullVisuals(100)).toEqual(fullVisuals(0));
    expect(
      fullDelta(snapshot, 1).upsert.every(
        (o) => o.x === snapshot.objects.find((b) => b.id === o.id)!.x! + 1,
      ),
    ).toBe(true);
  });
});
