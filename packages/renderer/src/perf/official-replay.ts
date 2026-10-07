/** Read-only official demo adaptation; no official runtime dependency. */
import type { ActionLogEntry, FixtureFile, RoomObjectState, RoomSnapshot } from '@kkscreeps/shared';

type JsonObject = Record<string, unknown>;
export interface OfficialReplay {
  room: string;
  ticks: Record<string, Record<string, JsonObject | null>>;
}
const isObject = (value: unknown): value is JsonObject => value !== null && typeof value === 'object';

/** Same JSON deep-merge behavior as the demo's lodash merge, including nested null. */
function merge(target: unknown, patch: unknown): unknown {
  if (!isObject(patch)) return patch;
  const out: JsonObject = Array.isArray(patch)
    ? (Array.isArray(target) ? structuredClone(target) : []) as unknown as JsonObject
    : isObject(target) ? structuredClone(target) : {};
  for (const [key, value] of Object.entries(patch)) {
    if (key === '__proto__' || key === 'constructor' || key === 'prototype') continue;
    out[key] = merge(out[key], value);
  }
  return out;
}

/** Raw restored frames retain official array-to-record semantics for oracle comparison. */
export function restoreOfficialFrames(source: OfficialReplay): Array<{ time: number; objects: JsonObject[] }> {
  const objects = new Map<string, JsonObject>();
  const frames: Array<{ time: number; objects: JsonObject[] }> = [];
  for (const tick of Object.keys(source.ticks).sort((a, b) => Number(a) - Number(b))) {
    const time = Number(tick);
    if (!Number.isSafeInteger(time)) throw new Error(`Invalid official tick: ${tick}`);
    for (const [id, patch] of Object.entries(source.ticks[tick]!)) {
      if (patch === null) { objects.delete(id); continue; }
      const before = objects.get(id);
      // demo/src/index.jsx arraysToObject is shallow, applied only to existing objects.
      if (before) {
        const previous = structuredClone(before), diff = structuredClone(patch);
        for (const value of [previous, diff]) for (const [key, array] of Object.entries(value)) {
          if (Array.isArray(array)) value[key] = Object.fromEntries(array.map((item, i) => [i, item]));
        }
        objects.set(id, merge(previous, diff) as JsonObject);
      } else objects.set(id, structuredClone(patch));
    }
    frames.push({ time, objects: structuredClone([...objects.values()]) });
  }
  if (frames.length === 0) throw new Error('Official replay has no ticks');
  return frames;
}

const structures = new Set(['source', 'mineral', 'controller', 'road', 'extension', 'rampart',
  'constructedWall', 'tower', 'link', 'lab', 'storage', 'terminal', 'container', 'extractor',
  'observer', 'powerSpawn', 'powerBank', 'nuker', 'factory', 'keeperLair', 'portal', 'deposit', 'invaderCore']);

export function projectOfficialObject(object: JsonObject, room: string): RoomObjectState {
  const copy = structuredClone(object);
  const id = copy._id;
  if (typeof id !== 'string' || typeof copy.type !== 'string') throw new Error('Official object lacks _id/type');
  if (isObject(copy.body) && !Array.isArray(copy.body)) {
    const keys = Object.keys(copy.body).sort((a, b) => Number(a) - Number(b));
    if (keys.some((key, i) => key !== String(i))) throw new Error(`Non-dense body: ${id}`);
    copy.body = keys.map(key => (copy.body as JsonObject)[key]);
  }
  const originalType = copy.type;
  if (structures.has(originalType)) { copy.type = 'structure'; copy.structureType = originalType; }
  else if (originalType === 'spawn') copy.structureType = 'spawn';
  else if (originalType === 'energy') { copy.type = 'resource'; copy.resourceType = 'energy'; copy.amount = copy.energy; }
  const x = copy.x, y = copy.y;
  return { ...copy, id, type: copy.type as string, room,
    ...(typeof x === 'number' && typeof y === 'number' ? { pos: { x, y, roomName: room } } : {}) };
}

export function makeOfficialFixture(source: OfficialReplay, terrain: readonly JsonObject[]): FixtureFile {
  const codes = new Array<number>(2500).fill(0);
  for (const cell of terrain) {
    if (typeof cell.x !== 'number' || typeof cell.y !== 'number' || !Number.isInteger(cell.x) || !Number.isInteger(cell.y)
      || cell.x < 0 || cell.x >= 50 || cell.y < 0 || cell.y >= 50) throw new Error('Invalid official terrain cell');
    codes[cell.y * 50 + cell.x]! |= cell.type === 'wall' ? 1 : cell.type === 'swamp' ? 2 : 0;
  }
  const restored = restoreOfficialFrames(source);
  let previous = new Map<string, string>();
  const ticks = restored.map((frame, i) => {
    const objects = frame.objects.map(o => projectOfficialObject(o, source.room));
    const current = new Map(objects.map(o => [o.id, JSON.stringify(o)]));
    if (i === 0) { previous = current; return { room: source.room, time: frame.time, terrain: { codes }, objects } as RoomSnapshot; }
    const actionLogs: ActionLogEntry[] = objects.filter(o => isObject(o.actionLog)).map(o => ({ id: o.id, actionLog: o.actionLog as ActionLogEntry['actionLog'] }));
    const delta = { room: source.room, time: frame.time,
      upsert: objects.filter(o => previous.get(o.id) !== current.get(o.id)),
      removedIds: [...previous.keys()].filter(id => !current.has(id)), actionLogs };
    previous = current;
    return delta;
  });
  // restoreOfficialFrames rejects empty input; map emits one snapshot then deltas.
  return { room: source.room, ticks: ticks as FixtureFile['ticks'] };
}
