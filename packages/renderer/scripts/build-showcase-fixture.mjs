/**
 * showcase 合成夹具生成器（R3；G5：引擎无这些对象——描述符+实现的合成取材）。
 * 确定性纯函数生成，产物入库（fixtures/showcase.json），重跑逐字节一致。
 *
 * 覆盖：container/link/lab/terminal/factory/nuker/observer/powerBank/
 * powerSpawn/portal/keeperLair/invaderCore/deposit/public+private rampart/
 * flag/constructionSite/敌我双色结构/NPC 塔/_isDisabled/废墟/墓碑。
 * 运行：node scripts/build-showcase-fixture.mjs
 */
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');

let nextId = 1;
function id() {
  return `sc${String(nextId++).padStart(4, '0')}`;
}

function structure(type, x, y, fields) {
  return { type: 'structure', structureType: type, id: id(), room: 'W0C0', x, y, ...fields };
}

function creep(name, user, x, y, body, fields) {
  return {
    type: 'creep', id: id(), room: 'W0C0', x, y, name, user, owner: user,
    body: body.map(([type, hits]) => ({ type, hits })),
    hits: body.reduce((sum, [, h]) => sum + h, 0),
    spawning: false, fatigue: 0,
    ...fields,
  };
}

function bp(type, n, hits = 100) {
  return Array.from({ length: n }, () => [type, hits]);
}

