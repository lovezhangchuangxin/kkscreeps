#!/usr/bin/env node
// Synthetic renderer inputs, explicitly distinct from kk-cli exporter outputs.
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const out = new URL('../fixtures/', import.meta.url);
const room = 'W7N7';
const object = (id, type, x, y, extra = {}) => ({ id, type, room, x, y, ...extra });
const structure = (id, kind, x, y, extra = {}) =>
  object(id, 'structure', x, y, { structureType: kind, user: 'p1', ...extra });
const creep = (id, x, y, body = ['work', 'carry', 'move'], extra = {}) =>
  object(id, 'creep', x, y, {
    name: id,
    user: 'p1',
    body: body.map((type) => ({ type, hits: 100 })),
    hits: body.length * 100,
    hitsMax: body.length * 100,
    spawning: false,
    store: { energy: 30 },
    storeCapacity: 50,
    ...extra,
  });
const target = (x, y, targetId) => ({ x, y, ...(targetId ? { targetId } : {}) });
const snapshot = (objects) => ({ time: 0, room, terrain: { codes: Array(2500).fill(0) }, objects });
const delta = (time, extra = {}) => ({ time, room, upsert: [], removedIds: [], ...extra });
const log = (id, action, value) => ({ id, actionLog: { [action]: value } });
const capacity = { store: { energy: 200 }, storeCapacityResource: { energy: 1000 } };
const objects = [
  creep('melee', 8, 10, ['attack', 'move']),
  creep('melee-target', 9, 10, ['tough', 'move'], { user: 'p2' }),
  creep('ranged', 18, 10, ['ranged_attack', 'move']),
  creep('ranged-target', 21, 10, ['tough', 'move'], { user: 'p2' }),
  creep('healer', 28, 10, ['heal', 'move']),
  creep('heal-target', 31, 10),
  creep('mass', 40, 10, ['ranged_attack', 'move']),
  creep('mass-target', 42, 10, ['tough', 'move'], { user: 'p2' }),
  creep('harvester', 8, 18),
  structure('source', 'source', 9, 18, { energy: 3000, energyCapacity: 3000 }),
  creep('builder', 18, 18),
  object('site', 'constructionSite', 21, 18, {
    structureType: 'extension',
    user: 'p1',
    progress: 1500,
    progressTotal: 3000,
  }),
  creep('repairer', 28, 18),
  structure('wall', 'constructedWall', 31, 18, { hits: 1, hitsMax: 100000 }),
  creep('upgrader', 40, 18),
  structure('controller', 'controller', 42, 18, {
    level: 5,
    progress: 10000,
    downgradeTime: 1000,
    reservation: { user: 'p1' },
    safeMode: 10,
  }),
  structure('tower', 'tower', 8, 26, capacity),
  creep('tower-target', 11, 26, ['tough', 'move'], { user: 'p2' }),
  structure('link-a', 'link', 18, 26, capacity),
  structure('link-b', 'link', 21, 26, capacity),
  structure('lab', 'lab', 28, 26, {
    store: { energy: 100, H: 1000 },
    storeCapacityResource: { energy: 2000, H: 3000 },
    cooldownTime: 2,
  }),
  structure('input-a', 'lab', 26, 26),
  structure('input-b', 'lab', 30, 26),
  object('operator', 'powerCreep', 40, 26, {
    className: 'operator',
    level: 8,
    user: 'p1',
    name: 'operator',
  }),
  structure('power-target', 'tower', 43, 26, capacity),
  creep('reserver', 8, 34, ['claim', 'move']),
  structure('reserve-target', 'controller', 11, 34, { level: 0 }),
  structure('core', 'invaderCore', 18, 34, { user: '2', level: 3 }),
  structure('core-controller', 'controller', 21, 34, { level: 0 }),
  structure('reverse-lab', 'lab', 28, 34, {
    store: { energy: 100, OH: 1500 },
    storeCapacityResource: { energy: 2000, OH: 3000 },
  }),
  structure('reverse-a', 'lab', 26, 34),
  structure('reverse-b', 'lab', 30, 34),
  creep('claimer', 40, 34, ['claim', 'move']),
  structure('claim-target', 'controller', 43, 34, { level: 0 }),
  structure('portal', 'portal', 8, 42),
  structure('keeper', 'keeperLair', 18, 42),
  structure('extractor', 'extractor', 28, 42, { cooldown: 1 }),
  structure('mineral', 'mineral', 28, 42, { mineralType: 'H', mineralAmount: 20000 }),
  structure('factory', 'factory', 38, 42, {
    level: 3,
    cooldownTime: 10,
    store: { energy: 1000 },
    storeCapacity: 50000,
    _isDisabled: true,
  }),
  object('spawn', 'spawn', 8, 47, {
    user: 'p1',
    name: 'spawning',
    store: { energy: 180 },
    storeCapacityResource: { energy: 300 },
    spawning: { spawnTime: 10, needTime: 10 },
  }),
  structure('power-spawn', 'powerSpawn', 18, 47, {
    store: { energy: 1000, power: 50 },
    storeCapacityResource: { energy: 5000, power: 100 },
  }),
  structure('terminal', 'terminal', 28, 47, {
    store: { energy: 5000, power: 500 },
    storeCapacity: 300000,
    cooldownTime: 10,
  }),
  object('nuke', 'nuke', 42, 46, { landTime: 3 }),
  structure('observer', 'observer', 38, 47),
  structure('deposit', 'deposit', 18, 6, {
    depositType: 'biomass',
    harvested: 1000,
    cooldownTime: 10,
  }),
];
const actions = [
  log('melee', 'attack', target(9, 10, 'melee-target')),
  log('ranged', 'rangedAttack', target(21, 10, 'ranged-target')),
  log('healer', 'rangedHeal', target(31, 10, 'heal-target')),
  log('mass', 'rangedMassAttack', true),
  log('harvester', 'harvest', target(9, 18, 'source')),
  log('builder', 'build', target(21, 18, 'site')),
  log('repairer', 'repair', target(31, 18, 'wall')),
  log('upgrader', 'upgradeController', target(42, 18, 'controller')),
  log('tower', 'attack', target(11, 26, 'tower-target')),
  log('link-a', 'transferEnergy', target(21, 26, 'link-b')),
  log('lab', 'runReaction', { x1: 26, y1: 26, x2: 30, y2: 26 }),
  log('operator', 'power', { ...target(43, 26, 'power-target'), powerId: 3 }),
  log('reserver', 'reserveController', target(11, 34, 'reserve-target')),
  log('core', 'reserveController', target(21, 34, 'core-controller')),
  log('reverse-lab', 'reverseReaction', { x1: 26, y1: 34, x2: 30, y2: 34 }),
  log('claimer', 'claimController', target(43, 34, 'claim-target')),
  log('melee-target', 'attacked', true),
  log('ranged-target', 'attacked', true),
  log('heal-target', 'healed', true),
  { id: 'tower-target', actionLog: { attacked: true, healed: true } },
];
const gallery = {
  provenance: 'synthetic renderer gallery; actionLog adapter, not engine output',
  room,
  ticks: [
    snapshot(objects),
    delta(1, {
      actionLogs: actions,
      says: [
        { id: 'melee', message: 'public', private: false },
        { id: 'healer', message: 'private', private: true },
      ],
      upsert: [
        structure('source', 'source', 9, 18, { energy: 2400, energyCapacity: 3000 }),
        { ...objects.find((o) => o.id === 'spawn'), spawning: { spawnTime: 10, needTime: 10 } },
      ],
    }),
    delta(2, {
      actionLogs: actions,
      says: [{ id: 'melee', message: 'new', private: false }],
      upsert: [creep('ranged-target', 22, 10, ['tough', 'move'], { user: 'p2' })],
    }),
    delta(3, {
      removedIds: ['nuke', 'site'],
      actionLogs: actions.filter((a) => a.id !== 'builder'),
    }),
    delta(4, {
      removedIds: ['melee', 'tower', 'operator', 'lab'],
      actionLogs: [log('reverse-lab', 'reverseReaction', { x1: 26, y1: 34, x2: 30, y2: 34 })],
    }),
  ],
};
const lifecycleObjects = [
  creep('a', 20, 20),
  creep('b', 22, 20, ['tough', 'move'], { user: 'p2' }),
  object('site', 'constructionSite', 22, 22, {
    progress: 2995,
    progressTotal: 3000,
    structureType: 'extension',
    user: 'p1',
  }),
  structure('ctl', 'controller', 24, 24, { level: 5, safeMode: 0 }),
  object('nuke', 'nuke', 25, 25, { landTime: 3 }),
];
const lifecycle = {
  provenance: 'synthetic lifecycle boundaries',
  room,
  ticks: [
    snapshot(lifecycleObjects),
    delta(1, {
      upsert: [creep('a', 21, 20)],
      actionLogs: [
        log('a', 'attack', target(22, 20, 'b')),
        log('a', 'rangedAttack', target(22, 20, 'b')),
      ],
      events: [{ event: 1, objectId: 'a', data: { attackType: 2, targetId: 'b' } }],
      says: [{ id: 'a', message: 'moving', private: false }],
    }),
    delta(2, {
      removedIds: ['site', 'b'],
      upsert: [structure('ctl', 'controller', 24, 24, { level: 5, safeMode: 10 })],
      events: [
        { event: 4, objectId: 'a', data: { targetId: 'site', x: 22, y: 22, incomplete: false } },
        { event: 7, objectId: 'a', data: { targetId: 'ghost' } },
      ],
    }),
    delta(3, { removedIds: ['a', 'nuke'] }),
    delta(4, {
      upsert: [creep('a', 30, 30)],
      says: [{ id: 'a', message: 'reborn', private: true }],
    }),
  ],
};
const crowd = Array.from({ length: 50 }, (_, i) =>
  creep(`stress-${i}`, 5 + (i % 10) * 4, 7 + Math.floor(i / 10) * 8, [
    'ranged_attack',
    'heal',
    'move',
  ]),
);
const pressure = {
  provenance: 'synthetic 50-creep concurrent effect smoke, not R7 benchmark',
  room,
  ticks: [
    snapshot(crowd),
    ...Array.from({ length: 30 }, (_, i) =>
      delta(i + 1, {
        upsert: crowd.map((o) => ({ ...o, x: o.x + (i % 2 ? 0 : 1) })),
        actionLogs: crowd.map((o, j) => {
          const t = crowd[(j + 1) % crowd.length];
          return log(
            o.id,
            j % 2 ? 'rangedAttack' : 'rangedHeal',
            target(t.x + (i % 2 ? 0 : 1), t.y, t.id),
          );
        }),
        events: crowd.map((o, j) => ({
          event: j % 2 ? 1 : 6,
          objectId: o.id,
          data: {
            targetId: crowd[(j + 1) % crowd.length].id,
            ...(j % 2 ? { attackType: 2 } : { healType: 2 }),
          },
        })),
      }),
    ),
  ],
};
for (const [name, fixture] of Object.entries({ effects: gallery, lifecycle, pressure })) {
  writeFileSync(fileURLToPath(new URL(`${name}.json`, out)), JSON.stringify(fixture));
  console.log(`✓ ${name}: synthetic, ${fixture.ticks.length} ticks`);
}
