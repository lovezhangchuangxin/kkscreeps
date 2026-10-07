/* 孵化生命周期：本 tick 名称、待出生视图与创建/续命/回收方法共享同一闭包。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const _ = globalThis.__kkp.lodash;
  const __Object = gw.__Object;
  const __MathCeil = Math.ceil;
  const __MathFloor = Math.floor;
  const C = gw.__structuresC;
  const Store = gw.Store;

  // 入口传入共用目标检查和弃用诊断；安装与 tick 调用均不回查 __kkp。
  function createSpawnApi({ deprecated, assertTargetObject, isKnownCreep }) {
    let createdCreepNames = [];
    function resetNames() {
      createdCreepNames = [];
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

    function installView() {
      // ---------------------------------------------------------------- Spawning 类
      // 官方 StructureSpawn.Spawning（structures.js:1301-1310）：own 定格
      // name/needTime/remainingTime/directions + 非 enumerable 的 spawn 引用
      // （官方 register._objects[spawnId]——我方经 objectsById 取当前壳）。
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
    }

    function installMethods() {
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
    }

    function installNotify() {
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

    }

    function installAssembler() {
      // spawn 的字段与方法全在原型（官方 Surface 无额外实例标记）；
      // InvaderCore 也复用 Spawning 类。
      gw.Spawning = Spawning;
      gw.assemblers.spawn = (o, base) => base;
    }

    return { resetNames, installView, installMethods, installNotify, installAssembler };
  }
  globalThis.__kkp.createSpawnApi = createSpawnApi;
})();
