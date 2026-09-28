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
    OBSTACLE_OBJECT_TYPES: freezeConst(globalThis.OBSTACLE_OBJECT_TYPES),
    CONSTRUCTION_COST: freezeConst(globalThis.CONSTRUCTION_COST),
    CONTROLLER_STRUCTURES: freezeConst(globalThis.CONTROLLER_STRUCTURES),
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
