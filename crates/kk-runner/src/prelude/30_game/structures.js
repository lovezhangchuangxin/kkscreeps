/* kk-runner prelude · 30_game 段 5：Structure 族装配（对照官方
 * engine/src/game/structures.js + sources.js + minerals.js + store.js）。
 *
 * A4-i Store 族 + spawn 方法族换真：
 *   · Store 类（官方 store.js:19-101 全语义）：构造挂 truthy 资源键（0 值
 *     不出现）、Proxy 的 RESOURCES_ALL 缺省 0、getCapacity/getUsedCapacity/
 *     getFreeCapacity 三函数族（capacityForResource，utils.js:671-675）、
 *     toString '[store]'；
 *   · StructureSpawn.Spawning（官方 structures.js:1301-1334）：name/needTime/
 *     remainingTime/directions own 定格 + 非 enumerable spawn 引用 +
 *     setDirections/cancel；
 *   · spawnCreep（:1063-1218 全预检序 + 同 tick 幽灵壳）/renewCreep
 *     （:1237-1267）/recycleCreep（:1269-1290）。
 * destroy/setActive/notifyWhenAttacked 保持桩（A4-iii/后续批次换真）。
 *
 * A3b 纪律：字段表挂各类原型（官方 defineGameObjectProperties 挂 prototype
 * 的同构）；getter 经 gw.rawData(this.id) 回查当前 tick 投影；方法体官方
 * this.xxx 走 this（getter 通道）、官方 data(this.id).xxx 走 gw.rawData
 * 显式回查；玩家 tick 内执行路径不得经可交换全局（Object/Proxy/Math/
 * 常量面板全部预置捕获——官方 game 层的 C 来自 driver.constants 注入对象，
 * 玩家覆盖全局常量不影响 API 判定，game.js 头部 C = driver.constants 同款）。
 * 依赖：gw.rawData/gw.rawById/gw.objectsById/gw.viewerName/gw.currentGame/
 * gw.world/gw.intents/gw.ensureMemory/gw.RoomPosition/gw.mkStubMethod/
 * gw.defineGameObjectProperties/gw.__Object/gw.perTickReset、__kkp.lodash。
 */
