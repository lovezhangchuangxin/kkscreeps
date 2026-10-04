/* 补充结构属性与动作：孵化、控制器、塔、link、terminal，以及墓碑和废墟视图。
 * 复用 structures.js 的 Store 和常量；方法记录指令，Rust 结算负责实际修改世界。
 * 孵化成功会立即在本 tick 的 Game.creeps 放入无 id 的待出生对象；它不是已提交的世界对象。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  const _ = globalThis.__kkp.lodash;
  const __Object = gw.__Object;
  const __MathCeil = Math.ceil;
  const __MathFloor = Math.floor;
  // 复用 structures.js 提供的常量、时间函数和 Store。
  const C = gw.__structuresC;
  const now = gw.now;
  const Store = gw.Store;

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

  // 每 tick 同一弃用消息只输出一次，使用加载时捕获的 console 对象。
  // 来源：上游 engine/src/game/game.js 的 register.deprecated。
  const __console = globalThis.console;
  function deprecated(msg) {
    if (!_.contains(deprecatedShown, msg)) {
      deprecatedShown.push(msg);
      __console.log(msg);
    }
  }
  gw.deprecated = deprecated;

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

  /** 检查 id 是否存在于当前 tick 的 raw 数据且 type 为 structure 或 spawn。
   * Source/Mineral 也使用 raw type=structure，本函数不能独立验证完整的结构目标范围；
   * 调用方还需检查 instanceof 或排除其 structureType。 */
  function isKnownStructure(id) {
    const o = id == null ? undefined : gw.rawById.get(id);
    return !!o && (o.type === 'structure' || o.type === 'spawn');
  }
  // 供 Creep 战斗方法复用当前 tick 的目标存活检查。
  gw.isKnownCreep = isKnownCreep;
  gw.isKnownStructure = isKnownStructure;

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

  // 官方 structures.js:865-885：spawn.memory 独立 getter/setter（非本人 undefined/
  // 抛错；gw.memoryFor 官方同形——含 'undefined' 病态检查与非对象分支）
  Object.defineProperty(globalThis.StructureSpawn.prototype, 'memory', {
    get() {
      if (!this.my) return undefined;
      return gw.memoryFor('spawns', this.name);
    },
    set(value) {
      if (!this.my) {
        throw new Error("Could not set other player's spawn memory");
      }
      gw.setMemoryFor('spawns', this.name, value, 'spawn');
    },
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
  gw.defineGameObjectProperties(globalThis.StructureRoad.prototype, gw.rawData, ELSE_STRUCTURE_FIELDS);
  gw.defineGameObjectProperties(globalThis.StructureWall.prototype, gw.rawData, ELSE_STRUCTURE_FIELDS);
  // road 优先以 nextDecayTime 计算剩余 tick，再用 decayTime。
  // wall 不添加 ticksToDecay，避免多出上游不存在的属性。
  // 来源：上游 engine/src/game/structures.js 的 StructureRoad/StructureWall 属性定义。
  gw.defineGameObjectProperties(globalThis.StructureRoad.prototype, gw.rawData, {
    ticksToDecay: (o) => (o.nextDecayTime ? o.nextDecayTime - gw.world.time
      : o.decayTime ? o.decayTime - gw.world.time : undefined),
  });

  // ---------------------------------------------------------------- spawn 方法族

  /** 立即放入 Game.creeps 的待出生对象，无 id，spawning=true，字段由 getter 给出。
   * 它只是当前 tick 的玩家视图；创建指令尚未在 Rust 世界中结算。
   * spawnCreep 与 createCreep 共用；来源：上游 engine/src/game/structures.js 的孵化视图装配。 */
  function spawnGhostCreep(name, body, spawn) {
    const ghost = __Object.create(globalThis.Creep.prototype);
    ghost.room = spawn.room;
    ghost.pos = new gw.RoomPosition(spawn.pos.x, spawn.pos.y, spawn.pos.roomName);
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
  }

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
    // 访问全局 Memory，让其只在本 tick 首次读取时解析。
    // 不能直接调用 ensureMemory；重复解析会覆盖玩家在同 tick 已经写入的数据。
    // 来源：上游 engine/src/game/structures.js 的 spawnCreep Memory 写入。
    const memory = globalThis.Memory;
    if (memory.creeps === undefined) {
      memory.creeps = {};
    }
    if (_.isObject(memory.creeps)) {
      memory.creeps[name] = options.memory || memory.creeps[name] || {};
    }
    // 同 tick 幽灵壳（官方 :1132-1213）——spawnCreep/createCreep 共用
    //（官方两 API 的壳逐字段同形，见 spawnGhostCreep）
    spawnGhostCreep(name, body, this);
    gw.intents.set(this.id, 'createCreep', { name, body, energyStructures, directions });
    return C.OK;
  };

  // 随机从名称池选取未使用的名字；第五次尝试起拼接两个名字，再继续重试。
  // 来源：上游 engine/src/game/names.js 的 getUniqueName。
  const boyNames = ['Jackson', 'Aiden', 'Liam', 'Lucas', 'Noah', 'Mason', 'Jayden', 'Ethan', 'Jacob', 'Jack', 'Caden', 'Logan', 'Benjamin', 'Michael', 'Caleb', 'Ryan', 'Alexander', 'Elijah', 'James', 'William', 'Oliver', 'Connor', 'Matthew', 'Daniel', 'Luke', 'Brayden', 'Jayce', 'Henry', 'Carter', 'Dylan', 'Gabriel', 'Joshua', 'Nicholas', 'Isaac', 'Owen', 'Nathan', 'Grayson', 'Eli', 'Landon', 'Andrew', 'Max', 'Samuel', 'Gavin', 'Wyatt', 'Christian', 'Hunter', 'Cameron', 'Evan', 'Charlie', 'David', 'Sebastian', 'Joseph', 'Dominic', 'Anthony', 'Colton', 'John', 'Tyler', 'Zachary', 'Thomas', 'Julian', 'Levi', 'Adam', 'Isaiah', 'Alex', 'Aaron', 'Parker', 'Cooper', 'Miles', 'Chase', 'Muhammad', 'Christopher', 'Blake', 'Austin', 'Jordan', 'Leo', 'Jonathan', 'Adrian', 'Colin', 'Hudson', 'Ian', 'Xavier', 'Camden', 'Tristan', 'Carson', 'Jason', 'Nolan', 'Riley', 'Lincoln', 'Brody', 'Bentley', 'Nathaniel', 'Josiah', 'Declan', 'Jake', 'Asher', 'Jeremiah', 'Cole', 'Mateo', 'Micah', 'Elliot'];
  const girlNames = ['Sophia', 'Emma', 'Olivia', 'Isabella', 'Mia', 'Ava', 'Lily', 'Zoe', 'Emily', 'Chloe', 'Layla', 'Madison', 'Madelyn', 'Abigail', 'Aubrey', 'Charlotte', 'Amelia', 'Ella', 'Kaylee', 'Avery', 'Aaliyah', 'Hailey', 'Hannah', 'Addison', 'Riley', 'Harper', 'Aria', 'Arianna', 'Mackenzie', 'Lila', 'Evelyn', 'Adalyn', 'Grace', 'Brooklyn', 'Ellie', 'Anna', 'Kaitlyn', 'Isabelle', 'Sophie', 'Scarlett', 'Natalie', 'Leah', 'Sarah', 'Nora', 'Mila', 'Elizabeth', 'Lillian', 'Kylie', 'Audrey', 'Lucy', 'Maya', 'Annabelle', 'Makayla', 'Gabriella', 'Elena', 'Victoria', 'Claire', 'Savannah', 'Peyton', 'Maria', 'Alaina', 'Kennedy', 'Stella', 'Liliana', 'Allison', 'Samantha', 'Keira', 'Alyssa', 'Reagan', 'Molly', 'Alexandra', 'Violet', 'Charlie', 'Julia', 'Sadie', 'Ruby', 'Eva', 'Alice', 'Eliana', 'Taylor', 'Callie', 'Penelope', 'Camilla', 'Bailey', 'Kaelyn', 'Alexis', 'Kayla', 'Katherine', 'Sydney', 'Lauren', 'Jasmine', 'London', 'Bella', 'Adeline', 'Caroline', 'Vivian', 'Juliana', 'Gianna', 'Skyler', 'Jordyn'];
  function getUniqueName(checkExistingFn) {
    let c = 0, name, exists;
    do {
      const list = Math.random() > 0.5 ? boyNames : girlNames;
      name = list[Math.floor(Math.random() * list.length)];
      if (c > 3) {
        name += list[Math.floor(Math.random() * list.length)];
      }
      c++;
      exists = checkExistingFn(name);
    } while (exists);
    return name;
  }

  /** 官方 canCreateCreep（structures.js:898-929，deprecated 对的判定半）：
   *  预检序与 spawnCreep **不同**（官方独立函数）——!my → spawning（raw
   *  回查）→ body 校验 → room.energyAvailable（无 energyStructures 面第
   *  六参不存在）→ off（raw 回查）→ NAME_EXISTS → 名长>100。 */
  globalThis.StructureSpawn.prototype.canCreateCreep = function canCreateCreep(body, name) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (gw.rawData(this.id).spawning) {
      return C.ERR_BUSY;
    }
    if (!body || !_.isArray(body) || body.length === 0 || body.length > C.MAX_CREEP_SIZE) {
      return C.ERR_INVALID_ARGS;
    }
    for (let i = 0; i < body.length; i++) {
      if (!_.contains(C.BODYPARTS_ALL, body[i])) {
        return C.ERR_INVALID_ARGS;
      }
    }
    if (this.room.energyAvailable < calcCreepCost(body)) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    if (gw.rawData(this.id).off) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (name) {
      name = name.toString();
      if (gw.currentGame().creeps[name] || createdCreepNames.indexOf(name) !== -1) {
        return C.ERR_NAME_EXISTS;
      }
      if (name.length > 100) {
        return C.ERR_INVALID_ARGS;
      }
    }
    return C.OK;
  };

  /** 官方 createCreep（structures.js:932-1051，deprecated）：memory 移位参 →
   *  canCreateCreep 透传 → 自动名（names.js）→ Memory.creeps 显式写（与
   *  spawnCreep 的 `opts.memory || 旧值 || {}` 不同：**显式 creepMemory 覆盖
   *  旧值**，:976-982）→ 幽灵壳 → intent（无 energyStructures/directions
   *  面）→ **返回 name**（错误时返回错误码）。 */
  globalThis.StructureSpawn.prototype.createCreep = function createCreep(body, name, creepMemory) {
    if (_.isObject(name) && _.isUndefined(creepMemory)) {
      creepMemory = name;
      name = undefined;
    }
    const canResult = this.canCreateCreep(body, name);
    if (canResult !== C.OK) {
      return canResult;
    }
    if (!name) {
      // 官方 :956-959：可见 creep 里同主同名 + 本 tick 已用名都算存在
      const spawnOwner = gw.rawData(this.id).owner;
      name = getUniqueName((i) => {
        const all = gw.world.objects || [];
        for (let k = 0; k < all.length; k++) {
          const o = all[k];
          if (o.type === 'creep' && o.owner === spawnOwner && o.name === i) {
            return true;
          }
        }
        return createdCreepNames.indexOf(i) !== -1;
      });
    }
    createdCreepNames.push(name);
    // 同 spawnCreep 的懒触发口径（官方 :969-982 读 globals.Memory——首访
    // 固化后同 tick 读写同一对象，见 spawnCreep 内勘正注）
    const memory = globalThis.Memory;
    if (_.isUndefined(memory.creeps)) {
      memory.creeps = {};
    }
    if (_.isObject(memory.creeps)) {
      if (!_.isUndefined(creepMemory)) {
        memory.creeps[name] = creepMemory;
      } else {
        memory.creeps[name] = memory.creeps[name] || {};
      }
    }
    spawnGhostCreep(name, body, this);
    gw.intents.set(this.id, 'createCreep', { name, body });
    return name;
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

  // 结构受攻击通知设置。
  /** Structure.notifyWhenAttacked 基类版（官方 structures.js:89-101）：
   *  无房 INVALID_TARGET；非己结构（my === false——undefined 中性结构放行）
   *  或房 controller 归他人 → NOT_OWNER；非布尔参数 INVALID_ARGS；与当前
   *  存值不同才记 intent（官方 `!=`——undefined 与任何布尔不等，首调恒记）。
   *  挂 Structure 基类原型（官方同位）：controller 经 OwnedStructure 继承链
   *  同样持有本方法（官方语义即如此——其 intent 在 processor 层因 controller
   *  不在 CONSTRUCTION_COST 而空转）；source/mineral 官方不 extends Structure，
   *  我方原型链同构（RoomObject 直挂）不受影响。 */
  globalThis.Structure.prototype.notifyWhenAttacked = function notifyWhenAttacked(enabled) {
    if (!this.room) return C.ERR_INVALID_TARGET;
    const ctl = this.room.controller;
    if (this.my === false || (ctl && ctl.owner && !ctl.my)) {
      return C.ERR_NOT_OWNER;
    }
    if (typeof enabled !== 'boolean') return C.ERR_INVALID_ARGS;
    if (enabled !== gw.rawData(this.id).notifyWhenAttacked) {
      gw.intents.set(this.id, 'notifyWhenAttacked', { enabled });
    }
    return C.OK;
  };


  // 动作方法挂在类原型上；装配器不能再添加同名实例方法，否则会遮蔽这些实现。

  // 控制器操作方法。
  /** StructureController.unclaim：官方 structures.js:200-208——!my 门后记
   *  intent（结算侧 unclaim.js 清主权，settle/controller.rs）。 */
  globalThis.StructureController.prototype.unclaim = function unclaim() {
    if (!this.my) return C.ERR_NOT_OWNER;
    gw.intents.set(this.id, 'unclaim', {});
    return C.OK;
  };

  /** 先检查归属、可用次数、冷却、升级封锁与降级剩余时间，再检查自家已有安全模式。
   * 同 tick 多个有效请求只保留最后一个，并移除上一个请求的指令费用。
   * 缺少 downgradeTime 时此实现读取 0，会影响 ERR_TIRED 预检；Rust 结算另行复核。
   * 来源：上游 engine/src/game/structures.js 的 activateSafeMode。 */
  globalThis.StructureController.prototype.activateSafeMode = function activateSafeMode() {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.safeModeAvailable <= 0) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (this.safeModeCooldown || this.upgradeBlocked > 0
      || this.ticksToDowngrade < C.CONTROLLER_DOWNGRADE[this.level] / 2
        - C.CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD) {
      return C.ERR_TIRED;
    }
    // 扫描所有自家控制器是否已有生效的安全模式，按索引读取以避开数组迭代器补丁。
    const objs = gw.world.objects || [];
    for (let i = 0; i < objs.length; i++) {
      const o = objs[i];
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


  /** StructureSpawn.notifyWhenAttacked 覆盖版（官方 structures.js:1220-1233）：
   *  与基类版的差异——无 `!this.room` 门、无 controller 归属分支（spawn 恒有
   *  主，my 判定即足）。 */
  globalThis.StructureSpawn.prototype.notifyWhenAttacked = function notifyWhenAttacked(enabled) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (typeof enabled !== 'boolean') return C.ERR_INVALID_ARGS;
    if (enabled !== gw.rawData(this.id).notifyWhenAttacked) {
      gw.intents.set(this.id, 'notifyWhenAttacked', { enabled });
    }
    return C.OK;
  };


  // spawn 的字段与方法来自原型，实例仅补 isStructure；InvaderCore 也复用 Spawning 类。
  gw.Spawning = Spawning;
  gw.assemblers.spawn = (o, base) => {
    base.isStructure = true;
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
    }
    return base;
  };

  // 销毁需要本房自家控制器，且房内没有敌方 creep/powerCreep，然后记录房间指令。
  // 方法挂在 Structure.prototype；controller 通过继承获得它，Source/Mineral 不继承 Structure。
  // 来源：上游 engine/src/game/structures.js 的 Structure.destroy。
  globalThis.Structure.prototype.destroy = function destroy() {
    if (!this.room) return C.ERR_INVALID_TARGET;
    if (!this.room.controller || !this.room.controller.my) return C.ERR_NOT_OWNER;
    if (this.room.find(C.FIND_HOSTILE_CREEPS).length > 0
      || this.room.find(C.FIND_HOSTILE_POWER_CREEPS).length > 0) {
      return C.ERR_BUSY;
    }
    gw.intents.pushByName('room', 'destroyStructure', { roomName: this.room.name, id: this.id });
    return C.OK;
  };

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

  // 结构动作的 API 预检与指令记录。
  /** 塔动作共用能量与 isActive 检查；目标检查由各动作先完成。
   * 没有控制器时 isActive 返回 false，因此返回 ERR_RCL_NOT_ENOUGH。 */
  function towerCommonGates(tower) {
    const o = gw.rawData(tower.id);
    if (!o.store || o.store.energy < C.TOWER_ENERGY_COST) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    if (!tower.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    return C.OK;
  }

  /** 检查本人权限、当前 tick 的目标类型与存活状态，再检查能量和名额。
   * 此 API 不检查距离，距离衰减由结算处理。来源：上游 engine/src/game/structures.js 的 StructureTower.attack。 */
  globalThis.StructureTower.prototype.attack = function attack(target) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    const okTarget = target && target.id
      && (isKnownCreep(target.id) || isKnownStructure(target.id))
      && (target instanceof globalThis.Creep
        || target instanceof globalThis.StructureSpawn
        || target instanceof globalThis.Structure);
    if (!okTarget) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const gate = towerCommonGates(this);
    if (gate !== C.OK) {
      return gate;
    }
    gw.intents.set(this.id, 'attack', { id: target.id });
    return C.OK;
  };

  /** 目标限当前 tick 中存活的 Creep，检查能量与启用状态后记录 heal。
   * 此实现尚不接受 PowerCreep。来源：上游 engine/src/game/structures.js 的 StructureTower.heal。 */
  globalThis.StructureTower.prototype.heal = function heal(target) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (!(target && target.id && isKnownCreep(target.id)
      && target instanceof globalThis.Creep)) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const gate = towerCommonGates(this);
    if (gate !== C.OK) {
      return gate;
    }
    gw.intents.set(this.id, 'heal', { id: target.id });
    return C.OK;
  };

  /** 官方 Tower.repair（:812-834）：目标限 structure register + instanceof。 */
  globalThis.StructureTower.prototype.repair = function repair(target) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    const okTarget = target && target.id
      && isKnownStructure(target.id)
      && (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn);
    if (!okTarget) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const gate = towerCommonGates(this);
    if (gate !== C.OK) {
      return gate;
    }
    gw.intents.set(this.id, 'repair', { id: target.id });
    return C.OK;
  };

  /** 官方 Link.transferEnergy（structures.js:488-531）十二步逐字：
   *  ①amount<0→INVALID_ARGS ②目标门（structure register+instanceof+非
   *  自身+structureType==link）→INVALID_TARGET ③!target.my→NOT_OWNER
   *  ④源是敌方 link（my===false 精确比较）且站 rampart→NOT_OWNER（官方
   *  奇行 :503-505）⑤源冷却→TIRED ⑥无 controller→RCL ⑦RCL 配额
   *  （isActive 等价，见 towerCommonGates 注）⑧无能量→NOT_ENOUGH_ENERGY
   *  ⑨amount 缺省=min(源能量, 目标余量) ⑩源能量<amount→NOT_ENOUGH_ENERGY
   *  ⑪目标无容量键/满→FULL ⑫跨房→NOT_IN_RANGE（**同房限定**——官方
   *  :526-528；结算层的本格坐标距离即全语义）。 */
  globalThis.StructureLink.prototype.transferEnergy = function transferEnergy(target, amount) {
    if (amount < 0) {
      return C.ERR_INVALID_ARGS;
    }
    if (!target || !target.id
      || !isKnownStructure(target.id)
      || !(target instanceof globalThis.Structure)
      || target === this
      || target.structureType !== C.STRUCTURE_LINK) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (!target.my) {
      return C.ERR_NOT_OWNER;
    }
    if (this.my === false
      && _.any(this.pos.lookFor(C.LOOK_STRUCTURES), (i) => i.structureType === C.STRUCTURE_RAMPART)) {
      return C.ERR_NOT_OWNER;
    }
    if (this.cooldown > 0) {
      return C.ERR_TIRED;
    }
    if (!this.room.controller) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    const o = gw.rawData(this.id);
    if (!o.store || !o.store.energy) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    if (!amount) {
      const t = gw.rawData(target.id);
      amount = Math.min(
        o.store.energy,
        t.storeCapacityResource ? t.storeCapacityResource.energy - t.store.energy : 0,
      );
    }
    if (this.energy < amount) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    const t = gw.rawData(target.id);
    if (!t.storeCapacityResource || !t.storeCapacityResource.energy
      || t.store.energy + amount > t.storeCapacityResource.energy) {
      return C.ERR_FULL;
    }
    if (target.pos.roomName !== this.pos.roomName) {
      return C.ERR_NOT_IN_RANGE;
    }
    gw.intents.set(this.id, 'transfer', { id: target.id, amount, resourceType: 'energy' });
    return C.OK;
  };

  /** 官方 Rampart.setPublic（structures.js:652-657）：!my → intent
   *  {isPublic: !!isPublic}——结算 ramparts/set-public.js 同式写回。 */
  globalThis.StructureRampart.prototype.setPublic = function setPublic(isPublic) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    gw.intents.set(this.id, 'setPublic', { isPublic: !!isPublic });
    return C.OK;
  };

  /** 官方 StructureTerminal.send（structures.js:714-745）九步校验序逐字
   *  （TERMINAL-SEND-013 的 36 变体矩阵锁的就是此序）——2B 批 P1：
   *  ①!my→NOT_OWNER ②RCL（官方直调 checkStructureAgainstController——
   *  terminal 在 CONTROLLER_STRUCTURES 表内，isActive 四门等价【towerCommonGates
   *  注同款，无 controller 房走③门返 RCL=合意外延】）③房名 regex→INVALID_ARGS
   *  ④RESOURCES_ALL→INVALID_ARGS ⑤store 保有（`!store || !store[rt] ||
   *  store[rt] < amount`）→NOT_ENOUGH_RESOURCES ⑥cooldownTime>time→TIRED
   *  ⑦能量成本（rt≠energy：energy<cost / rt==energy：energy<amount+cost）
   *  →NOT_ENOUGH_RESOURCES ⑧description（非 string 或 >100）→INVALID_ARGS
   *  ⑨intents.set(id,'send',{resourceType,amount,targetRoomName,description})。
   *  **无最小量门**（TERMINAL_MIN_SEND 死常量不消费——TERMINAL-SEND-014 锁
   *  amount=1 可发）；intent payload 官方键序（send.js 消费 targetRoomName/
   *  resourceType/amount/description，description 经 userString 帽在清洗层）。 */
  globalThis.StructureTerminal.prototype.send = function send(resourceType, amount, targetRoomName, description) {
    if (!this.my) {
      return C.ERR_NOT_OWNER; // :715-717
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH; // :718-720（isActive 等价见注）
    }
    if (!/^(W|E)\d+(N|S)\d+$/.test(targetRoomName)) {
      return C.ERR_INVALID_ARGS; // :721-723
    }
    if (!_.contains(C.RESOURCES_ALL || [], resourceType)) {
      return C.ERR_INVALID_ARGS; // :724-726
    }
    const o = gw.rawData(this.id);
    if (!o.store || !o.store[resourceType] || o.store[resourceType] < amount) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :727-729
    }
    if (o.cooldownTime > gw.world.time) {
      return C.ERR_TIRED; // :730-732
    }
    // :733-737 距离/成本——与 Game.market.calcTransactionCost 同式（不环绕
    // plain Chebyshev 登记；Rust 侧 settle/market.rs calc_rooms_distance 同判）
    const xy1 = gw.roomNameToXY(this.pos.roomName);
    const xy2 = gw.roomNameToXY(targetRoomName);
    const range = Math.max(Math.abs(xy2[0] - xy1[0]), Math.abs(xy2[1] - xy1[1]));
    const cost = Math.ceil(amount * (1 - Math.exp(-range / 30)));
    if ((resourceType !== C.RESOURCE_ENERGY && o.store.energy < cost)
      || (resourceType === C.RESOURCE_ENERGY && o.store.energy < amount + cost)) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :738-741
    }
    if (description && (!_.isString(description) || description.length > 100)) {
      return C.ERR_INVALID_ARGS; // :742-744（length 为 UTF-16 码元——清洗层
      // userString 的 chars() 口径差异为既有登记）
    }
    gw.intents.set(this.id, 'send', { resourceType, amount, targetRoomName, description });
    return C.OK; // :745
  };
})();