const objects = [
  // ---- 己方（p1）大宗结构群（左上象限） ----
  structure('extension', 8, 8, { user: 'p1', owner: 'p1', store: { energy: 50 }, storeCapacityResource: { energy: 50 }, hits: 1000, hitsMax: 1000 }),
  structure('extension', 10, 8, { user: 'p1', owner: 'p1', store: { energy: 0 }, storeCapacityResource: { energy: 50 }, hits: 1000, hitsMax: 1000, _isDisabled: true }),
  structure('storage', 12, 10, { user: 'p1', owner: 'p1', store: { energy: 300000, G: 20000, H: 100000 }, storeCapacity: 1000000, hits: 10000, hitsMax: 10000 }),
  structure('tower', 10, 12, { user: 'p1', owner: 'p1', hits: 3000, hitsMax: 3000, store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }),
  structure('lab', 14, 12, { user: 'p1', owner: 'p1', hits: 500, hitsMax: 500, store: { energy: 300, H: 1500 }, storeCapacity: 10000, storeCapacityResource: { energy: 1000, H: 5000 }, cooldownTime: null }),
  structure('terminal', 16, 14, { user: 'p2', owner: 'p2', hits: 3000, hitsMax: 3000, store: { energy: 1500, H: 400, power: 100 }, storeCapacity: 3000, cooldownTime: null }),
  structure('factory', 12, 14, { user: 'p1', owner: 'p1', hits: 1000, hitsMax: 1000, level: 2, store: { energy: 300 }, storeCapacity: 1000, cooldownTime: null }),
  structure('nuker', 16, 10, { user: 'p1', owner: 'p1', hits: 1000, hitsMax: 1000, store: { energy: 400, G: 30 }, storeCapacityResource: { energy: 500, G: 100 } }),
  structure('link', 8, 12, { user: 'p1', owner: 'p1', hits: 1000, hitsMax: 1000, store: { energy: 400 }, storeCapacityResource: { energy: 800 } }),
  structure('observer', 8, 10, { user: 'p1', owner: 'p1', hits: 500, hitsMax: 500 }),
  structure('powerSpawn', 14, 8, { user: 'p1', owner: 'p1', hits: 1000, hitsMax: 1000, store: { energy: 2000, power: 60 }, storeCapacity: 5000, storeCapacityResource: { energy: 5000, power: 1000 } }),
  structure('container', 18, 12, { store: { energy: 500, power: 100, H: 50 }, storeCapacity: 2000, hits: 250000, hitsMax: 250000 }),
  structure('deposit', 20, 10, { depositType: 'silicon', harvested: 10000, cooldownTime: 0 }),
  structure('extractor', 20, 20, { user: 'p1', owner: 'p1', hits: 500, hitsMax: 500, cooldown: 0 }),
  structure('source', 22, 8, { energy: 3000, energyCapacity: 3000, ticksToRegeneration: 10 }),
  structure('mineral', 22, 20, { mineralType: 'K', mineralAmount: 35000, density: 3, ticksToRegeneration: 0 }),

  // ---- NPC/中立家族（右上象限） ----
  structure('tower', 28, 8, { user: '3', owner: '3', hits: 3000, hitsMax: 3000, store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }),
  structure('invaderCore', 30, 10, { spawning: { needTime: 120, spawnTime: 160 }, decayTime: 51600 }),
  structure('powerBank', 32, 12, { store: { power: 2500 }, hits: 20000, hitsMax: 20000 }),
  structure('keeperLair', 28, 14, { ticksToSpawn: 100 }),
  structure('portal', 32, 8, {}),

  // ---- rampart：p1 私有簇 + p2 私有簇 + p1 公有单格 ----
  ...[[20, 24], [21, 24], [22, 24], [20, 25], [21, 25]].map(([x, y]) =>
    structure('rampart', x, y, { user: 'p1', owner: 'p1', isPublic: false, hits: 5000, hitsMax: 5000 })),
  ...[[26, 24], [27, 24], [26, 25]].map(([x, y]) =>
    structure('rampart', x, y, { user: 'p2', owner: 'p2', isPublic: false, hits: 5000, hitsMax: 5000 })),
  structure('rampart', 30, 26, { user: 'p1', owner: 'p1', isPublic: true, hits: 5000, hitsMax: 5000 }),

  // ---- 衰减/杂项 ----
  { type: 'tombstone', id: id(), room: 'W0C0', x: 10, y: 20, deathTime: 0, decayTime: 90, ticksToDecay: 90, creepId: null, creepName: 'fallen', creepTicksToLive: 800, creepBody: ['move', 'work'], store: { energy: 120 }, user: 'p1' },
  { type: 'ruin', id: id(), room: 'W0C0', x: 12, y: 20, destroyTime: 0, decayTime: 500, ticksToDecay: 500, store: { energy: 200, H: 10 }, user: 'p2' },
  { type: 'resource', id: id(), room: 'W0C0', x: 14, y: 20, resourceType: 'energy', amount: 450 },
  { type: 'resource', id: id(), room: 'W0C0', x: 15, y: 21, resourceType: 'power', amount: 600, powerCapacity: 900 },
  { type: 'constructionSite', id: id(), room: 'W0C0', x: 18, y: 18, structureType: 'extension', progress: 200, progressTotal: 3000, owner: 'p2' },
  { type: 'flag', id: id(), room: 'W0C0', x: 24, y: 14, name: 'camp', color: 1, secondaryColor: 5, user: 'p1' },

  // ---- 双方 spawn（名字只显示己方） ----
  { type: 'spawn', structureType: 'spawn', id: id(), room: 'W0C0', x: 10, y: 16, name: 'Spawn1', user: 'p1', owner: 'p1', store: { energy: 180 }, energy: 180, energyCapacity: 300, storeCapacityResource: { energy: 300 }, off: false, hits: 5000, hitsMax: 5000, spawning: { name: 'bootstrapper', needTime: 20, spawnTime: 20, directions: [1, 3, 5, 7] } },
  { type: 'spawn', structureType: 'spawn', id: id(), room: 'W0C0', x: 28, y: 18, name: 'EnemySpawn', user: 'p2', owner: 'p2', store: { energy: 90 }, energy: 90, energyCapacity: 300, storeCapacityResource: { energy: 300 }, off: false, hits: 5000, hitsMax: 5000 },

  // ---- controller（等级环+进度扇形） ----
  // safeMode 1000（R4）：controller 状态圈转 safemode 橙 + 全部 p2 creep/powerCreep
  // α0.5（官方 calculations.safeMode——controller.user!==user）——safemode 演示区。
  structure('controller', 5, 5, { user: 'p1', owner: 'p1', level: 2, progress: 20000, progressTotal: 45000, safeMode: 1000, safeModeAvailable: 1, downgradeTime: null, isPowerEnabled: false, upgradeBlocked: null, safeModeCooldown: null, reservation: null, sign: null }),
  // 敌方预定 controller（红状态圈）
  structure('controller', 40, 40, { user: 'p2', owner: 'p2', level: 3, progress: 0, progressTotal: 135000, safeMode: 0, downgradeTime: null, isPowerEnabled: false, upgradeBlocked: null, reservation: { user: 'p2', ticksToEnd: 5000 }, sign: null }),

  // ---- creep 展示区（R4：body 环形态谱系，下半场 y 32..40） ----
  // 满环 50 work（前向铺满两侧各 π）
  creep('ring-work50', 'p1', 10, 32, bp('work', 50)),
  // 满环 50 move（自正下铺满——backSide）
  creep('ring-move50', 'p1', 13, 32, bp('move', 50)),
  // 混色环：合并升序 ranged100→heal100→claim100→attack200→work300→move800
  // （carry 在列但被过滤——creepBuildBody.js:108-109）
  creep('ring-mixed', 'p1', 16, 32, [...bp('work', 3), ...bp('attack', 2), ...bp('ranged_attack', 1), ...bp('heal', 1), ...bp('claim', 1), ...bp('carry', 1), ...bp('move', 8)]),
  // 掉血环：部分部件 hits<100 → 弧短
  creep('ring-damaged', 'p1', 19, 32, [...bp('work', 1, 40), ...bp('work', 1, 70), ...bp('attack', 1, 100), ...bp('move', 2, 90)]),
  // tough 家族：中心 tough.svg + heal 弧
  creep('ring-tough', 'p1', 22, 32, [...bp('tough', 3), ...bp('heal', 1), ...bp('move', 2)]),
  // tough 全灭：无 tough 贴图（creepBuildBody.js:154-156 hits>0 门）
  creep('ring-tough-dead', 'p1', 25, 32, [...bp('tough', 2, 0), ...bp('work', 2)]),
  // NPC（user 3）：整只 creep-npc.svg；受 safemode 联动 α0.5（官方 calc 不排 NPC）
  creep('npc-keeper', '3', 28, 32, bp('attack', 5)),
  // 孵化中：无名字无 mask/glow（body 环照画——官方无 spawning 门）
  creep('spawner-creep', 'p1', 31, 32, bp('work', 2), { spawning: true }),
  // 资源圆三层（白 total→红 power→黄 energy）：
  creep('carry-energy', 'p1', 10, 36, [...bp('carry', 5), ...bp('move', 5)], { store: { energy: 400 }, storeCapacity: 800 }),
  creep('carry-rings', 'p1', 13, 36, [...bp('carry', 5), ...bp('move', 5)], { store: { energy: 100, power: 100, H: 600 }, storeCapacity: 800 }),
  creep('carry-mixed', 'p1', 16, 36, [...bp('carry', 4), ...bp('move', 4)], { store: { energy: 50, power: 50, H: 100 }, storeCapacity: 400 }),
  // 敌方 creep：safemode 期间 α0.5
  creep('enemy-dim', 'p2', 10, 40, [...bp('attack', 2), ...bp('move', 2)]),
  // powerCreep（G5 家族）：className-lvl 纹理 + 红 glow + 名字
  { type: 'powerCreep', id: id(), room: 'W0C0', x: 13, y: 40, name: 'pc-operator', user: 'p1', owner: 'p1', className: 'operator', level: 12 },
  { type: 'powerCreep', id: id(), room: 'W0C0', x: 16, y: 40, name: 'pc-executor', user: 'p2', owner: 'p2', className: 'executor', level: 30 },
  // 己方对照（不受 safemode 变暗）
  creep('friendly-bright', 'p1', 19, 40, [...bp('heal', 1), ...bp('move', 3)]),
];

// ---- 增量链：lab 冷却启动 → lab 矿物清空（值变化驱动重应用的两步取材） ----
const t1 = structuredClone(objects).find((o) => o.structureType === 'lab');
t1.cooldownTime = 30;
const t2 = structuredClone(objects).find((o) => o.structureType === 'lab');
t2.store = { energy: 600, H: 0 };
t2.cooldownTime = null;

const fixture = {
  room: 'W0C0',
  ticks: [
    { time: 0, room: 'W0C0', terrain: { codes: new Array(2500).fill(0) }, objects },
    { time: 10, room: 'W0C0', upsert: [t1], removedIds: [] },
    { time: 20, room: 'W0C0', upsert: [t2], removedIds: [] },
  ],
};

const out = join(ROOT, 'fixtures', 'showcase.json');
writeFileSync(out, `${JSON.stringify(fixture, null, 1)}\n`);
console.log(`showcase fixture → ${out}（对象 ${objects.length}，ticks ${fixture.ticks.length}）`);
