/* kk-runner prelude · 30_game 段 4a：Creep 前半（动作方法族）（对照官方 engine/src/game/creeps.js）。
 * B1 结构治理批：资源族四方法/controller 族/say-suicide-pull 杂项族/GCL 门
 * 三函数/CREEP_METHODS 表与残余桩循环/creep 装配拆至段 4b（creep_resources.js），
 * 段间共享经 gw 桥（C 表/countActiveParts/blockedBySafeMode），纯搬移零行为。
 *
 * A3b 薄壳化：字段/方法全部挂 Creep.prototype（官方 make 语义），实例 own 面
 * 只剩 id/pos/room（构造定格）。字段经 defineGameObjectProperties 的 lazy
 * getter + `_${name}` 首访缓存（utils.js:509-545 机制）——未缓存访问经
 * gw.rawData(this.id) 回查当前 tick 投影：对象消亡抛官方同文案错误
 * （STALERECV-001），跨 tick 旧壳读当前 tick 新值（STALERECV-002）。
 * 动作方法（A2 起从桩换真：预检 + intents.set）维持官方预检序，字段访问走
 * this（getter 通道，与官方 this.my 一致）；官方方法体显式 data(this.id)
 * 回查的位置我方对齐 gw.rawData。A4-iv 增补移动杂项族：moveTo（直线桩形态
 * ——A* 归 M2B FFI 批）/moveByPath/say/suicide/pull 换真。
 * 依赖：gw.intents（收集器，30_game/game.js）、gw.rawData/gw.rawById、
 * gw.viewerName、gw.ensureMemory、gw.mkStubMethod、gw.defineGameObjectProperties、
 * gw.RoomPosition、gw.deserializePath、gw.__Object。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;

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
    // 官方 parseInt 无进制参（'0x' 头病态串 NaN 抛错同官方，片 3b 评审 P3 清偿）
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
    // M2B 扩面 P1：ranged 族的部件门（rangedAttack/rangedMassAttack）
    RANGED_ATTACK: globalThis.RANGED_ATTACK,
    HEAL: globalThis.HEAL,
    CLAIM: globalThis.CLAIM,
    ERR_NOT_FOUND: globalThis.ERR_NOT_FOUND,
    ERR_RCL_NOT_ENOUGH: globalThis.ERR_RCL_NOT_ENOUGH,
    ERR_FULL: globalThis.ERR_FULL,
    ERR_NO_PATH: globalThis.ERR_NO_PATH,
    ERR_GCL_NOT_ENOUGH: globalThis.ERR_GCL_NOT_ENOUGH,
    // controller 族（A4-iii）：generateSafeMode 的 ghodium 消耗门 +
    // claimController 的 GCL 门（calcNeededGcl = GCL_MULTIPLY × (n-1)^GCL_POW，
    // utils.js:661-663）
    SAFE_MODE_COST: globalThis.SAFE_MODE_COST,
    RESOURCE_GHODIUM: globalThis.RESOURCE_GHODIUM,
    GCL_MULTIPLY: globalThis.GCL_MULTIPLY,
    GCL_POW: globalThis.GCL_POW,
    ERR_NO_PATH: globalThis.ERR_NO_PATH,
    OBSTACLE_OBJECT_TYPES: freezeConst(globalThis.OBSTACLE_OBJECT_TYPES),
    CONSTRUCTION_COST: freezeConst(globalThis.CONSTRUCTION_COST),
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
  gw.__creepC = C; // B1 段拆分桥：4b（creep_resources.js）复用同一 C 表对象

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

  // B1 段拆分桥：4b 经 gw 取用（加载序 4a→4b，绑定同一函数对象，4b 调用点保持裸名）
  gw.countActiveParts = countActiveParts;
  gw.blockedBySafeMode = blockedBySafeMode;

  function carriedEnergy(self) {
    return (self.store && self.store.energy) || 0;
  }

  /** A6：extractor 的 active 门已换 Structure.prototype.isActive 真实现
   *  （官方 creeps.js:380 同款调用；单实例配额近似 structureActiveAt 已删）。 */

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
    // M2B 扩面 P1 真化（官方 creeps.js:84-92）：本人恒见消息；他人仅
    // isPublic 见——载荷由投影 say/sayPublic 携带（say.js 截 10 已在结算侧落）
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

  /** Creep.moveTo：官方 creeps.js:158-257 的**直线桩形态**——预检段完整
   *  对照，寻路退化为 getDirectionTo 直线一步（真 A* 归 M2B FFI 批）。
   *  官方序：my(:166) → spawning(:169) → opts 归位（对象形态取第二参浅拷贝
   *  :172-177——数值形态不拷贝，reusePath 缺省赋值会改写调用方 opts 对象，
   *  官方 quirk 原样保留）→ fatigue 显式回查（:180，visualizePathStyle 在场
   *  免疲劳门——纯可视化 tick；官方 `!opts` 是 opts||{} 后的死条件不移植）→
   *  MOVE 部件(:184) → fetchXYArguments(:187) → roomName 缺省本房(:188) →
   *  x/y undefined 拒(:189-192，官方 assertTargetObject 是 Memory 序列化壳
   *  探针，我方无注册表断言面不移植) → 同格即 OK(:213，宽松 ==) →
   *  reusePath/serializeMemory 缺省 5/true(:205-211，memory._move 缓存面
   *  桩化登记) → noPathFinding(:253) → 寻路(:255) → move(path[0].direction)。
   *  直线退化与官方 A* 的行为差异面（绕障/对角穿墙/路径复用/跨房）见批次
   *  登记报告——ERR_NO_PATH 分支在退化形态下不可达（直线方向恒存在）。 */
  globalThis.Creep.prototype.moveTo = function moveTo(firstArg, secondArg, opts) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (isObjectLike(firstArg)) {
      opts = gw.__Object.assign({}, secondArg);
    }
    opts = opts || {};
    if (gw.rawData(this.id).fatigue > 0 && !opts.visualizePathStyle) {
      return C.ERR_TIRED;
    }
    if (countActiveParts(this.body, C.MOVE) === 0) return C.ERR_NO_BODYPART;
    const [x, y, rawRoomName] = fetchXYArguments(firstArg, secondArg);
    const roomName = rawRoomName || this.pos.roomName;
    if (x === undefined || y === undefined) {
      return C.ERR_INVALID_TARGET;
    }
    const targetPos = new gw.RoomPosition(x, y, roomName);
    if (opts.reusePath === undefined) opts.reusePath = 5;
    if (opts.serializeMemory === undefined) opts.serializeMemory = true;
    if (x == this.pos.x && y == this.pos.y && roomName == this.pos.roomName) {
      return C.OK;
    }
    if (opts.noPathFinding) {
      return C.ERR_NOT_FOUND;
    }
    // 官方 :255 `this.pos.findPathTo(targetPos, opts)` 的 A* 寻路退化为直线
    // 方向（官方阈值算法已在我方 RoomPosition.getDirectionTo）——只取一步，
    // 后续每 tick 由玩家代码再次调用推进
    const direction = this.pos.getDirectionTo(targetPos);
    if (direction === undefined) {
      return C.ERR_NO_PATH; // 防御：同格已提前 return OK，此路不可达
    }
    return this.move(direction);
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
    // 官方 creeps.js:607：register 存活判定（陈旧壳拒绝——UNDOC-STALEARG 面，
    // M2B P6 清偿：isKnown* 经 gw 桥与结构 API 单源）
    const okTarget = target && target.id
      && (gw.isKnownCreep(target.id) || gw.isKnownStructure(target.id))
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
      const objs = gw.world.objects || [];
      for (let i = 0; i < objs.length; i++) {
        const o = objs[i];
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
      && gw.isKnownStructure(target.id)
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
    if (!(target && target.id && gw.isKnownCreep(target.id)
      && target instanceof globalThis.Creep)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
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

  /** 官方 rangedAttack（creeps.js:626-657）：RANGED_ATTACK 部件 + 3 格 +
   *  目标族（Creep/PowerCreep/Spawn/Structure——PowerCreep 归 M4，instanceof
   *  链保持官方形）+ safeMode 门（敌房 safeMode 下 NO_BODYPART 官方口径）；
   *  invulnerability 效果门（:649-652）属 power 体系不建模。执行器
   *  rangedAttack.js：rampart 重定向在结算侧。 */
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
    if (!this.pos.inRangeTo(target, 3)) return C.ERR_NOT_IN_RANGE;
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

  /** 官方 rangedHeal（creeps.js:706-733）：HEAL + Creep 目标 + 3 格 + safeMode。 */
  globalThis.Creep.prototype.rangedHeal = function rangedHeal(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
    if (!(target && target.id && gw.isKnownCreep(target.id)
      && target instanceof globalThis.Creep)) {
      return C.ERR_INVALID_TARGET;
    }
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    if (!this.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'rangedHeal', { id: target.id });
    return C.OK;
  };

})();
