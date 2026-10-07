/* 定义 Creep 字段、移动、采集与战斗方法，并记录通过预检的玩家指令。
 * getter 的缓存与当前 tick 回查由 _registry.js 管理；资源和控制器方法续于 creep_resources.js。
 * moveTo 走 findPathTo 真路径并维护官方 _move 记忆缓存（结构代价归回调文档）。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  const _ = globalThis.__kkp.lodash;
  // room.js（加载序在本段之前）挂载的官方序列化器；捕获防玩家替换
  // globalThis.Room 后影响 moveTo 路径缓存。
  const roomSerializePath = globalThis.Room.serializePath;

  // 官方 utils.js:80-100 offsetsByDirection 表（deserializePath 的方向位移源）
  const OFFSETS_BY_DIRECTION = {
    1: [0, -1], 2: [1, -1], 3: [1, 0], 4: [1, 1],
    5: [0, 1], 6: [-1, 1], 7: [-1, 0], 8: [-1, -1],
  };

  /** 官方 `_.isObject`（creeps.js:172 的 moveTo opts 归位门）：对象与函数
   *  都算（null 除外）。 */
  function isObjectLike(v) {
    return v !== null && (typeof v === 'object' || typeof v === 'function');
  }

  /** 官方 utils.js:34-63 fetchXYArguments（moveTo 目标解析）：第二参是
   *  number → 数值形态；否则对象形态（RoomPosition 取自身、RoomObject 取
   *  .pos——两者都不是则 x/y 保持 undefined）。NaN 归 undefined。instanceof
   *  用 gw.RoomPosition（预置捕获——玩家换 globalThis.RoomPosition 不影响
   *  API 判定，与 C 表纪律一致）。 */
  function fetchXYArguments(firstArg, secondArg) {
    let x, y, roomName;
    if (secondArg === undefined || typeof secondArg !== 'number') {
      if (!isObjectLike(firstArg)) return [undefined, undefined, undefined];
      if (firstArg instanceof gw.RoomPosition) {
        x = firstArg.x; y = firstArg.y; roomName = firstArg.roomName;
      }
      if (firstArg.pos && firstArg.pos instanceof gw.RoomPosition) {
        x = firstArg.pos.x; y = firstArg.pos.y; roomName = firstArg.pos.roomName;
      }
    } else {
      x = firstArg; y = secondArg;
    }
    if (Number.isNaN(x)) x = undefined;
    if (Number.isNaN(y)) y = undefined;
    return [x, y, roomName];
  }

  /** 官方 utils.js:576-609 deserializePath：序列化串 "XXYYddd..." →
   *  [{x, y, dx, dy, direction}]（首项坐标 = 第一步的落点，x-dx/y-dy 即其
   *  起步格——moveByPath 的对象形态匹配面）。非串/坐标 NaN/非法方向位都抛
   *  官方同文案错误。 */
  function deserializePath(path) {
    if (typeof path !== 'string') {
      throw new Error('`path` is not a string');
    }
    const result = [];
    if (!path.length) return result;
    // 不传进制参数，保留 parseInt 对 0x 前缀等输入的解析规则。
    let x = parseInt(path.substring(0, 2));
    let y = parseInt(path.substring(2, 4));
    if (Number.isNaN(x) || Number.isNaN(y)) {
      throw new Error('`path` is not a valid serialized path string');
    }
    for (let i = 4; i < path.length; i++) {
      const direction = parseInt(path.charAt(i));
      const offs = OFFSETS_BY_DIRECTION[direction];
      if (!offs) {
        throw new Error('`path` is not a valid serialized path string');
      }
      const dx = offs[0], dy = offs[1];
      if (i > 4) { x += dx; y += dy; }
      result.push({ x, y, dx, dy, direction });
    }
    return result;
  }
  gw.deserializePath = deserializePath;
  // 官方 rooms.js:532-534：deserializePath 是 Room 静态（实现在本段故在此挂）
  globalThis.Room.deserializePath = deserializePath;

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
  gw.__creepC = C; // creep_resources.js 复用此常量表。

  // 官方 _hasActiveBodypart（game/creeps.js:9-17 的等价物）：hits>0 的指定部件数
  function countActiveParts(body, type) {
    let n = 0;
    for (const p of body || []) if (p.type === type && p.hits > 0) n += 1;
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
  /** Creep.move：官方 creeps.js:126-156——重预检（fatigue/bodypart/方向）后记
   *  intent 返回 OK；朝相邻 creep 形态（target 为 Creep 实例）同官方 :132-141，
   *  结构上跳过疲劳/MOVE 门（拉拽通道）。疲劳与 MOVE 门读计划派生常量
   *  （规则 builtin:creep.move.*，默认 0/1 即官方行为；与结算仲裁原生
   *  准入预填同源）。官方 :130-133 读 this.my/this.spawning（getter 通道）；
   *  :145 的 fatigue 是 data(this.id) 显式回查（新鲜读绕缓存——跨 tick
   *  旧壳按当前值判 ERR_TIRED）。 */
  globalThis.Creep.prototype.move = function move(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (target && target instanceof globalThis.Creep) {
      if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
      gw.intents.set(this.id, 'move', { id: target.id });
      return C.OK;
    }
    if (gw.rawData(this.id).fatigue > C.MOVE_FATIGUE_LIMIT) return C.ERR_TIRED;
    if (countActiveParts(this.body, C.MOVE) < C.MOVE_MIN_ACTIVE_MOVES) {
      return C.ERR_NO_BODYPART;
    }
    const direction = +target;
    if (!direction || direction < 1 || direction > 8) return C.ERR_INVALID_ARGS;
    gw.intents.set(this.id, 'move', { direction });
    return C.OK;
  };

  /** 官方 moveTo 全流程（engine/src/game/creeps.js 的 moveTo）：预检（归属/
   * 孵化/疲劳+visualizePathStyle 豁免/MOVE 部件/目标解析）后消费 creep
   * memory 的 _move 路径缓存（reusePath 缺省 5、serializeMemory 缺省 true；
   * 过期或换房删除、目的地匹配时按当前位置截断后 moveByPath），
   * noPathFinding 出口，findPathTo 真路径（range 缺省 1）写回缓存并走首步。
   * 数值坐标形态沿用调用方 opts 引用（官方同形：缺省值写入可见）；对象
   * 目标形态浅拷贝 secondArg。parseInt 用全局（官方 realm 语义）。 */
  globalThis.Creep.prototype.moveTo = function moveTo(firstArg, secondArg, opts) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (isObjectLike(firstArg)) {
      opts = _.clone(secondArg);
    }
    opts = opts || {};
    if (gw.rawData(this.id).fatigue > C.MOVE_FATIGUE_LIMIT && !opts.visualizePathStyle) {
      return C.ERR_TIRED;
    }
    if (countActiveParts(this.body, C.MOVE) < C.MOVE_MIN_ACTIVE_MOVES) {
      return C.ERR_NO_BODYPART;
    }
    const [x, y, rawRoomName] = fetchXYArguments(firstArg, secondArg);
    const roomName = rawRoomName || this.pos.roomName;
    if (x === undefined || y === undefined) {
      // 官方 creeps.js:182-184 先过 register.assertTargetObject（game.js:122-126）：
      // plain object + 24 位字符串 id 视为 Memory 序列化对象，抛官方文案
      if (firstArg && _.isPlainObject(firstArg) && _.isString(firstArg.id)
        && firstArg.id.length === 24) {
        throw new Error("It seems you're trying to use a serialized game object stored in Memory which is not allowed. Please use `Game.getObjectById` to retrieve a live object reference instead.");
      }
      return C.ERR_INVALID_TARGET;
    }
    const targetPos = new gw.RoomPosition(x, y, roomName);
    if (opts.reusePath === undefined) opts.reusePath = 5;
    if (opts.serializeMemory === undefined) opts.serializeMemory = true;
    // 官方 :196-198 绘图样式缺省（官方经 _.defaults 原地补全调用方对象）
    if (opts.visualizePathStyle) {
      _.defaults(opts.visualizePathStyle, {
        fill: 'transparent', stroke: '#fff', lineStyle: 'dashed', strokeWidth: 0.15, opacity: 0.1,
      });
    }
    if (x == this.pos.x && y == this.pos.y && roomName == this.pos.roomName) {
      return C.OK;
    }
    // 官方 :241-278 _move 缓存消费
    let visualized = false;
    if (opts.reusePath && this.memory && isObjectLike(this.memory) && this.memory._move) {
      const _move = this.memory._move;
      if (gw.world.time > _move.time + parseInt(opts.reusePath) || _move.room != this.pos.roomName) {
        delete this.memory._move;
      } else if (_move.dest.room == roomName && _move.dest.x == x && _move.dest.y == y) {
        let path = _.isString(_move.path) ? deserializePath(_move.path) : _move.path;
        const idx = _.findIndex(path, { x: this.pos.x, y: this.pos.y });
        if (idx !== -1) {
          path.splice(0, idx + 1);
          _move.path = opts.serializeMemory ? roomSerializePath(path) : path;
        }
        if (path.length === 0) {
          return this.pos.isNearTo(targetPos) ? C.OK : C.ERR_NO_PATH;
        }
        if (opts.visualizePathStyle) {
          this.room.visual.poly(path, opts.visualizePathStyle);
          visualized = true;
        }
        const result = this.moveByPath(path);
        if (result === C.OK) {
          return C.OK;
        }
      }
    }
    if (opts.noPathFinding) {
      return C.ERR_NOT_FOUND;
    }
    const path = this.pos.findPathTo(targetPos, opts);
    // 官方 :285-293 缓存写回（serializeMemory 缺省序列化串驻留 memory）
    if (opts.reusePath && this.memory && isObjectLike(this.memory)) {
      this.memory._move = {
        dest: { x, y, room: roomName },
        time: gw.world.time,
        path: opts.serializeMemory ? roomSerializePath(path) : _.clone(path),
        room: this.pos.roomName,
      };
    }
    if (path.length === 0) {
      return C.ERR_NO_PATH;
    }
    // 官方 :296-298：缓存臂已绘制时抑制重寻路径的二次绘制
    if (opts.visualizePathStyle && !visualized) {
      this.room.visual.poly(path, opts.visualizePathStyle);
    }
    return this.move(path[0].direction);
  };

  /** Creep.moveByPath：官方 creeps.js:305-327——自身无任何预检（my/spawning/
   *  fatigue/MOVE 全部经委托的 this.move 生效）。RoomPosition 数组形态：当前
   *  位置在路径中找 idx（找不到则须紧邻首步，否则 ERR_NOT_FOUND），取下一
   *  步方向 move；序列化串形态先 deserializePath（官方 utils.js:576-609，
   *  非法串抛错）；findPathTo 风格对象数组按 x-dx/y-dy 匹配当前格取方向。 */
  globalThis.Creep.prototype.moveByPath = function moveByPath(path) {
    if (Array.isArray(path) && path.length > 0 && path[0] instanceof gw.RoomPosition) {
      let idx = -1;
      for (let i = 0; i < path.length; i++) {
        if (path[i].isEqualTo(this.pos)) { idx = i; break; }
      }
      if (idx === -1 && !path[0].isNearTo(this.pos)) {
        return C.ERR_NOT_FOUND;
      }
      idx += 1;
      if (idx >= path.length) return C.ERR_NOT_FOUND;
      return this.move(this.pos.getDirectionTo(path[idx]));
    }
    if (typeof path === 'string') {
      path = deserializePath(path);
    }
    if (!Array.isArray(path)) return C.ERR_INVALID_ARGS;
    const cur = path.find(
      (i) => i.x - i.dx == this.pos.x && i.y - i.dy == this.pos.y,
    );
    if (!cur) return C.ERR_NOT_FOUND;
    return this.move(cur.direction);
  };

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
        if (o.room === target.pos.roomName && o.x === target.pos.x && o.y === target.pos.y
          && o.structureType === 'extractor') {
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
      // 其余目标同官方 register.assertTargetObject
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

  /** 检查 ATTACK 部件、目标存活与类型、ATTACK_RANGE 射程及敌方安全模式；未实现 invulnerability effects 检查。
   * 射程读常量表（规则 builtin:creep.attack.range 注入的生效值），
   * 与 Rust 结算同源。来源：上游 engine/src/game/creeps.js 的 attack。 */
  globalThis.Creep.prototype.attack = function attack(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.ATTACK) === 0) return C.ERR_NO_BODYPART;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    // 目标须在当前 tick 的原始数据中仍存在，并通过后面的实例类型检查。
    const okTarget = target && target.id
      && (gw.isKnownCreep(target.id) || gw.isKnownStructure(target.id))
      && (target instanceof globalThis.Creep
        || target instanceof globalThis.StructureSpawn
        || target instanceof globalThis.Structure);
    if (!okTarget) return C.ERR_INVALID_TARGET;
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
    const modBuilding = gw.modApi === undefined ? undefined
      : gw.modApi.buildings[target && target.structureType];
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
    const siteIsObstacle = (C.OBSTACLE_OBJECT_TYPES || []).includes(target.structureType)
      || (modBuilding !== undefined && modBuilding.pathing === 'obstacle');
    if (siteIsObstacle) {
      let objectsInTile = false;
      let creepsInTile = false;
      let myCreepsInTile = false;
      const objs = gw.world.objects || [];
      for (let i = 0; i < objs.length; i++) {
        const o = objs[i];
        if (o.room !== target.pos.roomName || o.x !== target.pos.x || o.y !== target.pos.y) continue;
        const t = o.type === 'structure' || o.type === 'spawn'
          ? (o.structureType || 'spawn') : o.type;
        const entry = gw.modApi === undefined ? undefined : gw.modApi.buildings[t];
        const obstacleType = (C.OBSTACLE_OBJECT_TYPES || []).includes(t)
          || (entry !== undefined && entry.pathing === 'obstacle' && o.type === 'structure');
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
    const okTarget = target && target.id
      && gw.isKnownStructure(target.id)
      && (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!this.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'repair', { id: target.id, x: target.pos.x, y: target.pos.y });
    return C.OK;
  };

  /** 检查 WORK 部件、可拆结构、邻接和安全模式；未实现 effects 检查。
   * 来源：上游 engine/src/game/creeps.js 的 dismantle。 */
  globalThis.Creep.prototype.dismantle = function dismantle(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    // 可拆类型 = 内建造价表或 Mod 建筑目录（与结算侧 cost_bearing 同源）。
    const modBuilding = gw.modApi === undefined ? undefined
      : gw.modApi.buildings[target && target.structureType];
    const okTarget = target && target.id
      && (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn)
      && ((C.CONSTRUCTION_COST && target.structureType in C.CONSTRUCTION_COST)
        || modBuilding !== undefined);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    gw.intents.set(this.id, 'dismantle', { id: target.id });
    return C.OK;
  };

  /** 官方 heal（creeps.js:678-705）：HEAL + Creep 目标 + HEAL_RANGE 射程 + safeMode。 */
  globalThis.Creep.prototype.heal = function heal(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
    if (!(target && target.id && gw.isKnownCreep(target.id)
      && target instanceof globalThis.Creep)) {
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

  /** 检查 RANGED_ATTACK、当前目标类型、RANGED_ATTACK_RANGE 射程及敌方安全模式，再记录指令。
   * 不建模 invulnerability effects；rampart 重定向由 Rust 结算处理。
   * 来源：上游 engine/src/game/creeps.js 的 rangedAttack。 */
  globalThis.Creep.prototype.rangedAttack = function rangedAttack(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.RANGED_ATTACK) === 0) return C.ERR_NO_BODYPART;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    const okTarget = target && target.id
      && (gw.isKnownCreep(target.id) || gw.isKnownStructure(target.id))
      && (target instanceof globalThis.Creep
        || target instanceof globalThis.PowerCreep
        || target instanceof globalThis.StructureSpawn
        || target instanceof globalThis.Structure);
    if (!okTarget) return C.ERR_INVALID_TARGET;
    if (!this.pos.inRangeTo(target, C.RANGED_ATTACK_RANGE)) return C.ERR_NOT_IN_RANGE;
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
    if (!(target && target.id && gw.isKnownCreep(target.id)
      && target instanceof globalThis.Creep)) {
      return C.ERR_INVALID_TARGET;
    }
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    if (!this.pos.inRangeTo(target.pos, C.RANGED_HEAL_RANGE)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'rangedHeal', { id: target.id });
    return C.OK;
  };

})();
