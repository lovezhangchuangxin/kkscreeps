/* 结构读面：私有通用字段表、类型选择、专用时间/资源字段与 Mod 建筑字段装配。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  // 其余现状暴露 store/energy/energyCapacity 的结构（road/wall 与未列类型
  // 兜底——官方 road/wall 无这些 getter，现状暴露面保持不变）；store 同样
  // 吃整个 raw 投影（Generic 透传桶里带 storeCapacityResource 的结构——
  // lab/tower/link 等——经此获得官方三函数族语义）。
  //
  // 本兜底是**实例级** own 属性，会遮蔽上方各 StructureXxx.prototype 的同名
  // getter（lab/powerSpawn/nuker/link/tower 的 energyCapacity 都在这条路上）。
  // 故 energyCapacity 必须照官方口径读 `storeCapacityResource`（structures.js
  // :305 lab / :483 link / :760 tower / :605 powerSpawn / :1349 nuker 同一式
  // `storeCapacityResource ? .energy || 0 : 0`）——**不能读原始键
  // `o.energyCapacity`**：官方 DB 从无该键（能量容量在 storeCapacityResource
  // 里），读它恒 0，而 P4 已声明 powerSpawn/lab 等 energyCapacity 转真。
  // 与 storeCapacity 的平面键不同，官方对 energyCapacity 无平面回退分支，
  // 故不补 `|| o.storeCapacity`（避免把官方的 0 抬成数值）。
  const ELSE_STRUCTURE_FIELDS = {
    energy: (o) => (o.store && o.store.energy) || 0,
    energyCapacity: (o) => (o.storeCapacityResource && o.storeCapacityResource.energy) || 0,
    store: (o) => gw.makeStore(o),
  };

  function installCoreFields() {
    // StructureExtension（官方 structures.js:253-255）
    gw.defineGameObjectProperties(globalThis.StructureExtension.prototype, gw.rawData, {
      energy: (o) => (o.store && o.store.energy) || 0,
      energyCapacity: (o) => (o.storeCapacityResource && o.storeCapacityResource.energy) || 0,
      store: (o) => gw.makeStore(o),
    });

    // StructureStorage（官方 structures.js:690-693）：store + storeCapacity
    gw.defineGameObjectProperties(globalThis.StructureStorage.prototype, gw.rawData, {
      store: (o) => gw.makeStore(o),
      storeCapacity: (o) => o.storeCapacity,
    });

    gw.defineGameObjectProperties(globalThis.StructureRoad.prototype, gw.rawData, ELSE_STRUCTURE_FIELDS);
    gw.defineGameObjectProperties(globalThis.StructureWall.prototype, gw.rawData, ELSE_STRUCTURE_FIELDS);
    // road 优先以 nextDecayTime 计算剩余 tick，再用 decayTime。
    // wall 不添加 ticksToDecay，避免多出上游不存在的属性。
    // 来源：上游 engine/src/game/structures.js 的 StructureRoad/StructureWall 属性定义。
    gw.defineGameObjectProperties(globalThis.StructureRoad.prototype, gw.rawData, {
      ticksToDecay: (o) => (o.nextDecayTime ? o.nextDecayTime - gw.world.time
        : o.decayTime ? o.decayTime - gw.world.time : undefined),
    });
  }

  function installAssembler() {
    gw.assemblers.structure = (o, base) => {
      base.isStructure = true;
      if (o.structureType === 'controller' || o.structureType === 'source' || o.structureType === 'mineral') {
        return base; // 字段/方法全在各自类原型
      }
      // extension/storage 有官方字段表（上方已挂类原型）；road/wall 与未来新
      // 结构类型兜底实例级 getter（同一回查机制）
      const cls = { extension: 'StructureExtension', storage: 'StructureStorage',
        road: 'StructureRoad', constructedWall: 'StructureWall', wall: 'StructureWall' }[o.structureType];
      if (!cls) {
        gw.defineGameObjectProperties(base, gw.rawData, ELSE_STRUCTURE_FIELDS);
        // Mod 建筑的自定义字段（存储键含命名空间冒号）以同一回查机制
        // 直供玩家；记录缺键（Missing 默认未写）读到 undefined。
        const entry = gw.modApi !== undefined && gw.modApi.buildings !== undefined
          ? gw.modApi.buildings[o.structureType] : undefined;
        if (entry !== undefined && entry.fields !== undefined && entry.fields.length > 0) {
          const props = {};
          for (let i = 0; i < entry.fields.length; i++) {
            const key = entry.fields[i];
            props[key] = (raw) => raw[key];
          }
          gw.defineGameObjectProperties(base, gw.rawData, props);
        }
      }
      return base;
    };
  }

  function installSpecialFields() {
    // harvest 的提取器预检读取 cooldown；缺失时按 0 处理。
    // 来源：上游 engine/src/game/structures.js 的 StructureExtractor 属性定义。
    gw.defineGameObjectProperties(globalThis.StructureExtractor.prototype, gw.rawData, {
      cooldown: (o) => o.cooldown || 0,
    });

    // 将 isPublic 转成布尔值，供 withdraw 判断敌方 rampart 是否公开。
    // 来源：上游 engine/src/game/structures.js 的 StructureRampart 属性定义。
    gw.defineGameObjectProperties(globalThis.StructureRampart.prototype, gw.rawData, {
      isPublic: (o) => !!o.isPublic,
    });

    // 衰减期限与 cooldownTime 是绝对游戏 tick，getter 转为剩余 tick。
    // 衰减优先 nextDecayTime，再用 decayTime；冷却已结束时返回 0。
    const C_LAB_MINERAL_CAPACITY = globalThis.LAB_MINERAL_CAPACITY;
    function decayFold(o) {
      const t = gw.world.time;
      return o.nextDecayTime ? o.nextDecayTime - t
        : o.decayTime ? o.decayTime - t : undefined;
    }
    function cooldownFold(o) {
      const t = gw.world.time;
      return o.cooldownTime && o.cooldownTime > t ? o.cooldownTime - t : 0;
    }
    // StructureContainer（官方 :146-158）
    gw.defineGameObjectProperties(globalThis.StructureContainer.prototype, gw.rawData, {
      store: (o) => gw.makeStore(o),
      storeCapacity: (o) => o.storeCapacity,
      ticksToDecay: decayFold,
    });
    // StructureRampart 衰减钟（官方 :318-334 tick 族——读取面同容器形态）
    gw.defineGameObjectProperties(globalThis.StructureRampart.prototype, gw.rawData, {
      ticksToDecay: decayFold,
    });
    // StructureLink（官方 structures.js:481-487）——link 用**每 tick 递减计数器
    // `o.cooldown` 直读**（官方 getter `(o) => o.cooldown || 0`，由
    // links/tick.js:11-22 逐 tick 递减），与 lab/terminal/nuker/factory 的
    // `cooldownTime` 折算形态**不同**（vanilla index.ts:1145-1158 明文两形态）
    gw.defineGameObjectProperties(globalThis.StructureLink.prototype, gw.rawData, {
      cooldown: (o) => o.cooldown || 0,
    });
    // StructureTerminal（官方 :709-713）
    gw.defineGameObjectProperties(globalThis.StructureTerminal.prototype, gw.rawData, {
      store: (o) => gw.makeStore(o),
      storeCapacity: (o) => o.storeCapacity,
      cooldown: cooldownFold,
    });
    // StructureLab（官方 :303-313——mineral 双 getter 官方 :304-305 lodash 式
    // 的循环直译：amount=Σstore−energy、type=首个非 energy 正量键）
    gw.defineGameObjectProperties(globalThis.StructureLab.prototype, gw.rawData, {
      energy: (o) => (o.store ? o.store.energy : 0),
      energyCapacity: (o) => (o.storeCapacityResource ? o.storeCapacityResource.energy : 0),
      cooldown: cooldownFold,
      mineralAmount: (o) => {
        let sum = 0;
        const store = o.store || {};
        for (const k in store) {
          if (k !== 'energy') sum += store[k] || 0;
        }
        return sum;
      },
      mineralCapacity: () => C_LAB_MINERAL_CAPACITY,
      mineralType: (o) => {
        const store = o.store || {};
        for (const k in store) {
          if (k !== 'energy' && store[k]) return k;
        }
        return undefined;
      },
      store: (o) => gw.makeStore(o),
    });
    // StructurePowerSpawn（官方 :715-721）
    gw.defineGameObjectProperties(globalThis.StructurePowerSpawn.prototype, gw.rawData, {
      energy: (o) => (o.store ? o.store.energy || 0 : 0),
      energyCapacity: (o) => (o.storeCapacityResource ? o.storeCapacityResource.energy : 0),
      power: (o) => (o.store ? o.store.power || 0 : 0),
      powerCapacity: (o) => (o.storeCapacityResource ? o.storeCapacityResource.power : 0),
      store: (o) => gw.makeStore(o),
    });
    // StructureNuker（官方 :723-731）
    gw.defineGameObjectProperties(globalThis.StructureNuker.prototype, gw.rawData, {
      energy: (o) => (o.store ? o.store.energy || 0 : 0),
      energyCapacity: (o) => (o.storeCapacityResource ? o.storeCapacityResource.energy : 0),
      ghodium: (o) => (o.store ? o.store.G || 0 : 0),
      ghodiumCapacity: (o) => (o.storeCapacityResource ? o.storeCapacityResource.G : 0),
      cooldown: cooldownFold,
      store: (o) => gw.makeStore(o),
    });
    // StructureFactory（官方 :751-756）
    gw.defineGameObjectProperties(globalThis.StructureFactory.prototype, gw.rawData, {
      level: (o) => o.level,
      store: (o) => gw.makeStore(o),
      storeCapacity: (o) => o.storeCapacity,
      cooldown: cooldownFold,
    });
    // StructurePortal（官方 :733-749——destination 跨 shard 形态给
    // {shard, room}，同 shard 给 RoomPosition 实例；衰减只读 decayTime）
    gw.defineGameObjectProperties(globalThis.StructurePortal.prototype, gw.rawData, {
      destination: (o) => {
        if (o.destination && o.destination.shard) {
          return { shard: o.destination.shard, room: o.destination.room };
        }
        return o.destination
          ? new gw.RoomPosition(o.destination.x, o.destination.y, o.destination.room)
          : undefined;
      },
      ticksToDecay: (o) => (o.decayTime ? o.decayTime - gw.world.time : undefined),
    });
    // StructureKeeperLair（官方 :162-166——NPC 常量归属面）
    gw.defineGameObjectProperties(globalThis.StructureKeeperLair.prototype, gw.rawData, {
      my: () => false,
      owner: () => ({ username: 'Source Keeper' }),
      ticksToSpawn: (o) => (o.nextSpawnTime ? o.nextSpawnTime - gw.world.time : undefined),
    });
    // StructurePowerBank（官方 :300-306）
    gw.defineGameObjectProperties(globalThis.StructurePowerBank.prototype, gw.rawData, {
      power: (o) => (o.store ? o.store.power : 0),
      ticksToDecay: decayFold,
      my: () => false,
      owner: () => ({ username: 'Power Bank' }),
    });
    // StructureInvaderCore（官方 :758-762——spawning 复用 Spawning 类经 gw 桥）
    gw.defineGameObjectProperties(globalThis.StructureInvaderCore.prototype, gw.rawData, {
      level: (o) => o.level,
      spawning: (o, id) => (o.spawning ? new gw.Spawning(id) : null),
      ticksToDeploy: (o) => (o.deployTime ? o.deployTime - gw.world.time : undefined),
    });
    // Resource.energy（官方 resources.js:36——energy 堆的 amount 别名键；
    // 非 energy 资源投影无 energy 键=undefined 官方同形）
    gw.defineGameObjectProperties(globalThis.Resource.prototype, gw.rawData, {
      energy: (o) => o.energy,
    });
  }

  // 入口加载时捕获安装函数；字段表与类型选择不暴露为可修改的共享值。
  globalThis.__kkp.structureFields = {
    installCoreFields, installAssembler, installSpecialFields,
  };
})();
