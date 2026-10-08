import { describe, expect, it } from 'vitest';
import { makeOfficialFixture, restoreOfficialFrames } from '../src/perf/official-replay.js';
import { isDelta, isSnapshot } from '@kkscreeps/shared';

describe('official compressed replay adapter', () => {
  it('restores numeric body patches, nested null, deletion and independent frames', () => {
    const source = {
      room: 'E1S1',
      ticks: {
        '10': {
          c: {
            _id: 'c',
            type: 'creep',
            x: 1,
            y: 2,
            body: [
              { type: 'move', hits: 100 },
              { type: 'work', hits: 100 },
            ],
            store: { energy: 5 },
            actionLog: { build: { x: 3, y: 4 } },
          },
        },
        '11': {
          c: { body: { '1': { hits: 0 } }, store: { energy: 0 }, actionLog: { build: null } },
        },
        '12': { c: null },
      },
    };
    const original = structuredClone(source);
    const raw = restoreOfficialFrames(source);
    expect(raw[1]!.objects[0]!.body).toEqual({
      '0': { type: 'move', hits: 100 },
      '1': { type: 'work', hits: 0 },
    });
    expect(source).toEqual(original);
    expect(raw[0]!.objects[0]!.body).toEqual(original.ticks['10'].c.body);
    const fixture = makeOfficialFixture(source, [
      { x: 0, y: 0, type: 'wall' },
      { x: 1, y: 0, type: 'swamp' },
    ]);
    expect(isSnapshot(fixture.ticks[0])).toBe(true);
    expect(fixture.ticks.slice(1).every(isDelta)).toBe(true);
    expect(fixture.ticks[0]!.terrain.codes.slice(0, 3)).toEqual([1, 2, 0]);
    const delta = fixture.ticks[1]!;
    if (!isDelta(delta)) throw new Error('Expected delta');
    expect(delta.upsert[0]!.body).toEqual([
      { type: 'move', hits: 100 },
      { type: 'work', hits: 0 },
    ]);
    expect(delta.actionLogs).toEqual([{ id: 'c', actionLog: { build: null } }]);
    const deleted = fixture.ticks[2]!;
    if (!isDelta(deleted)) throw new Error('Expected delta');
    expect(deleted.removedIds).toEqual(['c']);
  });
  it('preserves omitted action logs and structure/resource envelopes with absolute deadlines', () => {
    const source = {
      room: 'A',
      ticks: {
        '100': {
          s: {
            _id: 's',
            type: 'tower',
            x: 4,
            y: 5,
            cooldownTime: 150,
            actionLog: { repair: { x: 1, y: 1 } },
          },
          e: { _id: 'e', type: 'energy', energy: 7 },
        },
        '101': { s: { x: 6 } },
      },
    };
    const f = makeOfficialFixture(source, []);
    expect(f.ticks[0]!.objects[0]).toMatchObject({
      id: 's',
      type: 'structure',
      structureType: 'tower',
      cooldownTime: 150,
    });
    expect(f.ticks[0]!.objects[1]).toMatchObject({
      id: 'e',
      type: 'resource',
      resourceType: 'energy',
      amount: 7,
    });
    const d = f.ticks[1]!;
    if (!isDelta(d)) throw new Error('Expected delta');
    expect(d.actionLogs![0]).toEqual({ id: 's', actionLog: { repair: { x: 1, y: 1 } } });
  });
});
