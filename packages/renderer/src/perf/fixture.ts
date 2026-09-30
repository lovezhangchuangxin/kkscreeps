/** R7 synthetic full room. No engine state or exporter is read by this generator. */
import type { RoomObjectState, RoomSnapshot, RoomDelta, RoomVisual } from '@kkscreeps/shared';

export const PERF_ROOM = 'R7-PERF';
export const PERF_COUNTS: Readonly<Record<string, number>> = {
  road: 60, extension: 60, rampart: 50, constructedWall: 20, spawn: 3,
  tower: 6, link: 6, lab: 10, container: 10, storage: 2, terminal: 2, factory: 2,
  powerSpawn: 1, observer: 1, extractor: 1, nuker: 1, controller: 1,
  source: 2, mineral: 1, deposit: 1, keeperLair: 1, portal: 1, constructionSite: 8,
  creep: 50,
};

export function makeFullRoom(prototypes: readonly RoomObjectState[]) {
  const objects: RoomObjectState[] = [];
  for (const [key, count] of Object.entries(PERF_COUNTS)) {
    const prototype = prototypes.find(o => (o.type === 'structure' ? o.structureType : o.type) === key);
    if (!prototype && key !== 'road' && key !== 'constructedWall') throw new Error(`Missing prototype ${key}`);
    for (let i = 0; i < count; i++) {
      const n = objects.length;
      const x = 4 + (n % 21) * 2, y = 4 + Math.floor(n / 21) * 2;
      const o: RoomObjectState = structuredClone(prototype ?? { type: 'structure', structureType: key, hits: 10000, hitsMax: 10000, id: '' });
      Object.assign(o, { id: `r7-perf-${key}-${i}`, room: PERF_ROOM, x, y, pos: { x, y, roomName: PERF_ROOM } });
      if (key === 'rampart') { o.user = o.owner = `r7-owner-${i % 4}`; o.isPublic = i >= 40; }
      if (key === 'creep') {
        o.name = `r7-creep-${i}`; o.user = o.owner = 'r7-local';
        o.body = Array.from({ length: 50 }, (_, b) => ({ type: ['move', 'work', 'carry', 'attack', 'ranged_attack', 'heal', 'tough'][b % 7], hits: 100 }));
        o.hits = o.hitsMax = 5000; o.store = { energy: 100 }; o.storeCapacity = 350; o.ticksToLive = 1500;
      }
      if (key === 'controller') { o.user = o.owner = 'r7-local'; o.level = 8; o.safeMode = 0; }
      if (key === 'spawn') { o.name = `R7Spawn${i}`; o.user = o.owner = 'r7-local'; }
      objects.push(o);
    }
  }
  // All declared terrain bits appear. Lava has no distinct renderer feature (registered G5).
  const codes = Array.from({ length: 2500 }, (_, i) => i % 97 === 0 ? 1 : i % 11 === 0 ? 2 : i % 83 === 0 ? 4 : 0);
  const snapshot: RoomSnapshot = { room: PERF_ROOM, time: 0, terrain: { codes }, objects };
  return {
    room: PERF_ROOM,
    provenance: { kind: 'synthetic', generator: 'scripts/build-perf-fixtures.mjs', seed: 7, prototypes: 'showcase.json', objects: 300, creeps: 50, bodyPartsPerCreep: 50, visuals: 100, terrainBits: [0, 1, 2, 4], engineExporter: false },
    ticks: [snapshot], visuals: fullVisuals(0),
  };
}

export function fullVisuals(tick: number): RoomVisual[] {
  return Array.from({ length: 100 }, (_, i): RoomVisual => {
    const x = 3 + (i % 10) * 4.5, y = 3 + Math.floor(i / 10) * 4.5;
    switch (i % 5) {
      case 0: return { type: 'line', points: [{ x, y }, { x: x + 2, y: y + 1 }], style: { width: 0.15, color: '#44ddff', opacity: 0.7, lineStyle: 'dashed' } };
      case 1: return { type: 'circle', x, y, style: { radius: 0.7, fill: '#eeaa33', stroke: '#fff', opacity: 0.7 } };
      case 2: return { type: 'rect', x, y, w: 1.5, h: 1, style: { fill: '#33ee77', stroke: '#fff', opacity: 0.7 } };
      case 3: return { type: 'poly', points: [{ x, y }, { x: x + 1, y: y + 1.5 }, { x: x - 1, y: y + 1.5 }, { x, y }], style: { fill: '#aa77ff', stroke: '#fff', opacity: 0.7 } };
      default: return { type: 'text', x, y, text: `R7 ${i} ${tick % 100}`, style: { font: 'bold 0.6 Roboto', backgroundColor: '#222', stroke: '#000', color: '#fff', opacity: 1 } };
    }
  });
}

/** Absolute tick state: deterministic, bounded IDs, no accumulation of old history. */
export function fullDelta(base: RoomSnapshot, tick: number, moving = true, churn = false): RoomDelta {
  const upsert: RoomObjectState[] = [], removedIds: string[] = [];
  const creeps = base.objects.filter(o => o.type === 'creep');
  for (let i = 0; i < creeps.length; i++) {
    const original = creeps[i]!;
    if (churn && i < 5 && tick % 100 >= 50 && tick % 100 < 55) { removedIds.push(original.id); continue; }
    const x = original.x! + (moving ? tick % 2 : 0), y = original.y!;
    upsert.push({ ...original, x, y, pos: { x, y, roomName: PERF_ROOM } });
  }
  const actionLogs = moving ? upsert.map(o => ({ id: o.id, actionLog: { rangedAttack: { x: o.x! + 1, y: o.y!, targetId: base.objects[60]!.id } } })) : [];
  // Two channels carry the same attack; resolver deduplication must keep 50 beams.
  const events = moving ? upsert.map(o => ({ event: 1, objectId: o.id, data: { targetId: base.objects[60]!.id, attackType: 2, damage: 10 } })) : [];
  return { room: PERF_ROOM, time: tick, upsert, removedIds, actionLogs, events,
    says: churn && tick % 10 === 0 ? upsert.slice(0, 5).map(o => ({ id: o.id, message: `soak ${tick % 100}`, private: false })) : [],
  };
}
