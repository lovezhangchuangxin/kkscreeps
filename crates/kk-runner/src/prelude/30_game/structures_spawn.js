/* kk-runner prelude · 30_game 段 5b：Structure 族 spawn 面（Spawning 类/
 * StructureSpawn 字段表与 spawn 方法族/notifyWhenAttacked/controller 真方法/
 * 装配/destroy/Tombstone/Ruin 接线）。B1 结构治理批自段 5（structures.js）
 * 纯搬移；per-tick 状态（createdCreepNames/deprecatedShown + perTickReset
 * 钩子）与本段全部用户同迁（闭包内自洽，零接线变更）；段间共享经 gw 桥
 * （C 表/now/Store），调用点保持裸名。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  const _ = globalThis.__kkp.lodash;
  const __Object = gw.__Object;
  const __MathCeil = Math.ceil;
  const __MathFloor = Math.floor;
  // B1 段拆分桥（5a 定义点）
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

  /** 官方 register.structures[target.id] 判定的等价物：raw 投影里存在且是
   *  结构面（M2B 扩面 P5——tower/link 目标门的存活判定；陈旧壳 id 不在本
   *  tick raw → 拒，官方 UNDOC-STALEARG 面：API 不得对已消亡对象的缓存壳
   *  返回 OK）。
   *  **域注意（评审 2 P2-3）**：≠ register.structures 精确域——source/mineral
   *  的 raw type 亦为 'structure'（world_json 归一），本判定对它们过收；
   *  四个调用点（attack/heal/repair/transferEnergy）全部以 instanceof
   *  Structure/Creep 合取，Source/Mineral 原型链不含 Structure（_registry
   *  CLASS_PARENTS）→ 最终同 ERR_INVALID_TARGET。**单独复用本函数时必须
   *  自带 instanceof 或补 structureType 排除**。 */
  function isKnownStructure(id) {
    const o = id == null ? undefined : gw.rawById.get(id);
    return !!o && (o.type === 'structure' || o.type === 'spawn');
  }

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
  // M2B P4：road 衰减钟折叠（官方 structures.js:672-674——与容器同式，
  // nextDecayTime 优先、decayTime 兜底）。**wall 官方无 ticksToDecay**
  //（structures.js:839-841 只挂 ticksToLive），故不对 wall 挂——挂上虽
  // 读 undefined 与官方同值，但会多出官方不存在的属性面。
  gw.defineGameObjectProperties(globalThis.StructureRoad.prototype, gw.rawData, {
    ticksToDecay: (o) => (o.nextDecayTime ? o.nextDecayTime - gw.world.time
      : o.decayTime ? o.decayTime - gw.world.time : undefined),
  });

  // ---------------------------------------------------------------- spawn 方法族
  /** 官方 spawnCreep（structures.js:1063-1218）全预检序：
   *  ①!name||!isObject(options)→INVALID_ARGS ②name.length>100→INVALID_ARGS
   *  ③同名（Game.creeps 或同 tick createdCreepNames）→NAME_EXISTS
   *  ④energyStructures pluck+uniq ⑤directions 数组化校验 ⑥!my→NOT_OWNER
   *  ⑦spawning（显式回查）→BUSY ⑧off（显式回查）→RCL ⑨body 校验
   *  ⑩energyAvailable<cost→NOT_ENOUGH_ENERGY ⑪dryRun→OK ⑫名单+Memory
   *  ⑬幽灵壳 ⑭intent ⑮OK。 */
  /** 同 tick 幽灵壳（官方 structures.js:1132-1213）：new Creep()（无 id）+
   *  RoomObject.call（room/pos）+ 十三字段 enumerable getter——官方逐字段对照
   *  （name/spawning:true/my:true/body:{type,hits:100}/owner/
   *  ticksToLive:CREEP_LIFE_TIME/carryCapacity/carry:{energy:0}/
   *  store:new Store({store:{energy:0}})/fatigue:0/hits/hitsMax/saying:
   *  undefined）。spawnCreep（M2A）与 createCreep（M2B 扩面 P5）共用。 */
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
    // 官方 structures.js:1122-1130（spawnCreep）/ :976-984（createCreep）：
    // `Memory.creeps[name] = opts.memory || ... || {}`——写在 API 层（结算层
    // create-creep.js 无 Memory 写）。读 globals.Memory 懒触发。
    // **不能直调 ensureMemory**——它会强制重解析
    // 并重固化，把玩家同 tick 早先对 Memory 的写入清掉；官方 getter 首访
    // 解析后固化为值属性，同 tick 后续读写同一对象。M2B 扩面 P5 勘正：
    // 原直调致 `Memory.x=1; spawnCreep(...); Memory.x` 丢写——官方保留）
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

  // ---------------------------------------------------------------- canCreateCreep/createCreep（M2B 扩面 P5）
  // 官方 game/names.js 逐字（100+100 名池 + getUniqueName 重试：4 次后双名
  // 拼接；Math.random 官方同源不可测——只锁「不与现存/本 tick 已用重名」）
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

  // ---------------------------------------------------------------- notifyWhenAttacked（A4-iv 换真）
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


  // ---------------------------------------------------------------- 原型方法
  // setActive 桩已摘除（A6）：官方 API 全集无此方法（structures.js:103-127
  // 实为 destroy/notifyWhenAttacked/isActive 三方法，旧注释误植）——玩家面
  // 挂官方没有的方法本身就是兼容红线违背。isActive 已在上方 Structure 基类
  // 原型真化（全结构族经原型链继承）。destroy/notifyWhenAttacked 同在基类
  // 原型（B3/A4-iv 接线）；不再有实例级桩循环——实例桩会遮蔽真方法（坑⑥）。

  // ---------------------------------------------------------------- controller 真方法（A4-iii）
  /** StructureController.unclaim：官方 structures.js:200-208——!my 门后记
   *  intent（结算侧 unclaim.js 清主权，settle/controller.rs）。 */
  globalThis.StructureController.prototype.unclaim = function unclaim() {
    if (!this.my) return C.ERR_NOT_OWNER;
    gw.intents.set(this.id, 'unclaim', {});
    return C.OK;
  };

  /** StructureController.activateSafeMode：官方 structures.js:211-234 六门
   *  全序（预检序不重排）：
   /*  ① !this.my → ERR_NOT_OWNER（:213）
   /*  ② safeModeAvailable <= 0 → ERR_NOT_ENOUGH_RESOURCES（:216）
   *  ③ safeModeCooldown || upgradeBlocked > 0 || ticksToDowngrade <
   /*    CONTROLLER_DOWNGRADE[level]/2 − THRESHOLD → ERR_TIRED（:219-221）——
   *    经 getter 通道读（官方 this.xxx）；cooldown/blocked 官方 undefined
   *    参与比较恒假，我方同（新 getter undefined 形态）；ticksToDowngrade
   *    无钟 0（None→0 既有口径）→ 0 < 阈值为真，官方 undefined 比较为假
   *    ——API 返回码差异（登记报告；终态一致：结算侧对无钟房同拒）
   /*  ④ 第六门：任何自家房 controller safeMode 生效中 → ERR_BUSY（:223-225，
   *    官方 _.any(register.structures)——我方对注入世界全结构扫描等价）
   *  ⑤ lastActivateSafeMode 闭包：有则 intents.remove(旧 id,
   /*    'activateSafeMode')（收集器退费），记录本 id（:227-231）——同 tick
   *    后开者覆盖先开者；per-tick 重置（beginTick，官方 make 每 tick 新闭包）
   /*  ⑥ intents.set('activateSafeMode', {})（:233） */
  globalThis.StructureController.prototype.activateSafeMode = function activateSafeMode() {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.safeModeAvailable <= 0) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (this.safeModeCooldown || this.upgradeBlocked > 0
      || this.ticksToDowngrade < C.CONTROLLER_DOWNGRADE[this.level] / 2
        - C.CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD) {
      return C.ERR_TIRED;
    }
    // 门④扫描用索引迭代：for...of 经 Array.prototype[Symbol.iterator] 运行期
    // 解析，同 realm 玩家可污染迭代器喂空本门（A4-iii 评审 2 必修）——Engine
    // 层 tick_with_intents 已按世界真值兜底，此处索引迭代是纵深防御
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

  // 其余结构方法无桩残留（destroy/notifyWhenAttacked 已在 Structure 基类换真
  // ——else 族/未知类型经原型链继承，不再挂实例桩防遮蔽，坑⑥教训）

  // ---------------------------------------------------------------- 装配
  // spawn 装配：字段/方法全在原型，own 面只剩 id/pos/room + 静态标记
  // （isActive 在 Structure 基类原型真化，不再挂实例桩）
  // M2B 扩面 P4：InvaderCore.spawning 复用（官方 structures.js:760
  // `new StructureSpawn.Spawning(id)`——跨段经 gw 桥）
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

  // ---------------------------------------------------------------- Structure.destroy（B3 接线批换真）
  // 官方 structures.js:72-86：!room → INVALID_TARGET；controller 非我 →
  // NOT_OWNER；房内敌方 creep/powerCreep → BUSY（API 层门——引擎侧
  // destroy-structure.js 四门对等，官方两层同判）；记 room 级 intent
  //（pushByName 数组通道，from_json "room" 分支同形）。挂基类原型：
  // controller/source/mineral 经原型链继承是官方同形（官方即挂在
  // Structure.prototype）
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

  // ---------------------------------------------------------------- Tombstone/Ruin 类接线（B3 接线批）
  // 官方 tombstones.js:41-135 / ruins.js:38-70 的 getter 族——投影（B3
  // world_json）已是官方 DB 形态（绝对 tick / 折叠 ticksToDecay / 正量 store
  // / structure 存照 type 键），字段表直读投影。
  // creep 代理对象（tombstones.js:47-135）：普通对象 + own getter +
  // Creep.prototype 原型链（官方 new Creep + defineProperties 覆盖同构）
  function tombstoneCreepProxy(o) {
    const proxy = {};
    const carryCap = (o.creepBody || []).reduce((n, t) => n + (t === 'carry' ? C.CARRY_CAPACITY : 0), 0);
    // pos 构造期定格为闭包引用（官方 RoomObject.call 的 own 数据属性同构——
    // 身份比较稳定；清偿复审 P3-R1）
    const posInstance = new gw.RoomPosition(o.x, o.y, o.room);
    const defs = {
      id: { enumerable: true, get: () => o.creepId },
      name: { enumerable: true, get: () => o.creepName },
      // 官方 RoomObject.call(this, x, y, room) 挂 pos/room（tombstones.js:47-48，
      // 评审 2 P2-1 清偿——bot 生态读 dead creep 位置）
      pos: { enumerable: true, get: () => posInstance },
      room: { enumerable: true, get: () => (gw.currentGame() ? gw.currentGame().rooms[o.room] : undefined) },
      spawning: { enumerable: true, get: () => false },
      my: { enumerable: true, get: () => o.user === gw.viewerName },
      body: { enumerable: true, get: () => (o.creepBody || []).map((t) => ({ type: t, hits: 0 })) },
      owner: { enumerable: true, get: () => (o.user === undefined || o.user === null ? undefined : { username: o.user }) },
      ticksToLive: { enumerable: true, get: () => o.creepTicksToLive },
      carryCapacity: { enumerable: true, get: () => carryCap },
      // 官方上游 quirk（tombstones.js:63-65 的 carry/store 走 _storeGetter 无参
      // 回退 `{store:{}, storeCapacity: this.carryCapacity}` → 恒空 Store 且容量
      // =carryCapacity——死 creep 的 store 不透传（墓碑的 store 才是资源面）。
      // 评审 1 P3-B：原实现给真库存=与官方双反，quirk 原样对齐）
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
    // 官方 Tombstone 原型无 carry/carryCapacity（tombstones.js 字段表）——
    // 评审 1 P3-A 清偿：它们只在 creep 代理上；多挂会让枚举面多出两键
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

  // StructureExtractor.cooldown（GPT 复审 P1-3 清偿——B4 轨 A 待接第 1 条
  // 提前：官方 structures.js:268 `o.cooldown || 0`；缺它时 harvest 预检读
  // undefined → 冷却期 API 假 OK（结算拒、矿量不动——返回码与终态双错）
  gw.defineGameObjectProperties(globalThis.StructureExtractor.prototype, gw.rawData, {
    cooldown: (o) => o.cooldown || 0,
  });

  // StructureRampart.isPublic（A5 片 3a 评审 P2 清偿——官方 structures.js:648
  // `!!o.isPublic`）：withdraw 敌 rampart 门（creep.js）随本片 lookFor 真化
  // 可观测，包装缺键时恒 undefined——isPublic:true 的公开敌方 rampart 官方
  // 放行、我方误拒。缺省 false 两侧同形（Rust 结算侧已读原始 isPublic）
  gw.defineGameObjectProperties(globalThis.StructureRampart.prototype, gw.rawData, {
    isPublic: (o) => !!o.isPublic,
  });

  // ---------------------------------------------------------------- M2B 扩面 P4：静态折叠 getter 族
  // 官方 structures.js 逐类表对照。挂钟折叠（官方读 runtimeData.time——
  // 我方读 gw.world.time 同位等价）：衰减钟 `nextDecayTime ? 差值 :
  // decayTime ? 差值 : undefined`（:147 容器式）；冷却钟
  // `cooldownTime && cooldownTime > time ? 差值 : 0`（:304 lab 式）。
  // 数据源=Generic 透传投影的绝对键（放置/结算写入）——官方 DB 同形态
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

  // ---------------------------------------------------------------- M2B 扩面 P5：结构动作族 API
  /** tower 三 API 的共同门（官方 structures.js:764-834 每方法同两段）：
   *  能量（`!store || store.energy < TOWER_ENERGY_COST`）+ RCL。RCL 门官方
   *  直接调 utils.checkStructureAgainstController——对在 CONTROLLER_STRUCTURES
   *  表内且房内有 controller 的结构，`!this.isActive()` 与其逐门等价
   *  （isActive 四门：①!owner→active ≡ 官方 :458-460；②表外类型→active
   *  对 tower 不可达；③无房/无 controller→inactive ≡ 官方 :463-464 的
   *  !roomController→false；④即 checkStructureAgainstController 本体）。
   *  官方对无 controller 的房会 `data(undefined.id)` 抛错——官方世界每房
   *  有 controller 不可达，我方野房夹具可达，走 isActive 的 ③ 门返
   *  ERR_RCL_NOT_ENOUGH（官方不可达分支的合意外延，注释记录）。 */
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

  /** 官方 Tower.attack（structures.js:764-787）：!my → 目标门（creep/
   *  powerCreep/structure 三 register + instanceof；register 判定以
   *  instanceof 等价——我方游戏对象只能经装配获得，与 creep.attack 的
   *  P1 口径一致）→ 能量 → RCL → intent {id}。官方无距离门（20 格 falloff
   *  底 150 仍执行）；**无 TOWER_COOLDOWN 常量**（限速=能量+每 tick 单动作
   *  else-if，towers/intents.js:3-8）。 */
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

  /** 官方 Tower.heal（:789-810）：目标限 creep/powerCreep（M2 无 powerCreep）。 */
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
})();
