/* 定义 Creep 字段、采集与战斗方法，装配移动 API 并记录通过预检的玩家指令。
 * getter 的缓存与当前 tick 回查由 registry.js 管理；资源和控制器方法续于 creep-resources.js。
 * 移动参数、路径解码与 _move 记忆缓存归 creep/movement.js，入口显式维持安装阶段。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  const _ = globalThis.__kkp.lodash;
  const { assertTargetObject, isKnownCreep, isKnownStructure } = globalThis.__kkp.targets;
  const { createCreepMovementApi } = globalThis.__kkp;
  const { installPathCodec, installMethods: installMovementMethods } = createCreepMovementApi();
  // 路径静态方法仍在常量捕获、字段和动作注册之前安装。
  installPathCodec();

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
    // 远程攻击所需的部件类型。
    RANGED_ATTACK: globalThis.RANGED_ATTACK,
    HEAL: globalThis.HEAL,
    CLAIM: globalThis.CLAIM,
    // 单目标战斗动作射程（规则 builtin:creep.*.range 的生效值）。
    ATTACK_RANGE: globalThis.ATTACK_RANGE,
    RANGED_ATTACK_RANGE: globalThis.RANGED_ATTACK_RANGE,
    HEAL_RANGE: globalThis.HEAL_RANGE,
    RANGED_HEAL_RANGE: globalThis.RANGED_HEAL_RANGE,
    // 移动玩家门（规则 builtin:creep.move.* 的生效值）：move()/moveTo()
    // 的疲劳/MOVE 预检与结算仲裁原生准入同源。
    MOVE_FATIGUE_LIMIT: globalThis.MOVE_FATIGUE_LIMIT,
    MOVE_MIN_ACTIVE_MOVES: globalThis.MOVE_MIN_ACTIVE_MOVES,
    ERR_NOT_FOUND: globalThis.ERR_NOT_FOUND,
    ERR_RCL_NOT_ENOUGH: globalThis.ERR_RCL_NOT_ENOUGH,
    ERR_FULL: globalThis.ERR_FULL,
    ERR_NO_PATH: globalThis.ERR_NO_PATH,
    ERR_GCL_NOT_ENOUGH: globalThis.ERR_GCL_NOT_ENOUGH,
    // 安全模式消耗与 GCL 点数计算常量。
    SAFE_MODE_COST: globalThis.SAFE_MODE_COST,
    RESOURCE_GHODIUM: globalThis.RESOURCE_GHODIUM,
    GCL_MULTIPLY: globalThis.GCL_MULTIPLY,
    GCL_POW: globalThis.GCL_POW,
    ERR_NO_PATH: globalThis.ERR_NO_PATH,
    OBSTACLE_OBJECT_TYPES: freezeConst(globalThis.OBSTACLE_OBJECT_TYPES),
    CONSTRUCTION_COST: freezeConst(globalThis.CONSTRUCTION_COST),
    // 资源白名单、容量表与结构类型常量。
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
  gw.__creepC = C; // creep-resources.js 复用此常量表。

  // 官方 _hasActiveBodypart（game/creeps.js:9-17 的等价物）：hits>0 的指定部件数。
  // falsy body（powerCreep 委托 move/moveTo 的路径）官方恒真，返回 Infinity
  // 使「数量 ≥ 门槛」类门全部通过。
  function countActiveParts(body, type) {
    if (!body) return Infinity;
    let n = 0;
    for (const p of body) if (p.type === type && p.hits > 0) n += 1;
    return n;
  }

  // 玩家 API 通过预检后仅记录指令，实际动作由 Rust 结算执行。

  /** 官方 `controller && !controller.my && controller.safeMode` 判定
   *  （attack/heal/rangedHeal/dismantle 的 API 层门，creeps.js:602/699/723/1045）。 */
  function blockedBySafeMode(self) {
    const ctl = self.room && self.room.controller;
    return !!(ctl && !ctl.my && ctl.safeMode);
  }

  // 资源与控制器方法共用这两个检查函数。
  gw.countActiveParts = countActiveParts;
  gw.blockedBySafeMode = blockedBySafeMode;

  function carriedEnergy(self) {
    return (self.store && self.store.energy) || 0;
  }

  // 字段读取当前 tick 数据：body 字符串部件补 hits=100，store/carry 由 makeStore 构造。
  // 说话内容由 say/sayPublic 决定可见性，只有本人或公开消息可见。
  // 来源：上游 engine/src/game/creeps.js 的属性定义。
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
    // 本人读取说话内容，其他查看者只读取公开消息；say/sayPublic 来自世界数据。
    saying: (o) => {
      if (o.say == null) return undefined;
      if (o.owner === gw.viewerName) return o.say;
      return o.sayPublic ? o.say : undefined;
    },
  });

  // 官方 creeps.js:94-120：memory 是独立 getter/setter（不走 _${name} 缓存），
  // 每次访问重读 `Memory.creeps[name]`（gw.memoryFor 官方同形——含 'undefined'
  // 字符串病态检查与非对象分支），非本人 creep 读 undefined、写抛错
  Object.defineProperty(globalThis.Creep.prototype, 'memory', {
    get() {
      if (this.id && !this.my) return undefined;
      return gw.memoryFor('creeps', this.name);
    },
    set(value) {
      if (this.id && !this.my) {
        throw new Error("Could not set other player's creep memory");
      }
      gw.setMemoryFor('creeps', this.name, value, 'creep');
    },
  });

  // ---------------------------------------------------------------- Creep 原型：动作方法
  installMovementMethods({ C, countActiveParts, assertTargetObject });

  /** 支持 Source 和 Mineral；矿物还检查同格 extractor 的归属、启用状态与冷却。
   * 不支持 Deposit 分支。来源：上游 engine/src/game/creeps.js 的 harvest。 */
  globalThis.Creep.prototype.harvest = function harvest(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    if (!target || !target.id) return C.ERR_INVALID_TARGET;
    if (target instanceof globalThis.Mineral) {
      // 矿物采集检查数量、邻接与提取器的归属、启用和冷却；deposit 不支持。
      if (!target.mineralAmount) return C.ERR_NOT_ENOUGH_RESOURCES;
      if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
      let extractor = null;
      const objs = gw.world.objects || [];
      for (let i = 0; i < objs.length; i++) {
        const o = objs[i];
        if (
          o.room === target.pos.roomName &&
          o.x === target.pos.x &&
          o.y === target.pos.y &&
          o.structureType === 'extractor'
        ) {
          extractor = gw.objectsById.get(o.id);
          break;
        }
      }
      if (!extractor) return C.ERR_NOT_FOUND;
      if (extractor.owner && !extractor.my) return C.ERR_NOT_OWNER;
      if (!extractor.isActive()) {
        return C.ERR_RCL_NOT_ENOUGH;
      }
      if (extractor.cooldown) return C.ERR_TIRED;
      gw.intents.set(this.id, 'harvest', { id: target.id });
      return C.OK;
    }
    if (!(target instanceof globalThis.Source)) {
      // 当前实现对非 Source/Mineral 目标返回 INVALID_TARGET，不抛序列化对象错误。
      return C.ERR_INVALID_TARGET;
    }
    if (!target.energy) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    const ctl = this.room && this.room.controller;
    if (ctl && ctl.owner && ctl.owner.username !== this.owner.username) {
      return C.ERR_NOT_OWNER; // 当前只检查房间所有者，未检查 controller.reservation。
    }
    gw.intents.set(this.id, 'harvest', { id: target.id });
    return C.OK;
  };

  /** 官方 creeps.js:613/649/1048：attack/rangedAttack/dismantle 对带
   *  FORTIFY/EFFECT_INVULNERABILITY 活跃效果的目标预检即拒
   *  ERR_INVALID_TARGET（ticksRemaining > 0；SHIELD 不在预检门）。 */
  function invulnerableTarget(target) {
    const effects = target && target.effects;
    if (!effects) return false;
    for (let i = 0; i < effects.length; i++) {
      const e = effects[i];
      if (
        e &&
        (e.power === globalThis.PWR_FORTIFY ||
          e.effect === globalThis.EFFECT_INVULNERABILITY) &&
        e.ticksRemaining > 0
      ) {
        return true;
      }
    }
    return false;
  }

  /** Mod 耐久对象目标族（G09.b）：带 hits 信封的 Mod 记录经其玩家类绑定
   * 接收；信封在场与结算 object_hits>0 门同源（无信封类型结算静默不动，
   * 预检即拒 -7）。attack/rangedAttack 与塔 attack 共用同一判别。 */
  function modDurabilityTargetOk(target) {
    const raw = target && target.id !== undefined ? gw.rawById.get(target.id) : undefined;
    const modEntry =
      raw !== undefined &&
      raw.type === 'mod' &&
      gw.modApi !== undefined &&
      gw.modApi.types !== undefined
        ? gw.modApi.types[raw.modType]
        : undefined;
    return modEntry !== undefined && target instanceof modEntry.cls && raw.hits !== undefined;
  }

  gw.modDurabilityTargetOk = modDurabilityTargetOk;

  /** 检查 ATTACK 部件、目标存活与类型、FORTIFY/INVULNERABILITY 效果门、
   * ATTACK_RANGE 射程及敌方安全模式。
   * 射程读常量表（规则 builtin:creep.attack.range 注入的生效值），
   * 与 Rust 结算同源。来源：上游 engine/src/game/creeps.js 的 attack。 */
  globalThis.Creep.prototype.attack = function attack(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.ATTACK) === 0) return C.ERR_NO_BODYPART;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    // 目标须在当前 tick 的原始数据中仍存在，并通过后面的实例类型检查。
    const modTargetOk = modDurabilityTargetOk(target);
    const okTarget =
      target &&
      target.id &&
      (isKnownCreep(target.id) ||
        isKnownStructure(target.id) ||
        modTargetOk ||
        target instanceof globalThis.PowerCreep) &&
      (target instanceof globalThis.Creep ||
        target instanceof globalThis.PowerCreep ||
        target instanceof globalThis.StructureSpawn ||
        target instanceof globalThis.Structure ||
        modTargetOk);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (invulnerableTarget(target)) return C.ERR_INVALID_TARGET;
    if (!target.pos.inRangeTo(this.pos, C.ATTACK_RANGE)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'attack', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 官方 build（creeps.js:762-808）：WORK + 能量 + 工地目标 + 3 格 +
   *  目标格静态障碍/挡路 creep 预检（safeMode 敌我分支 :794-805）。 */
  globalThis.Creep.prototype.build = function build(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    // Mod 建筑的成本资源集可不含 energy：任一成本资源有持有即可通过
    // 预检，支付比例由结算按配方定论。
    const modBuilding =
      gw.modApi === undefined ? undefined : gw.modApi.buildings[target && target.structureType];
    if (modBuilding === undefined && !carriedEnergy(this)) {
      return C.ERR_NOT_ENOUGH_RESOURCES;
    }
    if (modBuilding !== undefined) {
      const hasInput = modBuilding.cost.some(function (resource) {
        return ((this.store && this.store[resource]) || 0) > 0;
      }, this);
      if (!hasInput) return C.ERR_NOT_ENOUGH_RESOURCES;
    }
    if (!(target && target.id && target instanceof globalThis.ConstructionSite)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!this.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
    const siteIsObstacle =
      (C.OBSTACLE_OBJECT_TYPES || []).includes(target.structureType) ||
      (modBuilding !== undefined && modBuilding.pathing === 'obstacle');
    if (siteIsObstacle) {
      let objectsInTile = false;
      let creepsInTile = false;
      let myCreepsInTile = false;
      const objs = gw.world.objects || [];
      for (let i = 0; i < objs.length; i++) {
        const o = objs[i];
        if (o.room !== target.pos.roomName || o.x !== target.pos.x || o.y !== target.pos.y)
          continue;
        const t =
          o.type === 'structure' || o.type === 'spawn' ? o.structureType || 'spawn' : o.type;
        const entry = gw.modApi === undefined ? undefined : gw.modApi.buildings[t];
        const obstacleType =
          (C.OBSTACLE_OBJECT_TYPES || []).includes(t) ||
          (entry !== undefined && entry.pathing === 'obstacle' && o.type === 'structure');
        if (!obstacleType) continue;
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
    const okTarget =
      target &&
      target.id &&
      isKnownStructure(target.id) &&
      (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!this.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'repair', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 检查 WORK 部件、可拆结构、邻接、安全模式与 FORTIFY/INVULNERABILITY
   * 效果门。来源：上游 engine/src/game/creeps.js 的 dismantle。 */
  globalThis.Creep.prototype.dismantle = function dismantle(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    // 可拆类型 = 内建造价表或 Mod 建筑目录（与结算侧 cost_bearing 同源）。
    const modBuilding =
      gw.modApi === undefined ? undefined : gw.modApi.buildings[target && target.structureType];
    const okTarget =
      target &&
      target.id &&
      (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn) &&
      ((C.CONSTRUCTION_COST && target.structureType in C.CONSTRUCTION_COST) ||
        modBuilding !== undefined);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    if (invulnerableTarget(target)) return C.ERR_INVALID_TARGET;
    gw.intents.set(this.id, 'dismantle', { id: target.id });
    return C.OK;
  };

  /** 官方 heal（creeps.js:678-705）：HEAL + Creep 目标 + HEAL_RANGE 射程 + safeMode。 */
  globalThis.Creep.prototype.heal = function heal(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
    // 官方门（creeps.js:687-690/715-718）接受 creep 与 powerCreep 双类型。
    if (
      !(
        target &&
        target.id &&
        (isKnownCreep(target.id) || target instanceof globalThis.PowerCreep) &&
        (target instanceof globalThis.Creep || target instanceof globalThis.PowerCreep)
      )
    ) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.inRangeTo(this.pos, C.HEAL_RANGE)) return C.ERR_NOT_IN_RANGE;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    gw.intents.set(this.id, 'heal', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 官方 getActiveBodyparts（creeps.js:589-592 → 私有 _getActiveBodyparts
   *  :9-18）：从 body 尾部向前计数，**遇首个 hits≤0 部件即断**（死亡分布保证
   *  损毁件成前缀——与 countActiveParts 的全量计数在不变量下等价；夹具手搓
   *  的洞形 body 上本 API 保持官方 break 语义）。 */
  globalThis.Creep.prototype.getActiveBodyparts = function getActiveBodyparts(type) {
    let count = 0;
    const body = this.body || [];
    for (let i = body.length - 1; i >= 0; i--) {
      if (body[i].hits <= 0) break;
      if (body[i].type === type) count += 1;
    }
    return count;
  };

  /** 检查 RANGED_ATTACK、当前目标类型、RANGED_ATTACK_RANGE 射程、敌方
   * 安全模式与 FORTIFY/INVULNERABILITY 效果门（官方 range 在效果门之前），
   * 再记录指令；rampart 重定向由 Rust 结算处理。
   * 来源：上游 engine/src/game/creeps.js 的 rangedAttack。 */
  globalThis.Creep.prototype.rangedAttack = function rangedAttack(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.RANGED_ATTACK) === 0) return C.ERR_NO_BODYPART;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    // Mod 耐久对象目标族（G09.b）与近战同款。
    const modTargetOk = modDurabilityTargetOk(target);
    const okTarget =
      target &&
      target.id &&
      (isKnownCreep(target.id) ||
        isKnownStructure(target.id) ||
        modTargetOk ||
        target instanceof globalThis.PowerCreep) &&
      (target instanceof globalThis.Creep ||
        target instanceof globalThis.PowerCreep ||
        target instanceof globalThis.StructureSpawn ||
        target instanceof globalThis.Structure ||
        modTargetOk);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!this.pos.inRangeTo(target, C.RANGED_ATTACK_RANGE)) return C.ERR_NOT_IN_RANGE;
    // 官方 creeps.js:645-652：range 检查在效果门之前（超射程且无敌 →
    // ERR_NOT_IN_RANGE，与 attack 的顺序相反）。
    if (invulnerableTarget(target)) return C.ERR_INVALID_TARGET;
    gw.intents.set(this.id, 'rangedAttack', { id: target.id });
    return C.OK;
  };

  /** 官方 rangedMassAttack（creeps.js:658-677）：无目标——RANGED_ATTACK 部件
   *  + safeMode 门后即记 intent；目标收集/距离衰减/rampart 跳过全在结算侧
   *  （rangedMassAttack.js）。 */
  globalThis.Creep.prototype.rangedMassAttack = function rangedMassAttack() {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.RANGED_ATTACK) === 0) return C.ERR_NO_BODYPART;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    gw.intents.set(this.id, 'rangedMassAttack', {});
    return C.OK;
  };

  /** 官方 cancelOrder（creeps.js:1016-1022）：按动作名移除已记录的 intent，
   *  成功返回 OK（收集器退费），未记录返回 ERR_NOT_FOUND。 */
  globalThis.Creep.prototype.cancelOrder = function cancelOrder(name) {
    if (gw.intents.remove(this.id, name)) return C.OK;
    return C.ERR_NOT_FOUND;
  };

  /** 官方 rangedHeal（creeps.js:706-733）：HEAL + Creep 目标 + RANGED_HEAL_RANGE 射程 + safeMode。 */
  globalThis.Creep.prototype.rangedHeal = function rangedHeal(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
    // 官方门（creeps.js:687-690/715-718）接受 creep 与 powerCreep 双类型。
    if (
      !(
        target &&
        target.id &&
        (isKnownCreep(target.id) || target instanceof globalThis.PowerCreep) &&
        (target instanceof globalThis.Creep || target instanceof globalThis.PowerCreep)
      )
    ) {
      return C.ERR_INVALID_TARGET;
    }
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    if (!this.pos.inRangeTo(target.pos, C.RANGED_HEAL_RANGE)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'rangedHeal', { id: target.id });
    return C.OK;
  };
})();
