/* PowerCreep 玩家面：getter 族、生命周期/战斗/资源方法与 PowerCreep.create。
 * 账号注册表经 world.power_account 注入（Rust 侧与房间对象投影同源），
 * 已生成侧记录经 wrapObject 进入 objectsById；两者按官方 data(id) 的
 * Object.assign({} ,账号, 房间) 合并语义读取。
 * 来源：上游 engine/src/game/power-creeps.js。 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  const __Object = Object;
  const __objectAssign = Object.assign;
  const __mapGet = Map.prototype.get;
  const __reflectApply = Reflect.apply;
  const mathMax = Math.max;

  // 对象型常量是全局对象的共享引用——冻结内容防玩家篡改 API 判定面
  //（creep.js 的 freezeConst 纪律同款；官方读私有 driver.constants）。
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
    ERR_INVALID_TARGET: globalThis.ERR_INVALID_TARGET,
    ERR_INVALID_ARGS: globalThis.ERR_INVALID_ARGS,
    ERR_NOT_IN_RANGE: globalThis.ERR_NOT_IN_RANGE,
    ERR_NOT_ENOUGH_RESOURCES: globalThis.ERR_NOT_ENOUGH_RESOURCES,
    ERR_NAME_EXISTS: globalThis.ERR_NAME_EXISTS,
    ERR_FULL: globalThis.ERR_FULL,
    ERR_TIRED: globalThis.ERR_TIRED,
    ERR_NO_BODYPART: globalThis.ERR_NO_BODYPART,
    ERR_NOT_FOUND: globalThis.ERR_NOT_FOUND,
    ERR_RCL_NOT_ENOUGH: globalThis.ERR_RCL_NOT_ENOUGH,
    POWER_CLASS: freezeConst(globalThis.POWER_CLASS),
    POWER_INFO: freezeConst(globalThis.POWER_INFO),
    POWER_CREEP_MAX_LEVEL: globalThis.POWER_CREEP_MAX_LEVEL,
    POWER_LEVEL_MULTIPLY: globalThis.POWER_LEVEL_MULTIPLY,
    POWER_LEVEL_POW: globalThis.POWER_LEVEL_POW,
  };
  const { assertTargetObject, isKnownStructure } = globalThis.__kkp.targets;

  // 官方 data(id)：账号记录与房间对象合并（房间侧键覆盖同名账号键；
  // 未生成/已消亡时仅账号面）。rawById 查不到不抛——账号面仍可读。
  gw.pcData = function pcData(id) {
    const account =
      gw.world.power_account &&
      gw.world.power_account.powerCreeps &&
      gw.world.power_account.powerCreeps[id];
    let room = null;
    const raw = __reflectApply(__mapGet, gw.rawById, [id]);
    if (raw !== undefined) room = raw;
    if (!account && !room) {
      throw new Error('Could not find an object with ID ' + id);
    }
    return __objectAssign({}, account || {}, room || {});
  };

  // 账号配额（官方 calcFreePowerLevels）：GPL 等级 −（记录数+等级和+已耗）。
  function calcFreePowerLevels() {
    const account = gw.world.power_account || {};
    const creeps = account.powerCreeps || {};
    let used = 0;
    for (const key in creeps) {
      used += 1 + (creeps[key].level || 0);
    }
    const level = mathMax(
      0,
      Math.floor(Math.pow((account.power || 0) / C.POWER_LEVEL_MULTIPLY, 1 / C.POWER_LEVEL_POW)),
    );
    return level - used;
  }
  gw.calcFreePowerLevels = calcFreePowerLevels;

  // ---------------------------------------------------------------- getter 面
  // 官方 power-creeps.js:45-73 的属性定义；未生成侧无房间键（undefined 形态
  // 由各 getter 自带缺省表达）。ticksToLive 对无 ageTime（未生成）返回
  // undefined——官方 NaN 分支按套件契约折 undefined。
  gw.defineGameObjectProperties(globalThis.PowerCreep.prototype, gw.pcData, {
    name: (o) => o.name,
    my: (o) => o.owner === gw.viewerName,
    owner: (o) => ({ username: o.owner || 'unknown' }),
    level: (o) => o.level,
    className: (o) => o.className,
    hitsMax: (o) => o.hitsMax,
    hits: (o) => o.hits,
    shard: (o) => o.shard || undefined,
    spawnCooldownTime: (o) => {
      const now = gw.world.power_account ? gw.world.power_account.now : 0;
      return o.spawnCooldownTime !== null &&
        o.spawnCooldownTime !== undefined &&
        o.spawnCooldownTime > now
        ? o.spawnCooldownTime
        : undefined;
    },
    deleteTime: (o) => o.deleteTime || undefined,
    powers: (o) => {
      const out = {};
      const powers = o.powers || {};
      for (const key in powers) {
        const entry = powers[key];
        out[key] = {
          level: entry.level,
          cooldown: mathMax(0, (entry.cooldownTime || 0) - gw.world.time),
        };
      }
      return out;
    },
    saying: (o) => {
      const say = o.actionLog && o.actionLog.say;
      if (!say) return undefined;
      if (o.owner === gw.viewerName) return say.message;
      return say.isPublic ? say.message : undefined;
    },
    carry: (o) => gw.makeStore(o),
    store: (o) => gw.makeStore(o),
    carryCapacity: (o) => o.storeCapacity,
    ticksToLive: (o) => (o.ageTime === undefined ? undefined : o.ageTime - gw.world.time),
  });

  // memory（官方独立 getter/setter，不走 defineGameObjectProperties 缓存）：
  // 非本人读 undefined、写抛错。
  Object.defineProperty(globalThis.PowerCreep.prototype, 'memory', {
    get() {
      if (this.id && !this.my) return undefined;
      return gw.memoryFor('powerCreeps', this.name);
    },
    set(value) {
      if (this.id && !this.my) {
        throw new Error("Could not set other player's creep memory");
      }
      // 官方 pc.js:95-99：Memory.powerCreeps[name] = value；错误文案与
      // creep 面同串（'Could not set creep memory'）。
      return gw.setMemoryFor('powerCreeps', this.name, value, 'creep');
    },
  });

  globalThis.PowerCreep.prototype.toString = function toString() {
    return `[powerCreep ${this.name}]`;
  };

  // ---------------------------------------------------------------- 委托方法
  // 官方 move/moveTo/moveByPath/transfer/withdraw/drop/pickup/say 均为
  // 「未生成 BUSY + Creep 原型转发」（power-creeps.js:106-160）。kk 的
  // Creep 原型方法以 this.my/spawning/store 面读写，powerCreep 的 getter
  // 面与房间记录与之同构；countActiveParts 对 falsy body 恒过（官方
  // _hasActiveBodypart 的 !body 支）。
  function delegateToCreep(name) {
    return function delegated(...args) {
      if (!this.room) return C.ERR_BUSY;
      return globalThis.Creep.prototype[name].apply(this, args);
    };
  }
  for (const name of [
    'move',
    'moveTo',
    'moveByPath',
    'transfer',
    'withdraw',
    'drop',
    'pickup',
    'say',
  ]) {
    globalThis.PowerCreep.prototype[name] = delegateToCreep(name);
  }

  // ---------------------------------------------------------------- 生命周期
  /** 官方 spawn（power-creeps.js:162-189）：已生成 BUSY → 类型门 → 归属 →
   * RCL（checkStructureAgainstController 的 isActive 等价）→ 冷却门 →
   * 全局意图。 */
  globalThis.PowerCreep.prototype.spawn = function spawn(powerSpawn) {
    if (this.room) return C.ERR_BUSY;
    if (!(powerSpawn instanceof globalThis.StructurePowerSpawn)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!this.my || !powerSpawn.my) return C.ERR_NOT_OWNER;
    if (!powerSpawn.isActive()) return C.ERR_RCL_NOT_ENOUGH;
    if (this.spawnCooldownTime) return C.ERR_TIRED;
    // 官方 pc.js:180-185：提交前确保 Memory.powerCreeps[name] 存在（该
    // 写入随 Memory 持久化，后续遍历 Memory.powerCreeps 可见）。Memory 为
    // 原始值时官方 sloppy 赋值静默无效（memoryFor 同款防御）。
    const m = globalThis.Memory;
    if (m !== null && (typeof m === 'object' || typeof m === 'function')) {
      if (m.powerCreeps === undefined) m.powerCreeps = {};
      if (
        typeof m.powerCreeps === 'object' &&
        m.powerCreeps !== null &&
        m.powerCreeps[this.name] === undefined
      ) {
        m.powerCreeps[this.name] = {};
      }
    }
    gw.intents.pushByName('global', 'spawnPowerCreep', { id: powerSpawn.id, name: this.name }, 50);
    return C.OK;
  };

  /** 官方 suicide（:191-202）：已生成 BUSY → 归属 → 全局意图。 */
  globalThis.PowerCreep.prototype.suicide = function suicide() {
    if (!this.room) return C.ERR_BUSY;
    if (!this.my) return C.ERR_NOT_OWNER;
    gw.intents.pushByName('global', 'suicidePowerCreep', { id: this.id }, 50);
    return C.OK;
  };

  /** 官方 delete（:204-215）：已生成 BUSY → 归属 → 全局意图。 */
  globalThis.PowerCreep.prototype.delete = function deletePowerCreep(cancel) {
    if (this.room) return C.ERR_BUSY;
    if (!this.my) return C.ERR_NOT_OWNER;
    gw.intents.pushByName('global', 'deletePowerCreep', { id: this.id, cancel }, 50);
    return C.OK;
  };

  /** 官方 rename（:356-373）：已生成 BUSY → 归属 → 名称校验（≤100 字符）
   * → 全局重名 → 全局意图。 */
  globalThis.PowerCreep.prototype.rename = function rename(name) {
    if (this.room) return C.ERR_BUSY;
    if (!this.my) return C.ERR_NOT_OWNER;
    if (!name || typeof name !== 'string' || name.length > 100) {
      return C.ERR_INVALID_ARGS;
    }
    const creeps = (gw.world.power_account && gw.world.power_account.powerCreeps) || {};
    for (const key in creeps) {
      if (creeps[key].name === name) return C.ERR_NAME_EXISTS;
    }
    gw.intents.pushByName('global', 'renamePowerCreep', { id: this.id, name }, 50);
    return C.OK;
  };

  /** 官方 upgrade（:217-244）：归属 → 配额 → 等级上限 → 类匹配 → power
   * 等级门槛（level >= POWER_INFO[power].level[当前 power 等级]）。 */
  globalThis.PowerCreep.prototype.upgrade = function upgrade(power) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (calcFreePowerLevels() <= 0) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (this.level >= C.POWER_CREEP_MAX_LEVEL) return C.ERR_FULL;
    const powerInfo = C.POWER_INFO[power];
    if (!powerInfo || powerInfo.className !== this.className) {
      return C.ERR_INVALID_ARGS;
    }
    const data = gw.pcData(this.id);
    const powerData = data.powers && data.powers[power];
    const powerLevel = powerData ? powerData.level : 0;
    if (powerLevel === 5) return C.ERR_FULL;
    if (this.level < powerInfo.level[powerLevel]) return C.ERR_FULL;
    gw.intents.pushByName('global', 'upgradePowerCreep', { id: this.id, power }, 50);
    return C.OK;
  };

  /** 官方 usePower（:246-293）：归属 → 生成态 → 房主 powerEnabled/安全
   * 模式 → power 等级 → 冷却 → ops → 射程与效果覆盖。 */
  globalThis.PowerCreep.prototype.usePower = function usePower(power, target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (!this.room) return C.ERR_BUSY;
    if (this.room.controller) {
      if (!this.room.controller.isPowerEnabled) return C.ERR_INVALID_ARGS;
      if (!this.room.controller.my && this.room.controller.safeMode) {
        return C.ERR_INVALID_ARGS;
      }
    }
    const data = gw.pcData(this.id);
    const powerData = data.powers && data.powers[power];
    const powerInfo = C.POWER_INFO[power];
    if (!powerData || !powerData.level || !powerInfo) return C.ERR_NO_BODYPART;
    if (powerData.cooldownTime > gw.world.time) return C.ERR_TIRED;
    let ops = powerInfo.ops || 0;
    if (Array.isArray(ops)) ops = ops[powerData.level - 1];
    if (((data.store && data.store.ops) || 0) < ops) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (powerInfo.range) {
      if (!target) return C.ERR_INVALID_TARGET;
      if (!this.pos.inRangeTo(target, powerInfo.range)) return C.ERR_NOT_IN_RANGE;
      const currentEffect = (target.effects || []).find((effect) => effect.power === power);
      if (
        currentEffect &&
        currentEffect.level > powerData.level &&
        currentEffect.ticksRemaining > 0
      ) {
        return C.ERR_FULL;
      }
    }
    gw.intents.set(this.id, 'usePower', { power, id: target ? target.id : undefined });
    return C.OK;
  };

  /** 官方 enableRoom（:295-317）：归属 → 生成态 → 结构目标门 → 相邻 →
   * 控制器类型与安全模式门。 */
  globalThis.PowerCreep.prototype.enableRoom = function enableRoom(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (!this.room) return C.ERR_BUSY;
    if (
      !target ||
      !target.id ||
      !isKnownStructure(target.id) ||
      !(target instanceof globalThis.Structure)
    ) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (target.structureType !== 'controller' || (target.safeMode && !target.my)) {
      return C.ERR_INVALID_TARGET;
    }
    gw.intents.set(this.id, 'enableRoom', { id: target.id });
    return C.OK;
  };

  /** 官方 renew（:319-340）：归属 → 生成态 → powerBank/powerSpawn 目标门
   * → RCL（powerSpawn）→ 相邻。 */
  globalThis.PowerCreep.prototype.renew = function renew(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (!this.room) return C.ERR_BUSY;
    if (
      !target ||
      !target.id ||
      !(
        target instanceof globalThis.StructurePowerBank ||
        target instanceof globalThis.StructurePowerSpawn
      )
    ) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (target instanceof globalThis.StructurePowerSpawn && !target.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'renew', { id: target.id });
    return C.OK;
  };

  /** 官方 cancelOrder（:342-354）。 */
  globalThis.PowerCreep.prototype.cancelOrder = function cancelOrder(name) {
    if (!this.room) return C.ERR_BUSY;
    if (!this.my) return C.ERR_NOT_OWNER;
    if (gw.intents.remove(this.id, name)) return C.OK;
    return C.ERR_NOT_FOUND;
  };

  /** 官方 notifyWhenAttacked（:375-393）：仅状态变化时登记指令。 */
  globalThis.PowerCreep.prototype.notifyWhenAttacked = function notifyWhenAttacked(enabled) {
    if (!this.room) return C.ERR_BUSY;
    if (!this.my) return C.ERR_NOT_OWNER;
    if (typeof enabled !== 'boolean') return C.ERR_INVALID_ARGS;
    if (enabled !== !!gw.pcData(this.id).notifyWhenAttacked) {
      gw.intents.set(this.id, 'notifyWhenAttacked', { enabled });
    }
    return C.OK;
  };

  /** 官方 PowerCreep.create（:395-410）：名称（≤100 字符）→ 配额 → 重名
   * → 类白名单 → 全局意图。 */
  globalThis.PowerCreep.create = function create(name, className) {
    if (!name || typeof name !== 'string' || name.length > 100) {
      return C.ERR_INVALID_ARGS;
    }
    if (calcFreePowerLevels() <= 0) return C.ERR_NOT_ENOUGH_RESOURCES;
    const creeps = (gw.world.power_account && gw.world.power_account.powerCreeps) || {};
    for (const key in creeps) {
      if (creeps[key].name === name) return C.ERR_NAME_EXISTS;
    }
    if (Object.values(C.POWER_CLASS).indexOf(className) === -1) {
      return C.ERR_INVALID_ARGS;
    }
    gw.intents.pushByName('global', 'createPowerCreep', { name, className }, 50);
    return C.OK;
  };

  // ---------------------------------------------------------------- 装配
  gw.assemblers.powerCreep = (o, base) => base;
})();
