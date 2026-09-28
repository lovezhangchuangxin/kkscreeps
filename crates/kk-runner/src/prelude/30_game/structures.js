/* kk-runner prelude · 30_game 段 5：Structure 族装配（对照官方
 * engine/src/game/structures.js + sources.js + minerals.js + store.js）。
 *
 * makeStore（Store 垫片，store.js 语义的 M0 形态）、spawn/structure 装配 case
 * （controller/source/mineral/其余结构的分支投影）。
 * 依赖：gw.viewerName、gw.mkStubMethod。
 */
(() => {
  'use strict';

  const { reportMissing, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  const SPAWN_METHODS = ['spawnCreep', 'renewCreep', 'recycleCreep', 'destroy', 'setActive', 'notifyWhenAttacked'];
  const STRUCTURE_METHODS = ['destroy', 'setActive', 'notifyWhenAttacked'];
  const CONTROLLER_METHODS = ['activateSafeMode'];
  const SOURCE_METHODS = [];

  function makeStore(storeSpec) {
    const store = Object.assign({}, storeSpec);
    store.getUsedCapacity = (resource) => storeSpec[resource || 'energy'] || 0;
    store.getFreeCapacity = () => 0;
    store.getCapacity = () => null;
    return new Proxy(store, {
      get(t, prop) {
        if (prop in t) return t[prop];
        if (typeof prop === 'string') reportMissing('store.' + prop);
        return undefined;
      },
    });
  }
  gw.makeStore = makeStore;

  // ---------------------------------------------------------------- spawn 装配
  gw.assemblers.spawn = (o, base) => {
    const obj = Object.assign(base, {
      name: o.name,
      my: o.owner === gw.viewerName,
      owner: { username: o.owner || 'bot' },
      structureType: o.structureType || 'spawn',
      store: makeStore(o.store || { energy: 0 }),
      energy: (o.store && o.store.energy) || 0,
      energyCapacity: o.energyCapacity || 300,
      spawning: null,
      isStructure: true,
    });
    obj.isActive = () => { countStub('structure.isActive'); return true; };
    for (const m of SPAWN_METHODS) obj[m] = gw.mkStubMethod('spawn.' + m);
    return obj;
  };

  // ---------------------------------------------------------------- structure 装配
  gw.assemblers.structure = (o, base, _pos, now) => {
    const obj = Object.assign(base, {
      structureType: o.structureType,
      my: o.owner === gw.viewerName,
      owner: o.owner ? { username: o.owner } : undefined,
      isStructure: true,
      store: makeStore(o.store || {}),
    });
    if (o.structureType === 'controller') {
      obj.level = o.level || 0;
      obj.progress = o.progress || 0;
      obj.progressTotal = o.progressTotal || 0;
      obj.reservation = undefined;
      // 官方 getter（structures.js:174-186）：绝对存储 − 当前 tick = 剩余量；
      // 无值/已过期归 0（None→0 投影归一，灰区登记口径）
      obj.safeMode = o.safeMode && o.safeMode > now ? o.safeMode - now : 0;
      obj.safeModeAvailable = o.safeModeAvailable ?? 0;
      obj.safeModeCooldown = 0;
      obj.ticksToDowngrade = o.downgradeTime != null ? o.downgradeTime - now : 0;
      obj.activationDate = undefined;
      // 官方 controller.isPowerEnabled（power API 本体 M2+，字段先行透传）
      obj.isPowerEnabled = o.isPowerEnabled === true;
      for (const m of CONTROLLER_METHODS) obj[m] = gw.mkStubMethod('controller.' + m);
    } else if (o.structureType === 'source') {
      obj.energy = o.energy ?? 3000;
      obj.energyCapacity = o.energyCapacity ?? 3000;
      obj.ticksToRegeneration = o.ticksToRegeneration ?? 0;
    } else if (o.structureType === 'mineral') {
      obj.mineralType = o.mineralType || 'XUH2O';
      obj.mineralAmount = o.mineralAmount ?? 0;
      obj.ticksToRegeneration = o.ticksToRegeneration ?? 0;
      obj.density = o.density ?? 3;
    } else {
      obj.energy = (o.store && o.store.energy) || 0;
      obj.energyCapacity = o.energyCapacity || 0;
      obj.isActive = () => { countStub('structure.isActive'); return true; };
      for (const m of STRUCTURE_METHODS) obj[m] = gw.mkStubMethod('structure.' + m);
    }
    return obj;
  };
})();
