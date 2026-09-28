/* kk-runner prelude · 30_game 段 4：Creep（对照官方 engine/src/game/creeps.js）。
 *
 * A3b 薄壳化：字段/方法全部挂 Creep.prototype（官方 make 语义），实例 own 面
 * 只剩 id/pos/room（构造定格）。字段经 defineGameObjectProperties 的 lazy
 * getter + `_${name}` 首访缓存（utils.js:509-545 机制）——未缓存访问经
 * gw.rawData(this.id) 回查当前 tick 投影：对象消亡抛官方同文案错误
 * （STALERECV-001），跨 tick 旧壳读当前 tick 新值（STALERECV-002）。
 * 动作方法（A2 起从桩换真：预检 + intents.set）维持官方预检序，字段访问走
 * this（getter 通道，与官方 this.my 一致）；官方方法体显式 data(this.id)
 * 回查的位置我方对齐 gw.rawData。
 * 依赖：gw.intents（收集器，30_game/game.js）、gw.rawData/gw.rawById、
 * gw.viewerName、gw.ensureMemory、gw.mkStubMethod、gw.defineGameObjectProperties。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;

  function creepMemoryFor(name) {
    const m = gw.ensureMemory();
    if (!m.creeps) m.creeps = {};
    if (!m.creeps[name]) m.creeps[name] = {};
    return m.creeps[name];
  }

  const CREEP_METHODS = ['move', 'moveByPath', 'moveTo', 'harvest', 'build', 'repair', 'dismantle', 'upgradeController', 'attack', 'rangedAttack', 'rangedMassAttack', 'heal', 'rangedHeal', 'claimController', 'reserveController', 'attackController', 'transfer', 'withdraw', 'drop', 'pickup', 'say', 'suicide', 'spawn', 'generateSafeMode', 'pull', 'recycleCreep', 'renewCreep', 'cancelOrder', 'moveHead'];

  // API 实现用的预置常量（官方 game 层的 C 来自 driver.constants 注入对象，
  // 玩家覆盖全局常量不影响 API 判定——game.js 头部 C = driver.constants 同款）
  // 对象型常量是全局对象的共享引用——冻结内容防玩家篡改（数值/字符串不可变无需处理）
  function freezeConst(obj) {
    if (obj && typeof obj === 'object') {
      Object.freeze(obj);
      for (const k of Object.keys(obj)) freezeConst(obj[k]);
    }
    return obj;
  }

  const C = {
    OK: globalThis.OK,
    ERR_NOT_OWNER: globalThis.ERR_NOT_OWNER,
    ERR_BUSY: globalThis.ERR_BUSY,
    ERR_TIRED: globalThis.ERR_TIRED,
    ERR_NO_BODYPART: globalThis.ERR_NO_BODYPART,
    ERR_INVALID_TARGET: globalThis.ERR_INVALID_TARGET,
    ERR_NOT_IN_RANGE: globalThis.ERR_NOT_IN_RANGE,
    ERR_NOT_ENOUGH_RESOURCES: globalThis.ERR_NOT_ENOUGH_RESOURCES,
    ERR_INVALID_ARGS: globalThis.ERR_INVALID_ARGS,
    MOVE: globalThis.MOVE,
    WORK: globalThis.WORK,
    ATTACK: globalThis.ATTACK,
    HEAL: globalThis.HEAL,
    ERR_NOT_FOUND: globalThis.ERR_NOT_FOUND,
    ERR_RCL_NOT_ENOUGH: globalThis.ERR_RCL_NOT_ENOUGH,
    ERR_FULL: globalThis.ERR_FULL,
    OBSTACLE_OBJECT_TYPES: freezeConst(globalThis.OBSTACLE_OBJECT_TYPES),
    CONSTRUCTION_COST: freezeConst(globalThis.CONSTRUCTION_COST),
    CONTROLLER_STRUCTURES: freezeConst(globalThis.CONTROLLER_STRUCTURES),
    // 资源族四方法（A4-ii）所需：资源白名单/容量折算表/结构类型常量
    RESOURCE_ENERGY: globalThis.RESOURCE_ENERGY,
    RESOURCES_ALL: freezeConst(globalThis.RESOURCES_ALL),
    CARRY_CAPACITY: globalThis.CARRY_CAPACITY,
    SPAWN_ENERGY_CAPACITY: globalThis.SPAWN_ENERGY_CAPACITY,
    EXTENSION_ENERGY_CAPACITY: freezeConst(globalThis.EXTENSION_ENERGY_CAPACITY),
    STORAGE_CAPACITY: globalThis.STORAGE_CAPACITY,
    CONTAINER_CAPACITY: globalThis.CONTAINER_CAPACITY,
    TERMINAL_CAPACITY: globalThis.TERMINAL_CAPACITY,
    FACTORY_CAPACITY: globalThis.FACTORY_CAPACITY,
    TOWER_CAPACITY: globalThis.TOWER_CAPACITY,
    LINK_CAPACITY: globalThis.LINK_CAPACITY,
    LAB_ENERGY_CAPACITY: globalThis.LAB_ENERGY_CAPACITY,
    LAB_MINERAL_CAPACITY: globalThis.LAB_MINERAL_CAPACITY,
    POWER_SPAWN_ENERGY_CAPACITY: globalThis.POWER_SPAWN_ENERGY_CAPACITY,
    POWER_SPAWN_POWER_CAPACITY: globalThis.POWER_SPAWN_POWER_CAPACITY,
    NUKER_ENERGY_CAPACITY: globalThis.NUKER_ENERGY_CAPACITY,
    NUKER_GHODIUM_CAPACITY: globalThis.NUKER_GHODIUM_CAPACITY,
    PWR_DISRUPT_TERMINAL: globalThis.PWR_DISRUPT_TERMINAL,
    STRUCTURE_RAMPART: globalThis.STRUCTURE_RAMPART,
    STRUCTURE_NUKER: globalThis.STRUCTURE_NUKER,
    STRUCTURE_POWER_BANK: globalThis.STRUCTURE_POWER_BANK,
  };

  // 官方 _hasActiveBodypart（game/creeps.js:9-17 的等价物）：hits>0 的指定部件数
  function countActiveParts(body, type) {
    let n = 0;
    for (const p of body || []) if (p.type === type && p.hits > 0) n += 1;
    return n;
  }

  // ---- 以下动作方法的预检逐条对照官方 creeps.js（file:line 见各函数）；
  // heal/rangedHeal 无执行器（B 批后续）但 intent 记录即参与压制图裁决 ----

  /** 官方 `controller && !controller.my && controller.safeMode` 判定
   *  （attack/heal/rangedHeal/dismantle 的 API 层门，creeps.js:602/699/723/1045）。 */
  function blockedBySafeMode(self) {
    const ctl = self.room && self.room.controller;
    return !!(ctl && !ctl.my && ctl.safeMode);
  }

  function carriedEnergy(self) {
    return (self.store && self.store.energy) || 0;
  }

  /** 单实例 isActive 近似（官方 utils.js checkStructureAgainstController 的
   * 单结构面）：无 controller 恒 active；owned 房按 CONTROLLER_STRUCTURES
   * 该类型在房 rcl 的配额 >0。isActive API 本体随 A4。 */
  function structureActiveAt(o, ctl) {
    if (!ctl) return true;
    const quota = (C.CONTROLLER_STRUCTURES || {})[o.structureType];
    if (!quota) return true;
    const level = Math.min(Math.max(ctl.level || 0, 0), 8);
    return (quota[level] || 0) > 0;
  }

  // ---------------------------------------------------------------- Creep 原型：字段表
  // 官方 creeps.js:64-90 字段表对照（我方投影字段名换算：ticksToLive 直存
  // 剩余值、owner 是用户名、body 部件归一 {type,hits}、saying 的 actionLog
  // 面未投影——官方无 say 记录同样 undefined）。store/carry 走官方
  // _storeGetter 形态（吃整个 raw 投影 o——creep 落库带 storeCapacity，
  // A4-i 起投影携带，Store 三函数族经它取数）
  gw.defineGameObjectProperties(globalThis.Creep.prototype, gw.rawData, {
    name: (o) => o.name,
    body: (o) => (o.body || []).map((p) => (typeof p === 'string' ? { type: p, hits: 100 } : p)),
    my: (o) => o.owner === gw.viewerName,
    owner: (o) => ({ username: o.owner || 'bot' }),
    spawning: (o) => o.spawning ?? false,
    ticksToLive: (o) => o.ticksToLive,
    carryCapacity: (o) => o.storeCapacity,
    carry: (o) => gw.makeStore(o),
    store: (o) => gw.makeStore(o),
    fatigue: (o) => o.fatigue || 0,
    hits: (o) => o.hits ?? 0,
    hitsMax: (o) => o.hitsMax ?? 0,
    saying: () => undefined,
  });

  // 官方 creeps.js:94-120：memory 是独立 getter/setter（不走 _${name} 缓存），
  // 非本人 creep 读 undefined、写抛错
  Object.defineProperty(globalThis.Creep.prototype, 'memory', {
    get() {
      if (this.id && !this.my) return undefined;
      return creepMemoryFor(this.name);
    },
    set(value) {
      if (this.id && !this.my) {
        throw new Error("Could not set other player's creep memory");
      }
      const m = gw.ensureMemory();
      if (!m.creeps) m.creeps = {};
      m.creeps[this.name] = value;
    },
  });

  // ---------------------------------------------------------------- Creep 原型：动作方法
  /** Creep.move：官方 creeps.js:126-156——重预检（fatigue/bodypart/方向）后记
   *  intent 返回 OK；朝相邻 creep 形态（target 为 Creep 实例）同官方 :132-141。
   *  官方 :130-133 读 this.my/this.spawning（getter 通道）；:145 的 fatigue 是
   *  data(this.id) 显式回查（新鲜读绕缓存——跨 tick 旧壳按当前值判 ERR_TIRED）。 */
  globalThis.Creep.prototype.move = function move(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (target && target instanceof globalThis.Creep) {
      if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
      gw.intents.set(this.id, 'move', { id: target.id });
      return C.OK;
    }
    if (gw.rawData(this.id).fatigue > 0) return C.ERR_TIRED;
    if (countActiveParts(this.body, C.MOVE) === 0) return C.ERR_NO_BODYPART;
    const direction = +target;
    if (!direction || direction < 1 || direction > 8) return C.ERR_INVALID_ARGS;
    gw.intents.set(this.id, 'move', { direction });
    return C.OK;
  };

  /** Creep.harvest：官方 creeps.js:335-402 的 source 分支（mineral/deposit 随
   *  对应能力批）——目标必须是 Source 实例（register.sources 判定的等价物）。 */
  globalThis.Creep.prototype.harvest = function harvest(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    if (!target || !target.id) return C.ERR_INVALID_TARGET;
    if (target instanceof globalThis.Mineral) {
      // 官方 mineral 分支（creeps.js:362-385）：矿量/邻接/提取器存在性与
      // 归属/启用/冷却。deposit 分支随 B4。
      if (!target.mineralAmount) return C.ERR_NOT_ENOUGH_RESOURCES;
      if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
      let extractor = null;
      for (const o of gw.world.objects || []) {
        if (o.room === target.pos.roomName && o.x === target.pos.x && o.y === target.pos.y
          && o.structureType === 'extractor') {
          extractor = gw.objectsById.get(o.id);
          break;
        }
      }
      if (!extractor) return C.ERR_NOT_FOUND;
      if (extractor.owner && !extractor.my) return C.ERR_NOT_OWNER;
      if (!structureActiveAt(extractor, this.room && this.room.controller)) {
        return C.ERR_RCL_NOT_ENOUGH;
      }
      if (extractor.cooldown) return C.ERR_TIRED;
      gw.intents.set(this.id, 'harvest', { id: target.id });
      return C.OK;
    }
    if (!(target instanceof globalThis.Source)) {
      // 其余目标同官方 register.assertTargetObject
      return C.ERR_INVALID_TARGET;
    }
    if (!target.energy) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    const ctl = this.room && this.room.controller;
    if (ctl && ctl.owner && ctl.owner.username !== this.owner.username) {
      return C.ERR_NOT_OWNER; // reservation 分支缺字段（灰区登记，B2 补）
    }
    gw.intents.set(this.id, 'harvest', { id: target.id });
    return C.OK;
  };

  /** 官方 attack（creeps.js:593-624）：ATTACK 部件 + Creep/Structure 目标 +
   *  邻接 + safeMode 门；effects 检查随 power 批（M2 无 effects 面）。 */
  globalThis.Creep.prototype.attack = function attack(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.ATTACK) === 0) return C.ERR_NO_BODYPART;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    const okTarget = target && target.id
      && (target instanceof globalThis.Creep
        || target instanceof globalThis.StructureSpawn
        || target instanceof globalThis.Structure);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'attack', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 官方 build（creeps.js:762-808）：WORK + 能量 + 工地目标 + 3 格 +
   *  目标格静态障碍/挡路 creep 预检（safeMode 敌我分支 :794-805）。 */
  globalThis.Creep.prototype.build = function build(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    if (!carriedEnergy(this)) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (!(target && target.id && target instanceof globalThis.ConstructionSite)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!this.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
    if ((C.OBSTACLE_OBJECT_TYPES || []).includes(target.structureType)) {
      let objectsInTile = false;
      let creepsInTile = false;
      let myCreepsInTile = false;
      for (const o of gw.world.objects || []) {
        if (o.room !== target.pos.roomName || o.x !== target.pos.x || o.y !== target.pos.y) continue;
        const t = o.type === 'structure' || o.type === 'spawn'
          ? (o.structureType || 'spawn') : o.type;
        if (!(C.OBSTACLE_OBJECT_TYPES || []).includes(t)) continue;
        if (o.type === 'creep') {
          creepsInTile = true;
          if (o.owner === this.owner.username) myCreepsInTile = true;
        } else {
          objectsInTile = true;
        }
      }
      if (objectsInTile) return C.ERR_INVALID_TARGET;
      const ctl = this.room && this.room.controller;
      const blocking = ctl && ctl.my && ctl.safeMode ? myCreepsInTile : creepsInTile;
      if (blocking) return C.ERR_INVALID_TARGET;
    }
    gw.intents.set(this.id, 'build', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 官方 repair（creeps.js:734-761）：WORK + 能量 + Structure 目标 + 3 格。 */
  globalThis.Creep.prototype.repair = function repair(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    if (!carriedEnergy(this)) return C.ERR_NOT_ENOUGH_RESOURCES;
    const okTarget = target && target.id
      && (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!this.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'repair', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 官方 dismantle（creeps.js:1024-1053）：WORK + 可拆结构
   *  （CONSTRUCTION_COST 有价）+ 邻接 + safeMode；effects 随 power 批。 */
  globalThis.Creep.prototype.dismantle = function dismantle(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    const okTarget = target && target.id
      && (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn)
      && C.CONSTRUCTION_COST && target.structureType in C.CONSTRUCTION_COST;
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    gw.intents.set(this.id, 'dismantle', { id: target.id });
    return C.OK;
  };

  /** 官方 heal（creeps.js:678-705）：HEAL + Creep 目标 + 邻接 + safeMode。 */
  globalThis.Creep.prototype.heal = function heal(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
    if (!(target && target.id && target instanceof globalThis.Creep)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    gw.intents.set(this.id, 'heal', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 官方 cancelOrder（creeps.js:1016-1022）：按动作名移除已记录的 intent，
   *  成功返回 OK（收集器退费），未记录返回 ERR_NOT_FOUND。 */
  globalThis.Creep.prototype.cancelOrder = function cancelOrder(name) {
    if (gw.intents.remove(this.id, name)) return C.OK;
    return C.ERR_NOT_FOUND;
  };

  /** 官方 rangedHeal（creeps.js:706-733）：HEAL + Creep 目标 + 3 格 + safeMode。 */
  globalThis.Creep.prototype.rangedHeal = function rangedHeal(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
    if (!(target && target.id && target instanceof globalThis.Creep)) {
      return C.ERR_INVALID_TARGET;
    }
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    if (!this.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'rangedHeal', { id: target.id });
    return C.OK;
  };

  // ---------------------------------------------------------------- 资源族四方法（A4-ii 换真）
  // 官方 creeps.js:404-593（drop:404 / transfer:428 / withdraw:493 / pickup:566）。
  // 公共面（本块局部助手，其余方法不消费）：
  //  · register.* 成员判定的投影等价：官方 register 按 tick 初 roomObjects 全集
  //    建（game.js:269-374；structures 收录键集 = structureTypes 表 :224-247，
  //    source/mineral 独立注册不进 structures，spawn 同时进 structures+spawns），
  //    我方 rawById 同源全集（M2A 无视野过滤的既有口径）；
  //  · utils.capacityForResource/calcResources（官方 utils.js:615-676）：官方读
  //    runtimeData 的 storeCapacity/storeCapacityResource 键——投影无此二键，
  //    按官方落成表折算（build.js:120-296 落成分支 + extensions/tick.js:8-17
  //    每结算 tick 的归一 + _recalc-body.js:23 的 creep body 折算），公式逐字保留；
  //  · assertTargetObject（官方 game.js:122-126）：预检拒绝分支里对 plain
  //    object + 24 位 id 的序列化对象抛错——调用位置与返回码次序逐条对照不动。

  /** 官方 structureTypes 键集（game.js:224-247，21 型）+ 'wall' 投影别名
   *  （classGlobalFor 将 wall/constructedWall 归并 StructureWall——手写世界
   *  两种拼法等价；官方 raw type 恒为 'constructedWall'）。 */
  const STRUCTURE_REGISTER_TYPES = new Set(['rampart', 'road', 'extension', 'constructedWall', 'wall',
    'keeperLair', 'controller', 'link', 'storage', 'tower', 'observer', 'powerBank', 'powerSpawn',
    'lab', 'extractor', 'terminal', 'container', 'spawn', 'nuker', 'portal', 'factory', 'invaderCore']);

  /** register.structures[id] 的投影等价（官方 game.js:295-299 收录 + :224-247
   *  键集；spawn 的 raw type 是 'spawn' 单列）。命中返回 raw 投影（官方表存
   *  wrapper、读 structureType 即 raw.type——我方直读 raw 等价），否则 undefined。 */
  function structureRegisterEntry(id) {
    const raw = gw.rawById.get(id);
    if (!raw) return undefined;
    if (raw.type === 'spawn') return raw;
    if (raw.type === 'structure' && STRUCTURE_REGISTER_TYPES.has(raw.structureType)) return raw;
    return undefined;
  }

  /** 官方 raw type 的投影等价（Structure.prototype.structureType 读 o.type，
   *  structures.js:65）：spawn 投影自带 structureType 键，structure 族读
   *  structureType，其余退 raw.type（仅结构语境消费）。 */
  function structureKindOf(raw) {
    return raw.structureType || (raw.type === 'spawn' ? 'spawn' : raw.type);
  }

  /** 官方 game.js:122-126：Memory 序列化对象（plain object + 24 位字符串 id）
   *  拒绝——预检拒绝分支的抛错点，文案逐字。 */
  function assertTargetObject(obj) {
    if (obj && isPlainObject(obj) && typeof obj.id === 'string' && obj.id.length === 24) {
      throw new Error("It seems you're trying to use a serialized game object stored in Memory which is not allowed. Please use `Game.getObjectById` to retrieve a live object reference instead.");
    }
  }
  // _.isPlainObject 等价（原型为 Object.prototype 或 null）——经 gw.__Object
  // 预置引用（玩家交换 globalThis.Object 不影响判定）
  function isPlainObject(v) {
    if (typeof v !== 'object' || v === null) return false;
    const proto = gw.__Object.getPrototypeOf(v);
    return proto === gw.__Object.prototype || proto === null;
  }

  /** 官方 creep storeCapacity（_recalc-body.js:23 calcBodyEffectiveness 的无
   *  boost 形态）：hits>0 的 carry 部件 × CARRY_CAPACITY。投影 body 是
   *  {type,hits}；手写世界可为字符串（同字段表归一 hits=100）。boost 容量
   *  加成随 power 批（M1 无 boost 面，两侧一致——执行器 store_capacity 同形）。 */
  function carryCapacityOf(raw) {
    let parts = 0;
    for (const p of raw.body || []) {
      const part = typeof p === 'string' ? { type: p, hits: 100 } : p;
      if (part && part.hits > 0 && part.type === 'carry') parts += 1;
    }
    return parts * C.CARRY_CAPACITY;
  }

  /** 房内 extension 容量（官方 extensions/tick.js:8-17 每结算 tick 归一
   *  storeCapacityResource.energy = EXTENSION_ENERGY_CAPACITY[rcl]；无
   *  controller 房保持落成值 0——build.js:134）。 */
  function extensionCapacityIn(roomName) {
    let level = null;
    for (const o of gw.world.objects || []) {
      if (o.type === 'structure' && o.structureType === 'controller' && o.room === roomName) {
        level = o.level || 0;
        break;
      }
    }
    if (level === null) return 0;
    return (C.EXTENSION_ENERGY_CAPACITY || {})[level] || 0;
  }

  /** 官方 raw 的 storeCapacity/storeCapacityResource 落成表（build.js:120-296）
   *  + creep body 折算。投影直供键（透传桶/手写世界/未来投影补键）优先原样——
   *  与官方 capacityForResource 直读 raw 键的形态对齐。 */
  function capacityFields(raw) {
    if (raw.storeCapacity !== undefined || raw.storeCapacityResource !== undefined) {
      return { capacity: raw.storeCapacity, perResource: raw.storeCapacityResource };
    }
    if (raw.type === 'creep') {
      return { capacity: carryCapacityOf(raw), perResource: undefined };
    }
    switch (structureKindOf(raw)) {
      case 'spawn': // build.js:124：scr {energy: SPAWN_ENERGY_CAPACITY}
        return { capacity: undefined,
          perResource: { energy: raw.energyCapacity || C.SPAWN_ENERGY_CAPACITY } };
      case 'extension': // build.js:134 落成 0，extensions/tick.js:8-17 按 rcl 归一
        return { capacity: undefined, perResource: { energy: extensionCapacityIn(raw.room) } };
      case 'storage': // build.js:155
        return { capacity: C.STORAGE_CAPACITY, perResource: undefined };
      case 'container': // build.js:258
        return { capacity: C.CONTAINER_CAPACITY, perResource: undefined };
      case 'terminal': // build.js:249
        return { capacity: C.TERMINAL_CAPACITY, perResource: undefined };
      case 'factory': // build.js:276
        return { capacity: C.FACTORY_CAPACITY, perResource: undefined };
      case 'tower': // build.js:199
        return { capacity: undefined, perResource: { energy: C.TOWER_CAPACITY } };
      case 'link': // build.js:144
        return { capacity: undefined, perResource: { energy: C.LINK_CAPACITY } };
      case 'lab': // build.js:230-231
        return { capacity: C.LAB_ENERGY_CAPACITY + C.LAB_MINERAL_CAPACITY,
          perResource: { energy: C.LAB_ENERGY_CAPACITY } };
      case 'powerSpawn': // build.js:239
        return { capacity: undefined,
          perResource: { energy: C.POWER_SPAWN_ENERGY_CAPACITY, power: C.POWER_SPAWN_POWER_CAPACITY } };
      case 'nuker': // build.js:265（ptr 变体不建模）
        return { capacity: undefined,
          perResource: { energy: C.NUKER_ENERGY_CAPACITY, G: C.NUKER_GHODIUM_CAPACITY } };
      default: // 其余类型官方无容量键 ⇒ capacityForResource 得 0
        return { capacity: undefined, perResource: undefined };
    }
  }

  /** 官方 utils.js:671-674 capacityForResource 逐字（scr 命中直取；否则
   *  storeCapacity − sum(scr)，负数钳 0）。 */
  function capacityForResource(raw, resourceType) {
    const fields = capacityFields(raw);
    const perResource = fields.perResource;
    if (perResource && perResource[resourceType]) return perResource[resourceType];
    let reserved = 0;
    if (perResource) for (const k of gw.__Object.keys(perResource)) reserved += perResource[k];
    return Math.max(0, (fields.capacity || 0) - reserved);
  }

  /** 官方 utils.js:615-620 calcResources 的 store 分支（own 键求和——_.sum
   *  同为 own 枚举键；无 store 对象的 RESOURCES_ALL 顶字段求和是旧引擎形态，
   *  投影对象不产生，不可达缺 0）。 */
  function sumStore(raw) {
    const store = raw.store;
    if (!store) return 0;
    let sum = 0;
    for (const k of gw.__Object.keys(store)) sum += store[k];
    return sum;
  }

  /** 官方 data(this.id).storeCapacity 读点（withdraw :544 / pickup :578 的
   *  actor 面）：creep 恒经 body 折算，投影直供键优先。 */
  function rawStoreCapacity(raw) {
    return capacityFields(raw).capacity;
  }

  /** Creep.drop：官方 creeps.js:404-427——my/spawning 门 → RESOURCES_ALL →
   *  store 保有（缺省全量）→ intents.set。注意官方**无** amount<0 预检（负数
   *  放行，执行器 drop.js:15 amount>0 才落地——no-op，官方同形不补门）。 */
  globalThis.Creep.prototype.drop = function drop(resourceType, amount) {
    if (!this.my) return C.ERR_NOT_OWNER; // :405-407
    if (this.spawning) return C.ERR_BUSY; // :408-410
    if (!(C.RESOURCES_ALL || []).includes(resourceType)) return C.ERR_INVALID_ARGS; // :411-413
    if (!gw.rawData(this.id).store || !gw.rawData(this.id).store[resourceType]) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :414-416
    }
    if (!amount) amount = gw.rawData(this.id).store[resourceType]; // :417-419 缺省全量
    if (gw.rawData(this.id).store[resourceType] < amount) return C.ERR_NOT_ENOUGH_RESOURCES; // :420-422
    gw.intents.set(this.id, 'drop', { amount, resourceType }); // :424
    return C.OK;
  };

  /** Creep.transfer：官方 creeps.js:428-492——amount<0 / 资源白名单 / 目标族
   *  判定（五 register 并查 + store/controller 形态 + spawning 目标 + instanceof
   *  族）/ controller 能量重定向 / 容量门 / 邻接 / 保有 / 缺省钳制，逐条对照。 */
  globalThis.Creep.prototype.transfer = function transfer(target, resourceType, amount) {
    if (!this.my) return C.ERR_NOT_OWNER; // :429-431
    if (this.spawning) return C.ERR_BUSY; // :432-434
    if (amount < 0) return C.ERR_INVALID_ARGS; // :435-437
    if (!(C.RESOURCES_ALL || []).includes(resourceType)) return C.ERR_INVALID_ARGS; // :438-440
    // 目标门 :441-452。customObjects M2 无（官方 register.customObjects 恒空表
    // ⇒ 该合取项恒真，省略）。structEntry 裸解引用保留官方求值形态：目标有
    // store 时 && 短路不解引用；storeless 的 creep/powerCreep 目标官方在此
    // TypeError（引擎态不可达——creep 恒有 store，手写世界可达，语义对齐）
    if (!target || !target.id) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const structEntry = structureRegisterEntry(target.id);
    const targetRaw = gw.rawById.get(target.id);
    const inRegisters = structEntry !== undefined
      || !!targetRaw && (targetRaw.type === 'creep' || targetRaw.type === 'powerCreep');
    if (!inRegisters
      || (!gw.rawData(target.id).store && structEntry.structureType !== 'controller')
      || (target instanceof globalThis.Creep && target.spawning) // :445
      || !(target instanceof globalThis.StructureSpawn)
        && !(target instanceof globalThis.Structure)
        && !(target instanceof globalThis.Creep)
        && !(target instanceof globalThis.PowerCreep)) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    // :455-457：能量转 controller 重定向 upgradeController（A4-iii 换真前经桩
    // 返回 OK 无 intent——换真后本分支自动闭合）
    if (resourceType === C.RESOURCE_ENERGY && structEntry
      && structureKindOf(structEntry) === 'controller') {
      return this.upgradeController(target);
    }
    const rawTarget = gw.rawData(target.id);
    if (!capacityForResource(rawTarget, resourceType)) return C.ERR_INVALID_TARGET; // :459-461
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE; // :463-465
    if (!gw.rawData(this.id).store || !gw.rawData(this.id).store[resourceType]) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :466-468
    }
    // :470-471：scr 目标按资源槽、通用仓按总量（calcResources —— 同 tick 内
    // wrapper store 求和 === raw store 求和，raw 面等价）
    const fields = capacityFields(rawTarget);
    const storedAmount = fields.perResource ? (rawTarget.store[resourceType] || 0) : sumStore(rawTarget);
    const targetCapacity = capacityForResource(rawTarget, resourceType);
    if (!rawTarget.store || storedAmount >= targetCapacity) return C.ERR_FULL; // :473-475
    if (!amount) {
      amount = Math.min(gw.rawData(this.id).store[resourceType], targetCapacity - storedAmount); // :477-479
    }
    if (gw.rawData(this.id).store[resourceType] < amount) return C.ERR_NOT_ENOUGH_RESOURCES; // :481-483
    if (amount + storedAmount > targetCapacity) return C.ERR_FULL; // :485-487（显式超容不钳制——拒）
    gw.intents.set(this.id, 'transfer', { id: target.id, amount, resourceType }); // :489
    return C.OK;
  };

  /** Creep.withdraw：官方 creeps.js:493-565——Structure/Tombstone/Ruin + store
   *  面 → terminal 封锁（M1 无 effects 面）→ 敌 rampart（lookFor 桩恒放行，
   *  调用形态保留）→ safeMode → nuker/powerBank → 容量/保有 → 邻接 → actor
   *  空容量 → 缺省/钳制，逐条对照。 */
  globalThis.Creep.prototype.withdraw = function withdraw(target, resourceType, amount) {
    if (!this.my) return C.ERR_NOT_OWNER; // :494-496
    if (this.spawning) return C.ERR_BUSY; // :497-499
    if (amount < 0) return C.ERR_INVALID_ARGS; // :500-502
    if (!(C.RESOURCES_ALL || []).includes(resourceType)) return C.ERR_INVALID_ARGS; // :503-505
    // 目标门 :507-516：store 面在 data() 读——未知 id 在此抛官方同文案错误
    // （rawData 的求值点 = 官方第三操作数，早于 assertTargetObject）
    if (!target || !target.id) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const rawTarget = gw.rawData(target.id);
    const structEntry = structureRegisterEntry(target.id);
    if (!rawTarget.store
      || ((!structEntry || !(target instanceof globalThis.Structure))
        && !(target instanceof globalThis.Tombstone)
        && !(target instanceof globalThis.Ruin))) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    // :518-521：terminal 被 PWR_DISRUPT_TERMINAL 封锁拒（M1 投影无 effects 键
    // 恒放行；power 批接线后生效——effects 形态按 wrapper 面 {power,
    // ticksRemaining} 读）
    if (structureKindOf(rawTarget) === 'terminal') {
      const effect = (rawTarget.effects || []).find((e) => e && e.power === C.PWR_DISRUPT_TERMINAL);
      if (effect && effect.ticksRemaining > 0) return C.ERR_INVALID_TARGET;
    }
    // :525-527：他人 rampart 护格（lookFor 现桩恒 []——调用形态保留，真面随
    // room.lookFor 批；执行器侧同缺，双侧一致的现状缺口，登记报告）
    if (target.my === false
      && target.pos.lookFor('structure').some(
        (i) => i.structureType === C.STRUCTURE_RAMPART && !i.my && !i.isPublic)) {
      return C.ERR_NOT_OWNER;
    }
    // :528-530：他人 safeMode 生效期拒（this.room 官方直解引用；引擎态房间恒
    // 在投影，手写世界缺房时按既有 blockedBySafeMode 同款守卫——登记灰区）
    const ctl = this.room && this.room.controller;
    if (ctl && !ctl.my && ctl.safeMode) return C.ERR_NOT_OWNER;
    // :532-534：nuker/powerBank 不可提取
    if (structEntry && (structureKindOf(structEntry) === C.STRUCTURE_NUKER
      || structureKindOf(structEntry) === C.STRUCTURE_POWER_BANK)) {
      return C.ERR_INVALID_TARGET;
    }
    // :536-538：容量 0 且目标无该资源且非 Tombstone 拒
    if (!capacityForResource(rawTarget, resourceType) && !rawTarget.store[resourceType]
      && !(target instanceof globalThis.Tombstone)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE; // :540-542
    // :544-545：actor 空容量（storeCapacity − calcResources——raw 面等价）
    const emptySpace = rawStoreCapacity(gw.rawData(this.id)) - sumStore(gw.rawData(this.id));
    if (emptySpace <= 0) return C.ERR_FULL; // :546-548
    if (!amount) amount = Math.min(emptySpace, rawTarget.store[resourceType]); // :550-552
    if (amount > emptySpace) return C.ERR_FULL; // :554-556
    if (!amount || (rawTarget.store[resourceType] || 0) < amount) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :558-560
    }
    gw.intents.set(this.id, 'withdraw', { id: target.id, amount, resourceType }); // :562
    return C.OK;
  };

  /** Creep.pickup：官方 creeps.js:566-593——掉落 Resource 目标（register.energy
   *  面 = 掉落资源全集）+ actor 空容量 + 邻接（isNearTo ≤1 格）。 */
  globalThis.Creep.prototype.pickup = function pickup(target) {
    if (!this.my) return C.ERR_NOT_OWNER; // :568-570
    if (this.spawning) return C.ERR_BUSY; // :571-573
    // 目标门 :574-577（register.energy 的投影等价 = raw type 'resource'）
    const rawTarget = target ? gw.rawById.get(target.id) : undefined;
    if (!target || !target.id || !rawTarget || rawTarget.type !== 'resource'
      || !(target instanceof globalThis.Energy)) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    // :579-580：满仓拒（calcResources >= storeCapacity——raw 面等价）
    const rawSelf = gw.rawData(this.id);
    if (sumStore(rawSelf) >= rawStoreCapacity(rawSelf)) return C.ERR_FULL; // :578-580
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE; // :581-583
    gw.intents.set(this.id, 'pickup', { id: target.id }); // :585
    return C.OK;
  };

  // 未换真的方法保持桩（stub_calls 计数，A4 批次清偿）——挂原型与真方法同层
  for (const m of CREEP_METHODS) {
    if (!(m in globalThis.Creep.prototype)) {
      globalThis.Creep.prototype[m] = gw.mkStubMethod('creep.' + m);
    }
  }

  // ---------------------------------------------------------------- creep 装配
  // A3b：字段/方法全在 Creep.prototype，装配只确认壳进 objectsById（wrapObject
  // 的 base {id,pos,room} 即官方构造器的 own 定格面）
  gw.assemblers.creep = (o, base) => base;
})();
