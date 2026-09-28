/* kk-runner prelude · 30_game 段 5：Structure 族装配（对照官方
 * engine/src/game/structures.js + sources.js + minerals.js + store.js）。
 *
 * A3b 薄壳化：字段表挂各类原型（官方 defineGameObjectProperties 挂 prototype
 * 的同构），实例 own 面只剩 id/pos/room + 少量静态标记。getter 经
 * gw.rawData(this.id) 回查当前 tick 投影（消亡抛错/旧壳读新值）。
 * 值换算口径保持 A1 起的既有语义（None→0 归一等灰区口径不变——A3b 只改
 * 访问机制不改值语义；官方有而现状未暴露的字段（storage.storeCapacity、
 * road.ticksToDecay、reservation/sign 等）随 A4-iii/Store 族批次补）。
 * 依赖：gw.viewerName、gw.rawData、gw.mkStubMethod、gw.defineGameObjectProperties。
 */
(() => {
  'use strict';

  const { reportMissing, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  const SPAWN_METHODS = ['spawnCreep', 'renewCreep', 'recycleCreep', 'destroy', 'setActive', 'notifyWhenAttacked'];
  const STRUCTURE_METHODS = ['destroy', 'setActive', 'notifyWhenAttacked'];
  const CONTROLLER_METHODS = ['activateSafeMode'];

  function makeStore(storeSpec) {
    // 预置 Object（装配发生在玩家 tick 内的 getter 首访——不得经可交换的
    // 全局 Object，对抗评审 P1 的预置捕获全家桶原则；store 副本用 __Object
    // 构造，玩家交换 globalThis.Object 只影响玩家自己的代码路径）
    const store = gw.__Object.assign({}, storeSpec);
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

  // 当前 tick（getter 体内换算用——官方 getter 读 runtimeData.time 同款）
  function now() {
    return gw.world.time;
  }

  // ---------------------------------------------------------------- 原型字段表
  // Structure 基类（官方 structures.js:60-65）：hits/hitsMax/structureType
  gw.defineGameObjectProperties(globalThis.Structure.prototype, gw.rawData, {
    hits: (o) => o.hits ?? 0,
    hitsMax: (o) => o.hitsMax ?? 0,
    structureType: (o) => o.structureType,
  });

  // OwnedStructure（官方 structures.js:135-139）：owner 的 undefined 语义
  // （_.isUndefined(o.user) || o.user === null ? undefined——降级归零的
  // user:null 哨兵同样归 undefined）；my 三态（官方 :139，null 走比较得
  // false——我方 === viewerName 同构）
  gw.defineGameObjectProperties(globalThis.OwnedStructure.prototype, gw.rawData, {
    owner: (o) => (o.owner != null ? { username: o.owner } : undefined),
    my: (o) => (o.owner === undefined ? undefined : o.owner === gw.viewerName),
  });

  // Source（官方 sources.js:38-43）
  gw.defineGameObjectProperties(globalThis.Source.prototype, gw.rawData, {
    energy: (o) => o.energy ?? 3000,
    energyCapacity: (o) => o.energyCapacity ?? 3000,
    ticksToRegeneration: (o) => o.ticksToRegeneration ?? 0,
  });

  // Mineral（官方 minerals.js:40-46）——world_json 暂不投影 mineral 对象，
  // 表先挂（防御分支的现状口径）
  gw.defineGameObjectProperties(globalThis.Mineral.prototype, gw.rawData, {
    mineralType: (o) => o.mineralType || 'XUH2O',
    mineralAmount: (o) => o.mineralAmount ?? 0,
    density: (o) => o.density ?? 3,
    ticksToRegeneration: (o) => o.ticksToRegeneration ?? 0,
  });

  // Controller（官方 structures.js:139-174 的现状子集——换算口径保持现状：
  // downgradeTime/safeMode 绝对存储 − 当前 tick；无值/已过期归 0（None→0
  // 投影归一，灰区登记口径）；progress/progressTotal 直存；safeModeCooldown
  // 现状硬编码 0（投影缺字段，A4-iii 随 B2 schema 补投影后换真）
  gw.defineGameObjectProperties(globalThis.StructureController.prototype, gw.rawData, {
    level: (o) => o.level || 0,
    progress: (o) => o.progress || 0,
    progressTotal: (o) => o.progressTotal || 0,
    ticksToDowngrade: (o) => (o.downgradeTime != null ? o.downgradeTime - now() : 0),
    safeMode: (o) => (o.safeMode && o.safeMode > now() ? o.safeMode - now() : 0),
    safeModeAvailable: (o) => o.safeModeAvailable ?? 0,
    safeModeCooldown: () => 0,
    isPowerEnabled: (o) => o.isPowerEnabled === true,
    reservation: () => undefined,
    activationDate: () => undefined,
  });

  // StructureSpawn（官方 structures.js:1245-1251 的现状子集；spawning 官方是
  // Spawning 实例——现状硬 null，A4-i spawn 族连 StructureSpawn.Spawning 一起换真）。
  // structureType 子类覆盖：spawn 投影的 type 键即结构类型（手写世界 JSON
  // 可无 structureType 键——保持现状值拷贝的 'spawn' 缺省）
  gw.defineGameObjectProperties(globalThis.StructureSpawn.prototype, gw.rawData, {
    structureType: (o) => o.structureType || 'spawn',
    name: (o) => o.name,
    energy: (o) => (o.store && o.store.energy) || 0,
    energyCapacity: (o) => o.energyCapacity || 300,
    spawning: () => null,
    store: (o) => gw.makeStore(o.store || { energy: 0 }),
  });

  // 其余现状暴露 store/energy/energyCapacity 的结构（extension/storage/road/
  // wall——else 族现状口径：energyCapacity 缺省 0）：官方各子类有独立字段表，
  // 我方按现状暴露面挂各自的类原型
  const ELSE_STRUCTURE_FIELDS = {
    energy: (o) => (o.store && o.store.energy) || 0,
    energyCapacity: (o) => o.energyCapacity || 0,
    store: (o) => gw.makeStore(o.store || {}),
  };
  for (const cls of ['StructureExtension', 'StructureStorage', 'StructureRoad', 'StructureWall']) {
    gw.defineGameObjectProperties(globalThis[cls].prototype, gw.rawData, ELSE_STRUCTURE_FIELDS);
  }

  // ---------------------------------------------------------------- 原型方法
  // controller（activateSafeMode 官方 structures.js:223-231 六门预检——A4-iii
  // 换真；桩先挂原型保持实例 own 面干净）
  for (const m of CONTROLLER_METHODS) {
    globalThis.StructureController.prototype[m] = gw.mkStubMethod('controller.' + m);
  }
  // spawn（spawnCreep 是 ~691 校验矩阵用例的公共前置——A4-i 换真）
  for (const m of SPAWN_METHODS) {
    globalThis.StructureSpawn.prototype[m] = gw.mkStubMethod('spawn.' + m);
  }
  // 通用结构方法（notifyWhenAttacked 官方 structures.js:89-101——A4 换真）：
  // 不挂 Structure 基类原型（controller/source/mineral 现状无这些方法，
  // 挂基类会让它们经原型链继承——语义变化）；else 族按类原型/实例兜底挂载

  // ---------------------------------------------------------------- 装配
  // spawn 装配：字段/方法全在原型，own 面只剩 id/pos/room + 静态标记
  gw.assemblers.spawn = (o, base) => {
    base.isStructure = true;
    base.isActive = () => { countStub('structure.isActive'); return true; };
    return base;
  };

  gw.assemblers.structure = (o, base) => {
    base.isStructure = true;
    if (o.structureType === 'controller' || o.structureType === 'source' || o.structureType === 'mineral') {
      return base; // 字段/方法全在各自类原型
    }
    // else 族：extension/storage/road/wall 有类原型字段表 + 类原型方法；
    // 未来新结构类型兜底实例级 getter（同一回查机制）
    const cls = { extension: 'StructureExtension', storage: 'StructureStorage',
      road: 'StructureRoad', constructedWall: 'StructureWall', wall: 'StructureWall' }[o.structureType];
    if (!cls) {
      gw.defineGameObjectProperties(base, gw.rawData, ELSE_STRUCTURE_FIELDS);
      for (const m of STRUCTURE_METHODS) base[m] = gw.mkStubMethod('structure.' + m);
    } else {
      for (const m of STRUCTURE_METHODS) {
        globalThis[cls].prototype[m] = globalThis[cls].prototype[m]
          || gw.mkStubMethod('structure.' + m);
      }
    }
    base.isActive = () => { countStub('structure.isActive'); return true; };
    return base;
  };
})();
