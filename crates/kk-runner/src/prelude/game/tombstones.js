/* 墓碑与废墟的历史对象视图：描述死亡或销毁时的数据。
 * 从 structures-spawn.js 按领域拆出（源码行数门）；依赖同段装配的 gw 机制。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  const __Object = gw.__Object;
  // 复用 structures.js 提供的常量与 Store（CARRY_CAPACITY、makeStore）。
  const C = gw.__structuresC;

  // 装配墓碑与废墟的历史对象视图；它们描述死亡或销毁时的数据。
  // 死亡 creep 代理继承 Creep.prototype，但属性由专门 getter 提供，不回查已消亡的 creep id。
  function tombstoneCreepProxy(o) {
    const proxy = {};
    const carryCap = (o.creepBody || []).reduce((n, t) => n + (t === 'carry' ? C.CARRY_CAPACITY : 0), 0);
    // 复用构造时的位置实例，连续读取 pos 保持同一引用。
    const posInstance = new gw.RoomPosition(o.x, o.y, o.room);
    const defs = {
      id: { enumerable: true, get: () => o.creepId },
      name: { enumerable: true, get: () => o.creepName },
      // 死亡 creep 视图也提供位置和房间引用；来源：上游 engine/src/game/tombstones.js。
      pos: { enumerable: true, get: () => posInstance },
      room: { enumerable: true, get: () => (gw.currentGame() ? gw.currentGame().rooms[o.room] : undefined) },
      spawning: { enumerable: true, get: () => false },
      my: { enumerable: true, get: () => o.user === gw.viewerName },
      body: { enumerable: true, get: () => (o.creepBody || []).map((t) => ({ type: t, hits: 0 })) },
      owner: { enumerable: true, get: () => (o.user === undefined || o.user === null ? undefined : { username: o.user }) },
      ticksToLive: { enumerable: true, get: () => o.creepTicksToLive },
      carryCapacity: { enumerable: true, get: () => carryCap },
      // 死亡 creep 的 Store 恒空，容量按其原 body 计算；可提取库存属于墓碑自身的 store。
      // 来源：上游 engine/src/game/tombstones.js 的 carry/store getter。
      carry: { enumerable: true, get: () => gw.makeStore({ store: {}, storeCapacity: carryCap }) },
      store: { enumerable: true, get: () => gw.makeStore({ store: {}, storeCapacity: carryCap }) },
      fatigue: { enumerable: true, get: () => 0 },
      hits: { enumerable: true, get: () => 0 },
      hitsMax: { enumerable: true, get: () => (o.creepBody || []).length * 100 },
      saying: { enumerable: true, get: () => o.creepSaying },
    };
    for (const k of __Object.keys(defs)) __Object.defineProperty(proxy, k, defs[k]);
    __Object.setPrototypeOf(proxy, globalThis.Creep.prototype);
    return proxy;
  }

  gw.defineGameObjectProperties(globalThis.Tombstone.prototype, gw.rawData, {
    deathTime: (o) => o.deathTime,
    ticksToDecay: (o) => o.ticksToDecay, // 投影已折叠（官方 getter 是 decayTime−time 的等价值）
    store: (o) => gw.makeStore(o),
    // carry/carryCapacity 属于死亡 creep 代理，不能添加到 Tombstone 自身，否则改变枚举结果。
    creep: (o) => (o.creepId ? tombstoneCreepProxy(o) : undefined),
  });

  // structure 代理对象（ruins.js:43-68）：user 有 → OwnedStructure 形（owner/my
  // getter），无 → Structure 形。structureType 读存照 type 键（官方 getter 写
  // o.structureType 但 DB 键在 structure.type——vanilla snapshots.ts:394 同构）
  function ruinStructureProxy(o) {
    const st = o.structure || {};
    const proxy = {};
    const defs = {
      id: { enumerable: true, get: () => st.id },
      hits: { enumerable: true, get: () => st.hits },
      hitsMax: { enumerable: true, get: () => st.hitsMax },
      structureType: { enumerable: true, get: () => st.type },
    };
    if (st.user) {
      defs.owner = { enumerable: true, get: () => ({ username: st.user }) };
      defs.my = { enumerable: true, get: () => st.user === gw.viewerName };
    }
    for (const k of __Object.keys(defs)) __Object.defineProperty(proxy, k, defs[k]);
    __Object.setPrototypeOf(proxy, st.user ? globalThis.OwnedStructure.prototype : globalThis.Structure.prototype);
    return proxy;
  }

  gw.defineGameObjectProperties(globalThis.Ruin.prototype, gw.rawData, {
    structureType: (o) => (o.structure && o.structure.type),
    destroyTime: (o) => o.destroyTime,
    ticksToDecay: (o) => o.ticksToDecay, // 投影已折叠
    store: (o) => gw.makeStore(o),
    structure: (o) => ruinStructureProxy(o),
  });

  // 字段全在类原型——装配只透传壳
  gw.assemblers.tombstone = (o, base) => base;
  gw.assemblers.ruin = (o, base) => base;
})();