(() => {
  'use strict';

  const { reportMissing, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;
  // 预置 lodash（00_shared 装配的同一实例，闭包引用——玩家重绑 globalThis._
  // 不影响本段；官方 API 闭包 require('lodash') 的同构）
  const _ = globalThis.__kkp.lodash;

  // ---------------------------------------------------------------- 预置捕获
  const __Proxy = Proxy;
  const __Object = gw.__Object;
  const __MathCeil = Math.ceil;
  const __MathFloor = Math.floor;
  const __MathMax = Math.max;

  function freezeConst(obj) {
    if (obj && typeof obj === 'object') {
      Object.freeze(obj);
      for (const k of Object.keys(obj)) freezeConst(obj[k]);
    }
    return obj;
  }

  const C = {
    OK: globalThis.OK,
    ERR_INVALID_ARGS: globalThis.ERR_INVALID_ARGS,
    ERR_NAME_EXISTS: globalThis.ERR_NAME_EXISTS,
    ERR_NOT_OWNER: globalThis.ERR_NOT_OWNER,
    ERR_BUSY: globalThis.ERR_BUSY,
    ERR_RCL_NOT_ENOUGH: globalThis.ERR_RCL_NOT_ENOUGH,
    ERR_NOT_ENOUGH_ENERGY: globalThis.ERR_NOT_ENOUGH_ENERGY,
    ERR_INVALID_TARGET: globalThis.ERR_INVALID_TARGET,
    ERR_NOT_IN_RANGE: globalThis.ERR_NOT_IN_RANGE,
    ERR_FULL: globalThis.ERR_FULL,
    MAX_CREEP_SIZE: globalThis.MAX_CREEP_SIZE,
    CREEP_LIFE_TIME: globalThis.CREEP_LIFE_TIME,
    CREEP_SPAWN_TIME: globalThis.CREEP_SPAWN_TIME,
    SPAWN_RENEW_RATIO: globalThis.SPAWN_RENEW_RATIO,
    CARRY_CAPACITY: globalThis.CARRY_CAPACITY,
    CARRY: globalThis.CARRY,
    CLAIM: globalThis.CLAIM,
    // 私有冻结副本（数组 slice / 对象浅拷）——玩家改写全局常量面板不影响判定
    BODYPARTS_ALL: freezeConst(globalThis.BODYPARTS_ALL.slice()),
    BODYPART_COST: freezeConst(__Object.assign({}, globalThis.BODYPART_COST)),
    RESOURCES_ALL: freezeConst(globalThis.RESOURCES_ALL.slice()),
    // controller 族（A4-iii）：activateSafeMode 门③阈值表与第六门扫描
    ERR_NOT_ENOUGH_RESOURCES: globalThis.ERR_NOT_ENOUGH_RESOURCES,
    ERR_TIRED: globalThis.ERR_TIRED,
    ERR_BUSY: globalThis.ERR_BUSY,
    CONTROLLER_DOWNGRADE: freezeConst(globalThis.CONTROLLER_DOWNGRADE),
    CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD: globalThis.CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD,
  };

  // ---------------------------------------------------------------- per-tick 状态
  // 官方 structures.js:7/:27 的模块级闭包 createdCreepNames——make 每 tick
  // 重置（同 tick 同名拒、跨 tick 由 Game.creeps 接管）。等价物：gw.perTickReset
  // 钩子由 game.js beginTick 调用（官方 game/structures.js make 的重置位）。
  let createdCreepNames = [];
  const deprecatedShown = [];
  (gw.perTickReset = gw.perTickReset || []).push(() => {
    createdCreepNames = [];
    deprecatedShown.length = 0;
  });

  // 官方 register.deprecated（game.js:111-117）：每 tick 每条消息只 console.log
  // 一次（deprecatedShown 同 tick 去重）。console 装载期捕获（评审 1 P3-1：
  // 玩家重绑 globalThis.console 不截获官方警告文本——官方经注入的
  // globals.console 闭包同构）
  const __console = globalThis.console;
  function deprecated(msg) {
    if (!_.contains(deprecatedShown, msg)) {
      deprecatedShown.push(msg);
      __console.log(msg);
    }
  }

  // 官方 register.assertTargetObject（game.js:122-126）：Memory 序列化壳拦截
  // （isPlainObject && isString(id) && id.length == 24 → throw）。24 判按官方
  // 原样保留——我方 id 是 18 字符 kk 前缀（自身格式的序列化壳走 INVALID_TARGET
  // 分支），从 vanilla 迁移的 bot 其 Memory 残留 24 字符 Mongo id 同样命中。
  function assertTargetObject(obj) {
    if (obj && _.isPlainObject(obj) && _.isString(obj.id) && obj.id.length === 24) {
      throw new Error("It seems you're trying to use a serialized game object stored in Memory which is not allowed. Please use `Game.getObjectById` to retrieve a live object reference instead.");
    }
  }

  /** 官方 register.creeps[target.id] 判定的等价物：raw 投影里存在且是 creep
   *  （官方 register.creeps 收全部可见 creep，不分敌我）。 */
  function isKnownCreep(id) {
    const o = id == null ? undefined : gw.rawById.get(id);
    return !!o && o.type === 'creep';
  }

  // ---------------------------------------------------------------- Store 类
  // 官方 store.js:19-101 逐行对照（A4-i 换真——原垫片 getUsedCapacity 只认
  // energy/getFreeCapacity 恒 0/getCapacity 恒 null）。

  /** _.sum(object)（官方 store.js 的 _.sum(object.store) / _.sum(storeCapacityResource)）：
   *  自有可枚举值求和；undefined 归 0。 */
  function sumValues(o) {
    if (!o) return 0;
    let sum = 0;
    for (const v of __Object.values(o)) sum += v;
    return sum;
  }

  /** 官方 utils.capacityForResource（utils.js:671-675）：
   *  `storeCapacityResource && storeCapacityResource[resourceType] ||
   *   Math.max(0, (storeCapacity||0) - _.sum(storeCapacityResource))`。 */
  function capacityForResource(object, resourceType) {
    return (
      (object.storeCapacityResource && object.storeCapacityResource[resourceType]) ||
      __MathMax(0, (object.storeCapacity || 0) - sumValues(object.storeCapacityResource))
    );
  }

  /** 官方 Store 构造器（store.js:19-97）：truthy 资源键挂 own（0 值不出现），
   *  三函数族 + toString 以非 enumerable 的 value 属性定义，返回包 this 的
   *  Proxy——RESOURCES_ALL 资源名缺省读 0（而非 undefined），其余未知名
   *  undefined。每次 .store 访问新建实例（官方 _storeGetter），_sum 惰性缓存
   *  （defineProperty value，非 writable）是实例内的。 */
  function Store(object) {
    const storeData = object.store || {};
    for (const [resourceType, resourceAmount] of __Object.entries(storeData)) {
      if (resourceAmount) {
        this[resourceType] = resourceAmount;
      }
    }
    __Object.defineProperties(this, {
      getCapacity: {
        value: function getCapacity(resource) {
          if (!resource) {
            return object.storeCapacityResource ? null : object.storeCapacity || null;
          }
          return capacityForResource(object, resource) || null;
        },
      },
      getUsedCapacity: {
        value: function getUsedCapacity(resource) {
          if (!resource) {
            if (!!object.storeCapacityResource && capacityForResource(object, resource) === 0) {
              return null;
            }
            if (this._sum === undefined) {
              __Object.defineProperty(this, '_sum', { value: sumValues(storeData) });
            }
            return this._sum;
          }
          return storeData[resource] || (!!object.storeCapacityResource && capacityForResource(object, resource) === 0 ? null : 0);
        },
      },
      getFreeCapacity: {
        value: function getFreeCapacity(resource) {
          if (capacityForResource(object, resource) === 0) {
            return null;
          }
          if (!object.storeCapacity) {
            return this.getCapacity(resource) - this.getUsedCapacity(resource);
          }
          const capacity = this.getCapacity(resource);
          if (!capacity) {
            return null;
          }
          if (object.storeCapacityResource) {
            return capacity - this.getUsedCapacity(resource);
          }
          if (this._sum === undefined) {
            __Object.defineProperty(this, '_sum', { value: sumValues(storeData) });
          }
          return capacity - this._sum;
        },
      },
      toString: {
        value: function toString() {
          return '[store]';
        },
      },
    });
    return new __Proxy(this, {
      get(target, name) {
        if (target[name] !== undefined) {
          return target[name];
        }
        if (typeof name === 'string' && C.RESOURCES_ALL.indexOf(name) !== -1) {
          return 0;
        }
        // 仪表：非资源的未知属性访问登记（返回值保持官方 undefined）
        if (typeof name === 'string') reportMissing('store.' + name);
      },
    });
  }
  gw.Store = Store;
  // 官方 Object.defineProperty(globals, 'Store', ...)（store.js:99）——替换
  // _registry 段的 M0 桩类（instanceof Store / new Store({...}) 对玩家可用）
  globalThis.Store = Store;

  /** 官方 _storeGetter（structures.js:62-64 / creeps.js:37-39）：吃整个 raw
   *  投影对象 o（store/storeCapacity/storeCapacityResource 三键的消费者）。 */
  function makeStore(object) {
    return new Store(object);
  }
  gw.makeStore = makeStore;

  // ---------------------------------------------------------------- 官方工具
  /** 官方 utils.calcCreepCost（utils.js:114-123）：BODYPART_COST 求和，
   *  白名单外部件 throw（spawnCreep 已过 BODYPARTS_ALL 预检，不可达）。 */
  function calcCreepCost(body) {
    let result = 0;
    for (const part of body) {
      const partType = _.isObject(part) ? part.type : part;
      if (!C.BODYPART_COST[partType]) {
        throw new Error('Invalid body part ' + partType);
      }
      result += C.BODYPART_COST[partType];
    }
    return result;
  }

  /** 官方 calcEnergyAvailable（structures.js:1053-1061）：显式 energyStructures
   *  的可用量。raw 投影直查——**无 owner 过滤**（官方 quirk 原样：API 层不查
   *  归属，结算层 _charge-energy 才按 owner 过滤）。官方 roomObjects 的
   *  extension 是独立 type；我方投影归一为 type:'structure' +
   *  structureType:'extension'（world_json 的形态）——两形都认。 */
  function calcEnergyAvailable(energyStructures) {
    let sum = 0;
    for (const id of energyStructures) {
      const o = id == null ? undefined : gw.rawById.get(id);
      const kind =
        o && (o.type === 'spawn' || o.type === 'extension'
          || (o.type === 'structure' && o.structureType === 'extension'));
      if (kind && !o.off && o.store) {
        sum += o.store.energy;
      }
    }
    return sum;
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

  // Controller（官方 structures.js:139-174）。A4-iii 投影清偿：reservation/
  // sign/upgradeBlocked/safeModeCooldown 四 getter 换官方真形态（绝对 tick
  // 挂钟 → `> time ? 差值 : undefined`；{user,endTime} → {username,
  // ticksToEnd}——官方 :143-146/:151/:157-169）。ticksToDowngrade/safeMode
  // 维持既有 None→0 口径（灰区表登记，A3b 只换访问机制）——注意
  // activateSafeMode 门③ 读 this.ticksToDowngrade 时无钟房 0 < 阈值为真
  // （官方 undefined < 阈值为假——API 返回码差异，登记报告；终态一致：结算
  // 侧 B2 对无钟房同拒）。
  gw.defineGameObjectProperties(globalThis.StructureController.prototype, gw.rawData, {
    level: (o) => o.level || 0,
    progress: (o) => o.progress || 0,
    progressTotal: (o) => o.progressTotal || 0,
    ticksToDowngrade: (o) => (o.downgradeTime != null ? o.downgradeTime - now() : 0),
    safeMode: (o) => (o.safeMode && o.safeMode > now() ? o.safeMode - now() : 0),
    safeModeAvailable: (o) => o.safeModeAvailable ?? 0,
    safeModeCooldown: (o) => (o.safeModeCooldown && o.safeModeCooldown > now()
      ? o.safeModeCooldown - now()
      : undefined),
    isPowerEnabled: (o) => o.isPowerEnabled === true,
    reservation: (o) => (o.reservation
      ? { username: o.reservation.user, ticksToEnd: o.reservation.endTime - now() }
      : undefined),
    sign: (o) => (o.sign
      ? {
          username: o.sign.user,
          text: o.sign.text,
          time: o.sign.time,
          // 官方第四字段 datetime: new Date(o.sign.datetime)（structures.js:163，
          // 墙钟落库）——确定性引擎不落盘，合成当前墙钟（灰区表登记口径：
          // 合成或差异登记，replay 不确定性与官方 Date.now() 同源）
          datetime: new Date(),
        }
      : undefined),
    upgradeBlocked: (o) => (o.upgradeBlocked && o.upgradeBlocked > now()
      ? o.upgradeBlocked - now()
      : undefined),
    activationDate: () => undefined,
  });

  // 当前 tick（getter 体内换算用——官方 getter 读 runtimeData.time 同款）
  function now() {
    return gw.world.time;
  }

  // ---------------------------------------------------------------- Spawning 类
  // 官方 StructureSpawn.Spawning（structures.js:1301-1310）：own 定格
  // name/needTime/remainingTime/directions + 非 enumerable 的 spawn 引用
  // （官方 register._objects[spawnId]——我方经 objectsById 取当前壳）。
  function Spawning(spawnId) {
    const o = gw.rawData(spawnId); // 官方 data(spawnId)：消亡抛官方同文案
    __Object.defineProperty(this, 'spawn', {
      enumerable: false,
      value: gw.objectsById.get(spawnId),
    });
    this.name = o.spawning.name;
    this.needTime = o.spawning.needTime;
    this.remainingTime = o.spawning.spawnTime - gw.world.time;
    this.directions = o.spawning.directions;
  }
  globalThis.StructureSpawn.Spawning = Spawning;

  /** 官方 setDirections（structures.js:1312-1326）：数组数字化去重后任一越界/
   *  非整 → INVALID_ARGS（_.any 反向判定——与 spawnCreep 的 _.all 形态互补）。 */
  Spawning.prototype.setDirections = function setDirections(directions) {
    if (!this.spawn.my) {
      return C.ERR_NOT_OWNER;
    }
    if (_.isArray(directions) && directions.length > 0) {
      // convert directions to numbers, eliminate duplicates
      directions = _.uniq(_.map(directions, (e) => +e));
      // bail if any numbers are out of bounds or non-integers
      if (!_.any(directions, (direction) => direction < 1 || direction > 8 || direction !== (direction | 0))) {
        gw.intents.set(this.spawn.id, 'setSpawnDirections', { directions });
        return C.OK;
      }
    }
    return C.ERR_INVALID_ARGS;
  };

  /** 官方 cancel（structures.js:1328-1334）。 */
  Spawning.prototype.cancel = function cancel() {
    if (!this.spawn.my) {
      return C.ERR_NOT_OWNER;
    }
    gw.intents.set(this.spawn.id, 'cancelSpawning', {});
    return C.OK;
  };

  // ---------------------------------------------------------------- StructureSpawn 字段表
  // 官方 structures.js:857-863 对照（energyCapacity 官方读 storeCapacityResource
  // .energy；spawning 官方 :861 是 Spawning 实例；store 走 _storeGetter）。
  // structureType 子类覆盖：spawn 投影的 type 键即结构类型（手写世界 JSON
  // 可无 structureType 键——保持现状值拷贝的 'spawn' 缺省）
  gw.defineGameObjectProperties(globalThis.StructureSpawn.prototype, gw.rawData, {
    structureType: (o) => o.structureType || 'spawn',
    name: (o) => o.name,
    energy: (o) => (o.store && o.store.energy) || 0,
    energyCapacity: (o) => (o.storeCapacityResource && o.storeCapacityResource.energy) || 0,
    spawning: (o, id) => (o.spawning ? new Spawning(id) : null),
    store: (o) => gw.makeStore(o),
  });

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

  // 其余现状暴露 store/energy/energyCapacity 的结构（road/wall 与未列类型
  // 兜底——官方 road/wall 无这些 getter，现状暴露面保持不变）；store 同样
  // 吃整个 raw 投影（Generic 透传桶里带 storeCapacityResource 的结构——
  // lab/tower/link 等——经此获得官方三函数族语义）
  const ELSE_STRUCTURE_FIELDS = {
    energy: (o) => (o.store && o.store.energy) || 0,
    energyCapacity: (o) => o.energyCapacity || 0,
    store: (o) => gw.makeStore(o),
  };
  for (const cls of ['StructureRoad', 'StructureWall']) {
    gw.defineGameObjectProperties(globalThis[cls].prototype, gw.rawData, ELSE_STRUCTURE_FIELDS);
  }

  // ---------------------------------------------------------------- spawn 方法族
  /** 官方 spawnCreep（structures.js:1063-1218）全预检序：
   *  ①!name||!isObject(options)→INVALID_ARGS ②name.length>100→INVALID_ARGS
   *  ③同名（Game.creeps 或同 tick createdCreepNames）→NAME_EXISTS
   *  ④energyStructures pluck+uniq ⑤directions 数组化校验 ⑥!my→NOT_OWNER
   *  ⑦spawning（显式回查）→BUSY ⑧off（显式回查）→RCL ⑨body 校验
   *  ⑩energyAvailable<cost→NOT_ENOUGH_ENERGY ⑪dryRun→OK ⑫名单+Memory
   *  ⑬幽灵壳 ⑭intent ⑮OK。 */
  globalThis.StructureSpawn.prototype.spawnCreep = function spawnCreep(body, name, options) {
    if (options === undefined) options = {};
    if (!name || !_.isObject(options)) {
      return C.ERR_INVALID_ARGS;
    }
    name = name.toString();
    if (name.length > 100) {
      return C.ERR_INVALID_ARGS;
    }
    if (gw.currentGame().creeps[name] || createdCreepNames.indexOf(name) !== -1) {
      return C.ERR_NAME_EXISTS;
    }
    let energyStructures = options.energyStructures && _.uniq(_.map(options.energyStructures, 'id'));
    let directions = options.directions;
    if (directions !== undefined) {
      if (!_.isArray(directions)) {
        return C.ERR_INVALID_ARGS;
      }
      // convert directions to numbers, eliminate duplicates
      directions = _.uniq(_.map(directions, (d) => +d));
      // bail if any numbers are out of bounds or non-integers
      if (directions.length === 0 || !_.all(directions, (direction) => direction >= 1 && direction <= 8 && direction === (direction | 0))) {
        return C.ERR_INVALID_ARGS;
      }
    }
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (gw.rawData(this.id).spawning) {
      return C.ERR_BUSY;
    }
    if (gw.rawData(this.id).off) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (!body || !_.isArray(body) || body.length === 0 || body.length > C.MAX_CREEP_SIZE) {
      return C.ERR_INVALID_ARGS;
    }
    for (let i = 0; i < body.length; i++) {
      if (!_.contains(C.BODYPARTS_ALL, body[i])) {
        return C.ERR_INVALID_ARGS;
      }
    }
    const energyAvailable = energyStructures ? calcEnergyAvailable(energyStructures) : this.room.energyAvailable;
    if (energyAvailable < calcCreepCost(body)) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    if (options.dryRun) {
      return C.OK;
    }
    createdCreepNames.push(name);
    const memory = gw.ensureMemory();
    if (memory.creeps === undefined) {
      memory.creeps = {};
    }
    if (_.isObject(memory.creeps)) {
      memory.creeps[name] = options.memory || memory.creeps[name] || {};
    }
    // 同 tick 幽灵壳（官方 :1132-1213）：new Creep()（无 id）+ RoomObject.call
    // （room/pos）+ 十三字段 enumerable getter——官方逐字段对照（name/spawning:
    // true/my:true/body:{type,hits:100}/owner/ticksToLive:CREEP_LIFE_TIME/
    // carryCapacity/carry:{energy:0}/store:new Store({store:{energy:0}})/
    // fatigue:0/hits/hitsMax/saying:undefined）
    const ghost = __Object.create(globalThis.Creep.prototype);
    ghost.room = this.room;
    ghost.pos = new gw.RoomPosition(this.pos.x, this.pos.y, this.pos.roomName);
    __Object.defineProperties(ghost, {
      name: { enumerable: true, get() { return name; } },
      spawning: { enumerable: true, get() { return true; } },
      my: { enumerable: true, get() { return true; } },
      body: {
        enumerable: true,
        get() {
          return _.map(body, (type) => ({ type, hits: 100 }));
        },
      },
      owner: { enumerable: true, get() { return { username: gw.viewerName }; } },
      ticksToLive: { enumerable: true, get() { return C.CREEP_LIFE_TIME; } },
      carryCapacity: {
        enumerable: true,
        get() {
          return _.reduce(body, (result, type) => (result += type === C.CARRY ? C.CARRY_CAPACITY : 0), 0);
        },
      },
      carry: { enumerable: true, get() { return { energy: 0 }; } },
      store: { enumerable: true, get() { return new Store({ store: { energy: 0 } }); } },
      fatigue: { enumerable: true, get() { return 0; } },
      hits: { enumerable: true, get() { return body.length * 100; } },
      hitsMax: { enumerable: true, get() { return body.length * 100; } },
      saying: { enumerable: true, get() { return undefined; } },
    });
    gw.currentGame().creeps[name] = ghost;
    gw.intents.set(this.id, 'createCreep', { name, body, energyStructures, directions });
    return C.OK;
  };

  /** 官方 renewCreep（structures.js:1237-1267）：busy → target 校验（含
   *  assertTargetObject 与 CLAIM 部件拒）→ 归属 → off（raw 回查）→ 邻接 →
   *  能量（ceil(SPAWN_RENEW_RATIO*cost/CREEP_SPAWN_TIME/body.length)）→
   *  满（floor(SPAWN_RENEW_RATIO*CREEP_LIFE_TIME/CREEP_SPAWN_TIME/body.length)
   *  超 CREEP_LIFE_TIME）→ boost 部件 deprecated 警告 → intent。 */
  globalThis.StructureSpawn.prototype.renewCreep = function renewCreep(target) {
    if (this.spawning) {
      return C.ERR_BUSY;
    }
    if (!target || !target.id || !isKnownCreep(target.id) || !(target instanceof globalThis.Creep) || target.spawning || _.any(target.body, { type: C.CLAIM })) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (!this.my || !target.my) {
      return C.ERR_NOT_OWNER;
    }
    if (gw.rawData(this.id).off) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (!target.pos.isNearTo(this.pos)) {
      return C.ERR_NOT_IN_RANGE;
    }
    if (this.room.energyAvailable < __MathCeil((C.SPAWN_RENEW_RATIO * calcCreepCost(target.body)) / C.CREEP_SPAWN_TIME / target.body.length)) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    if (target.ticksToLive + __MathFloor((C.SPAWN_RENEW_RATIO * C.CREEP_LIFE_TIME) / C.CREEP_SPAWN_TIME / target.body.length) > C.CREEP_LIFE_TIME) {
      return C.ERR_FULL;
    }
    if (_.any(target.body, (i) => !!i.boost)) {
      deprecated('Using `StructureSpawn.renewCreep` on a boosted creep is deprecated and will throw an error soon. Please remove boosts using `StructureLab.unboostCreep` before renewing.');
    }
    gw.intents.set(this.id, 'renewCreep', { id: target.id });
    return C.OK;
  };

  /** 官方 recycleCreep（structures.js:1269-1290）：预检序与 renewCreep 不同——
   *  my 最先、off 在 target 校验后、target.my 单独判（无 CLAIM 条件）。 */
  globalThis.StructureSpawn.prototype.recycleCreep = function recycleCreep(target) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (!target || !target.id || !isKnownCreep(target.id) || !(target instanceof globalThis.Creep) || target.spawning) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (gw.rawData(this.id).off) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (!target.my) {
      return C.ERR_NOT_OWNER;
    }
    if (!target.pos.isNearTo(this.pos)) {
      return C.ERR_NOT_IN_RANGE;
    }
    gw.intents.set(this.id, 'recycleCreep', { id: target.id });
    return C.OK;
  };

  // ---------------------------------------------------------------- 原型方法（桩）
  const STRUCTURE_METHODS = ['destroy', 'setActive', 'notifyWhenAttacked'];
  // spawn：destroy/setActive/notifyWhenAttacked 保持桩（官方 destroy :79-97/
  // setActive :103-127/notifyWhenAttacked :89-101——A4-iv 批次换真面）
  for (const m of STRUCTURE_METHODS) {
    globalThis.StructureSpawn.prototype[m] = gw.mkStubMethod('spawn.' + m);
  }
  // 通用结构方法：不挂 Structure 基类原型（controller/source/mineral 现状无
  // 这些方法，挂基类会让它们经原型链继承——语义变化）；else 族按类原型/实例
  // 兜底挂载

  // ---------------------------------------------------------------- controller 真方法（A4-iii）
  /** StructureController.unclaim：官方 structures.js:200-208——!my 门后记
   *  intent（结算侧 unclaim.js 清主权，settle/controller.rs）。 */
  globalThis.StructureController.prototype.unclaim = function unclaim() {
    if (!this.my) return C.ERR_NOT_OWNER;
    gw.intents.set(this.id, 'unclaim', {});
    return C.OK;
  };

  /** StructureController.activateSafeMode：官方 structures.js:210-234 六门
   *  全序（预检序不重排）：
   *  ① !this.my → ERR_NOT_OWNER（:211）
   *  ② safeModeAvailable <= 0 → ERR_NOT_ENOUGH_RESOURCES（:214）
   *  ③ safeModeCooldown || upgradeBlocked > 0 || ticksToDowngrade <
   *    CONTROLLER_DOWNGRADE[level]/2 − THRESHOLD → ERR_TIRED（:215-218）——
   *    经 getter 通道读（官方 this.xxx）；cooldown/blocked 官方 undefined
   *    参与比较恒假，我方同（新 getter undefined 形态）；ticksToDowngrade
   *    无钟 0（None→0 既有口径）→ 0 < 阈值为真，官方 undefined 比较为假
   *    ——API 返回码差异（登记报告；终态一致：结算侧对无钟房同拒）
   *  ④ 第六门：任何自家房 controller safeMode 生效中 → ERR_BUSY（:223-225，
   *    官方 _.any(register.structures)——我方对注入世界全结构扫描等价）
   *  ⑤ lastActivateSafeMode 闭包：有则 intents.remove(旧 id,
   *    'activateSafeMode')（收集器退费），记录本 id（:227-231）——同 tick
   *    后开者覆盖先开者；per-tick 重置（beginTick，官方 make 每 tick 新闭包）
   *  ⑥ intents.set('activateSafeMode', {})（:233） */
  globalThis.StructureController.prototype.activateSafeMode = function activateSafeMode() {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.safeModeAvailable <= 0) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (this.safeModeCooldown || this.upgradeBlocked > 0
      || this.ticksToDowngrade < C.CONTROLLER_DOWNGRADE[this.level] / 2
        - C.CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD) {
      return C.ERR_TIRED;
    }
    for (const o of gw.world.objects || []) {
      if (o.type === 'structure' && o.structureType === 'controller'
        && o.owner === gw.viewerName && o.safeMode && o.safeMode > now()) {
        return C.ERR_BUSY;
      }
    }
    if (gw.lastActivateSafeMode) {
      gw.intents.remove(gw.lastActivateSafeMode, 'activateSafeMode');
    }
    gw.lastActivateSafeMode = this.id;
    gw.intents.set(this.id, 'activateSafeMode', {});
    return C.OK;
  };

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
    // extension/storage 有官方字段表（上方已挂类原型）；road/wall 与未来新
    // 结构类型兜底实例级 getter（同一回查机制）
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
