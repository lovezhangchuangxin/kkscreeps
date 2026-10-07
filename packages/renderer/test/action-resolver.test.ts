import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import * as constants from '../../shared/src/events.js';
import { resolveActions } from '../src/anim/action-resolver.js';
import { POWER_ICONS } from '../src/anim/effect-specs.js';
import type { RoomEvent, RoomObjectState } from '@kkscreeps/shared';

const source: RoomObjectState = { id: 'a', type: 'creep', x: 10, y: 10 };
const target: RoomObjectState = { id: 'b', type: 'creep', x: 12, y: 11 };
const ctl: RoomObjectState = { id: 'ctl', type: 'structure', structureType: 'controller', x: 30, y: 30 };
function run(events: RoomEvent[], objects = [source, target]) {
  const current = new Map(objects.map(s => [s.id, s]));
  return resolveActions({ events, current, previous: current });
}
describe('action input semantics', () => {
  it('used TS constants agree with Rust, including subtypes and power icon IDs', () => {
    const rust = readFileSync(new URL('../../../crates/kk-core/src/constants.rs', import.meta.url), 'utf8');
    for (const [name, value] of Object.entries(constants)) {
      expect(rust, name).toMatch(new RegExp(`pub const ${name}: i64 = ${value}i64;`));
    }
    for (const [id, name] of Object.entries(POWER_ICONS)) {
      expect(rust).toContain(`pub const PWR_${name.toUpperCase().replaceAll('-', '_')}: i64 = ${id}i64;`);
    }
  });
  it('zero amount still animates; subtype picks ranged vs melee', () => {
    const r = run([{ event: 6, objectId: 'a', data: { targetId: 'b', amount: 0, healType: 2 } },
      { event: 1, objectId: 'a', data: { targetId: 'b', damage: 0, attackType: 1 } }]);
    expect(r.commands.map(c => c.action)).toEqual(['rangedHeal', 'attack']);
    expect(r.hits).toEqual([{ targetId: 'b', attacked: true, healed: true }]);
  });
  it('controller events without targetId use only an unambiguous controller', () => {
    const events = [{ event: 9, objectId: 'a', data: { amount: 1 } }, { event: 8, objectId: 'a' }, { event: 3, objectId: 'a' }];
    expect(run(events, [source, ctl]).commands.map(c => c.action)).toEqual(['upgradeController', 'reserveController', 'attackController']);
    expect(run(events, [source, ctl, { ...ctl, id: 'other' }])).toMatchObject({ commands: [], skipped: 3 });
  });
  it('completed build prefers explicit coordinates, removed targets fall back to the previous index', () => {
    const r = resolveActions({ current: new Map([[source.id, source]]), previous: new Map([[target.id, target]]), events: [
      { event: 4, objectId: 'a', data: { targetId: 'b', x: 15, y: 16, incomplete: false } },
      { event: 7, objectId: 'a', data: { targetId: 'b' } },
    ] });
    expect(r.commands[0]).toMatchObject({ target: { x: 15, y: 16 } });
    expect(r.commands[1]).toMatchObject({ target: { x: 12, y: 11 } });
  });
  it('actionLog sources resolve through the previous index like events do', () => {
    const r = resolveActions({ current: new Map([[source.id, source]]), previous: new Map([[target.id, target]]),
      actionLogs: [{ id: 'b', actionLog: { rangedAttack: { targetId: 'a' } } }] });
    expect(r.commands[0]).toMatchObject({ action: 'rangedAttack', sourceId: 'b', target: { x: 10, y: 10 } });
  });
  it('missing coordinates skip diagnostics, never invent a zero endpoint', () => {
    expect(run([{ event: 5, objectId: 'a', data: { targetId: 'ghost' } }])).toMatchObject({ commands: [], skipped: 1 });
    const r = resolveActions({ current: new Map([[source.id, source]]), previous: new Map(), actionLogs: [42, { id: 'a', actionLog: { repair: { x: NaN, y: 0 } } }] });
    expect(r).toMatchObject({ commands: [], skipped: 2 });
  });
  it('official null action sentinels mean no action; other malformed values still count', () => {
    const current = new Map([[source.id, source]]);
    const log = Object.fromEntries(['attack', 'rangedAttack', 'heal', 'rangedHeal', 'harvest', 'build', 'repair',
      'upgradeController', 'reserveController', 'attackController', 'claimController', 'transferEnergy', 'power',
      'runReaction', 'reverseReaction', 'say'].map(action => [action, null]));
    const input = { current, previous: current, actionLogs: [{ id: source.id, actionLog: log }] };
    expect(resolveActions(input)).toEqual({ commands: [], hits: [], skipped: 0 });
    expect(resolveActions({ ...input, actionLogs: [{ id: source.id, actionLog: { ...log,
      attack: false, heal: 0, repair: [], runReaction: {}, harvest: { x: 12, y: 11 } } }] }))
      .toMatchObject({ commands: [{ action: 'harvest', target: { x: 12, y: 11 } }], skipped: 4 });
  });
  it('explicit logs override the same event, different actions on the same pair survive', () => {
    const current = new Map([source, target].map(s => [s.id, s]));
    const r = resolveActions({ current, previous: current,
      events: [{ event: 1, objectId: 'a', data: { targetId: 'b', attackType: 2 } }],
      actionLogs: [{ id: 'a', actionLog: { rangedAttack: { x: 12, y: 11 }, rangedHeal: { x: 12, y: 11 } } }],
    });
    expect(r.commands).toHaveLength(2);
    expect(r.commands[0]).toMatchObject({ action: 'rangedAttack', origin: 'actionLog' });
    expect(r.commands[1]).toMatchObject({ action: 'rangedHeal' });
  });
  it('one mass wave per source, hit-back/dismantle/nuke only contribute target covers', () => {
    const events = [3, 3, 4, 5, 6].map(attackType => ({ event: 1, objectId: 'a', data: { targetId: 'b', attackType } }));
    const r = run(events);
    expect(r.commands.map(c => c.action)).toEqual(['rangedMassAttack']);
    expect(r.hits).toEqual([{ targetId: 'b', attacked: true, healed: false }]);
  });
  it('different target identities sharing a cell are not silently collapsed', () => {
    const r = run(['b', 'c'].map(targetId => ({ event: 1, objectId: 'a', data: { targetId, attackType: 2 } })),
      [source, target, { ...target, id: 'c' }]);
    expect(r.commands).toHaveLength(2);
  });
  it('target identity deduplicates a log even when its explicit coordinate differs from the event index', () => {
    const current = new Map([source, target].map(s => [s.id, s]));
    const r = resolveActions({ current, previous: current,
      events: [{ event: 1, objectId: 'a', data: { targetId: 'b', attackType: 2 } }],
      actionLogs: [{ id: 'a', actionLog: { rangedAttack: { targetId: 'b', x: 13, y: 11 } } }],
    });
    expect(r.commands).toHaveLength(1); expect(r.commands[0]).toMatchObject({ origin: 'actionLog', target: { x: 13, y: 11 } });
  });
  it('only confirmed energy link → link transfers get a beam, including type normalization', () => {
    const link = { ...source, type: 'structure', structureType: 'link' };
    const other = { ...target, type: 'structure', structureType: 'link' };
    const event = { event: 12, objectId: 'a', data: { targetId: 'b', resourceType: 'energy', amount: 0 } };
    expect(run([event], [link, other]).commands[0]).toMatchObject({ action: 'transferEnergy', sourceType: 's:link' });
    expect(run([event], [source, other]).commands).toEqual([]);
    expect(run([{ ...event, data: { targetId: 'b', resourceType: 'power' } }], [link, other]).commands).toEqual([]);
  });
  it('reaction direction and say channel precedence are explicit', () => {
    const current = new Map([[source.id, source]]);
    const r = resolveActions({ current, previous: current,
      actionLogs: [{ id: 'a', actionLog: { runReaction: { x1: 3, y1: 4, x2: 5, y2: 6 }, say: { message: 'old', isPublic: false } } }],
      says: [{ id: 'a', message: 'new', private: false }],
    });
    expect(r.commands.slice(0, 2)).toMatchObject([{ source: { x: 3, y: 4 }, target: { x: 10, y: 10 } }, { source: { x: 5, y: 6 } }]);
    expect(r.commands[2]).toMatchObject({ action: 'say', message: 'new', private: false, origin: 'say' });
  });
});
