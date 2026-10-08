/* 定义 Creep 的资源转移、控制器操作和其他动作，复用 creep.js 捕获的常量与检查函数。
 * 容量优先读取世界数据提供的字段；字段缺失时按对象类型或有效 CARRY 部件推算。
 * 方法先按玩家 API 的顺序检查参数，再加入指令；实际资源与世界变化由 Rust 结算复核。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  // 与 creep.js 使用同一常量表和检查函数。
  const C = gw.__creepC;
  const countActiveParts = gw.countActiveParts;
  const blockedBySafeMode = gw.blockedBySafeMode;

  // 捕获 GCL 计算所需的函数和构造器，避免玩家替换全局后影响 API 预检。
  const mathPow = Math.pow;
  const __Set = Set;

  /** 官方 calcNeededGcl（utils.js:661-663）：GCL_MULTIPLY × (level−1)^GCL_POW。 */
  function calcNeededGcl(level) {
    return C.GCL_MULTIPLY * mathPow(level - 1, C.GCL_POW);
  }

  /** 优先读取 world.gcl 的 GCL 总点数；缺失时由 Game.gcl 反算。
   * 这里只做玩家 API 预检，Rust 结算还会根据 PlayerState 复核占领条件。 */
  function gclPoints() {
    if (typeof gw.world.gcl === 'number') return gw.world.gcl;
    const g = Game.gcl;
    return C.GCL_MULTIPLY * mathPow(g.level - 1, C.GCL_POW) + g.progress;
  }

  /** 扫描当前世界控制器，按查看者所有权统计已占房间，按房间名去重。 */
  function myClaimedRoomCount() {
    const seen = new __Set();
    const objs = gw.world.objects || [];
    for (let i = 0; i < objs.length; i++) {
      const o = objs[i];
      if (o.type === 'structure' && o.structureType === 'controller' && o.owner === gw.viewerName) {
        seen.add(o.room);
      }
    }
    return seen.size;
  }

  // 资源方法先检查目标在当前 tick 注册范围内，再计算容量与数量并记录指令。
  // 容量字段由正式世界数据提供；缺失时使用下面的类型推算。
  // 拒绝 24 字符 id 的普通序列化对象时按上游 assertTargetObject 抛错，而非返回错误码。

  // 捕获数量计算与白名单检查的方法，避免玩家替换全局改变预检结果。
  const __MathMin = Math.min;
  const __MathMax = Math.max;
  const __arrayIncludes = Array.prototype.includes;
  const __reflectApply = Reflect.apply;

  /** 官方 structureTypes 键集（game.js:224-247，21 型）+ 'wall' 投影别名
   *  （classGlobalFor 将 wall/constructedWall 归并 StructureWall——手写世界
   *  两种拼法等价；官方 raw type 恒为 'constructedWall'）。 */
  const STRUCTURE_REGISTER_TYPES = new Set([
    'rampart',
    'road',
    'extension',
    'constructedWall',
    'wall',
    'keeperLair',
    'controller',
    'link',
    'storage',
    'tower',
    'observer',
    'powerBank',
    'powerSpawn',
    'lab',
    'extractor',
    'terminal',
    'container',
    'spawn',
    'nuker',
    'portal',
    'factory',
    'invaderCore',
  ]);

  /** register.structures[id] 的投影等价（官方 game.js:295-299 收录 + :224-247
   *  键集；spawn 的 raw type 是 'spawn' 单列）。命中返回 raw 投影（官方表存
   *  wrapper、读 structureType 即 raw.type——我方直读 raw 等价），否则 undefined。 */
  function structureRegisterEntry(id) {
    const raw = gw.rawById.get(id);
    if (!raw) return undefined;
    if (raw.type === 'spawn') return raw;
    if (
      raw.type === 'structure' &&
      (STRUCTURE_REGISTER_TYPES.has(raw.structureType) ||
        (gw.modApi !== undefined &&
          gw.modApi.buildings !== undefined &&
          gw.modApi.buildings[raw.structureType] !== undefined))
    ) {
      return raw;
    }
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
      throw new Error(
        "It seems you're trying to use a serialized game object stored in Memory which is not allowed. Please use `Game.getObjectById` to retrieve a live object reference instead.",
      );
    }
  }
  // _.isPlainObject 等价（原型为 Object.prototype 或 null）——经 gw.__Object
  // 预置引用（玩家交换 globalThis.Object 不影响判定）
  function isPlainObject(v) {
    if (typeof v !== 'object' || v === null) return false;
    const proto = gw.__Object.getPrototypeOf(v);
    return proto === gw.__Object.prototype || proto === null;
  }

  /** 缺少 storeCapacity 时，按存活 CARRY 部件数推算基础容量。
   * 此后备计算不含 boost；正式世界数据提供容量字段时优先使用该字段。 */
  function carryCapacityOf(raw) {
    let parts = 0;
    // 按索引读取 body，避免动态调用玩家可替换的数组迭代器。
    const body = raw.body || [];
    for (let i = 0; i < body.length; i++) {
      const part = typeof body[i] === 'string' ? { type: body[i], hits: 100 } : body[i];
      if (part && part.hits > 0 && part.type === 'carry') parts += 1;
    }
    return parts * C.CARRY_CAPACITY;
  }

  /** 房内 extension 容量（官方 extensions/tick.js:8-17 每结算 tick 归一
   *  storeCapacityResource.energy = EXTENSION_ENERGY_CAPACITY[rcl]；无
   *  controller 房保持落成值 0——build.js:134）。 */
  function extensionCapacityIn(roomName) {
    let level = null;
    // 按索引扫描当前房间控制器，避免动态调用玩家可替换的数组迭代器。
    const objs = gw.world.objects || [];
    for (let i = 0; i < objs.length; i++) {
      const o = objs[i];
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
        return {
          capacity: undefined,
          perResource: { energy: raw.energyCapacity || C.SPAWN_ENERGY_CAPACITY },
        };
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
        return {
          capacity: C.LAB_ENERGY_CAPACITY + C.LAB_MINERAL_CAPACITY,
          perResource: { energy: C.LAB_ENERGY_CAPACITY },
        };
      case 'powerSpawn': // build.js:239
        return {
          capacity: undefined,
          perResource: {
            energy: C.POWER_SPAWN_ENERGY_CAPACITY,
            power: C.POWER_SPAWN_POWER_CAPACITY,
          },
        };
      case 'nuker': // build.js:265（ptr 变体不建模）
        return {
          capacity: undefined,
          perResource: { energy: C.NUKER_ENERGY_CAPACITY, G: C.NUKER_GHODIUM_CAPACITY },
        };
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
    if (perResource) {
      const keys = gw.__Object.keys(perResource);
      for (let i = 0; i < keys.length; i++) reserved += perResource[keys[i]];
    }
    return __MathMax(0, (fields.capacity || 0) - reserved);
  }

  /** 官方 utils.js:615-620 calcResources 的 store 分支（own 键求和——_.sum
   *  同为 own 枚举键；无 store 对象的 RESOURCES_ALL 顶字段求和是旧引擎形态，
   *  投影对象不产生，不可达缺 0）。 */
  function sumStore(raw) {
    const store = raw.store;
    if (!store) return 0;
    let sum = 0;
    const keys = gw.__Object.keys(store);
    for (let i = 0; i < keys.length; i++) sum += store[keys[i]];
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
    if (!__reflectApply(__arrayIncludes, C.RESOURCES_ALL || [], [resourceType]))
      return C.ERR_INVALID_ARGS; // :411-413
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
    if (!__reflectApply(__arrayIncludes, C.RESOURCES_ALL || [], [resourceType]))
      return C.ERR_INVALID_ARGS; // :438-440
    // 有 store 时短路跳过 structEntry 解引用；无 store 的非结构目标可能在此抛 TypeError。
    // 不建模 customObjects。来源：上游 engine/src/game/creeps.js 的 transfer 目标检查。
    if (!target || !target.id) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const structEntry = structureRegisterEntry(target.id);
    const targetRaw = gw.rawById.get(target.id);
    // Mod 库存对象目标族（G09.a）：带 store 信封的 Mod 记录经其玩家类
    // 绑定接收（与结构/creep 族并列）；预检读投影信封，与结算同源。
    const modEntry =
      targetRaw !== undefined &&
      targetRaw.type === 'mod' &&
      gw.modApi !== undefined &&
      gw.modApi.types !== undefined
        ? gw.modApi.types[targetRaw.modType]
        : undefined;
    const inRegisters =
      structEntry !== undefined ||
      (!!targetRaw && (targetRaw.type === 'creep' || targetRaw.type === 'powerCreep')) ||
      modEntry !== undefined;
    if (
      !inRegisters ||
      (!gw.rawData(target.id).store &&
        (structEntry === undefined || structEntry.structureType !== 'controller')) ||
      (target instanceof globalThis.Creep && target.spawning) || // :445
      (!(target instanceof globalThis.StructureSpawn) &&
        !(target instanceof globalThis.Structure) &&
        !(target instanceof globalThis.Creep) &&
        !(target instanceof globalThis.PowerCreep) &&
        !(modEntry !== undefined && target instanceof modEntry.cls))
    ) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    // 向控制器 transfer 能量时转发 upgradeController，复用其权限、部件和距离检查。
    // 来源：上游 engine/src/game/creeps.js 的 transfer。
    if (
      resourceType === C.RESOURCE_ENERGY &&
      structEntry &&
      structureKindOf(structEntry) === 'controller'
    ) {
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
    const storedAmount = fields.perResource
      ? rawTarget.store[resourceType] || 0
      : sumStore(rawTarget);
    const targetCapacity = capacityForResource(rawTarget, resourceType);
    if (!rawTarget.store || storedAmount >= targetCapacity) return C.ERR_FULL; // :473-475
    if (!amount) {
      amount = __MathMin(gw.rawData(this.id).store[resourceType], targetCapacity - storedAmount); // :477-479
    }
    if (gw.rawData(this.id).store[resourceType] < amount) return C.ERR_NOT_ENOUGH_RESOURCES; // :481-483
    if (amount + storedAmount > targetCapacity) return C.ERR_FULL; // :485-487（显式超容不钳制——拒）
    gw.intents.set(this.id, 'transfer', { id: target.id, amount, resourceType }); // :489
    return C.OK;
  };

  /** 检查目标类型与 store、terminal 封锁、rampart、安全模式和不可提取结构，
   * 再检查距离、空容量与资源数量。来源：上游 engine/src/game/creeps.js 的 withdraw。 */
  globalThis.Creep.prototype.withdraw = function withdraw(target, resourceType, amount) {
    if (!this.my) return C.ERR_NOT_OWNER; // :494-496
    if (this.spawning) return C.ERR_BUSY; // :497-499
    if (amount < 0) return C.ERR_INVALID_ARGS; // :500-502
    if (!__reflectApply(__arrayIncludes, C.RESOURCES_ALL || [], [resourceType]))
      return C.ERR_INVALID_ARGS; // :503-505
    // 目标门 :507-516：store 面在 data() 读——未知 id 在此抛官方同文案错误
    // （rawData 的求值点 = 官方第三操作数，早于 assertTargetObject）
    if (!target || !target.id) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const rawTarget = gw.rawData(target.id);
    const structEntry = structureRegisterEntry(target.id);
    // Mod 库存对象目标族（G09.a）：带 store 信封的 Mod 记录经其玩家类
    // 绑定提取；结构专属拒绝（terminal 封锁/nuker/powerBank）不适用。
    const targetRaw = gw.rawById.get(target.id);
    const modEntry =
      targetRaw !== undefined &&
      targetRaw.type === 'mod' &&
      gw.modApi !== undefined &&
      gw.modApi.types !== undefined
        ? gw.modApi.types[targetRaw.modType]
        : undefined;
    if (
      !rawTarget.store ||
      ((!structEntry || !(target instanceof globalThis.Structure)) &&
        !(target instanceof globalThis.Tombstone) &&
        !(target instanceof globalThis.Ruin) &&
        !(modEntry !== undefined && target instanceof modEntry.cls))
    ) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    // 按 effects 中 power/ticksRemaining 判断 terminal 是否被 PWR_DISRUPT_TERMINAL 封锁。
    if (structEntry && structureKindOf(structEntry) === 'terminal') {
      const effect = (rawTarget.effects || []).find((e) => e && e.power === C.PWR_DISRUPT_TERMINAL);
      if (effect && effect.ticksRemaining > 0) return C.ERR_INVALID_TARGET;
    }
    // 敌方非公开 rampart 保护的目标不能提取资源；公开 rampart 不触发此拒绝。
    if (
      target.my === false &&
      target.pos
        .lookFor('structure')
        .some((i) => i.structureType === C.STRUCTURE_RAMPART && !i.my && !i.isPublic)
    ) {
      return C.ERR_NOT_OWNER;
    }
    // :528-530：他人 safeMode 生效期拒（this.room 官方直解引用；引擎态房间恒
    // 在投影，手写世界缺房时按既有 blockedBySafeMode 同款守卫——登记灰区）
    const ctl = this.room && this.room.controller;
    if (ctl && !ctl.my && ctl.safeMode) return C.ERR_NOT_OWNER;
    // :532-534：nuker/powerBank 不可提取
    if (
      structEntry &&
      (structureKindOf(structEntry) === C.STRUCTURE_NUKER ||
        structureKindOf(structEntry) === C.STRUCTURE_POWER_BANK)
    ) {
      return C.ERR_INVALID_TARGET;
    }
    // :536-538：容量 0 且目标无该资源且非 Tombstone 拒
    if (
      !capacityForResource(rawTarget, resourceType) &&
      !rawTarget.store[resourceType] &&
      !(target instanceof globalThis.Tombstone)
    ) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE; // :540-542
    // :544-545：actor 空容量（storeCapacity − calcResources——raw 面等价）
    const emptySpace = rawStoreCapacity(gw.rawData(this.id)) - sumStore(gw.rawData(this.id));
    if (emptySpace <= 0) return C.ERR_FULL; // :546-548
    if (!amount) amount = __MathMin(emptySpace, rawTarget.store[resourceType]); // :550-552
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
    if (
      !target ||
      !target.id ||
      !rawTarget ||
      rawTarget.type !== 'resource' ||
      !(target instanceof globalThis.Energy)
    ) {
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

  // 控制器操作；预检顺序决定多个条件同时失败时的返回码。
  /** Creep.claimController：官方 creeps.js:839-886。预检序不重排——
   *  my/spawning → GCL 门（:848-852，读 rooms+同 tick 已 claim 计数）→
   *  新手区门（:853-855，N/A——世界无 novice 概念，官方私服 roomInfo.novice=0
   *  恒不触发）→ 目标（:852-855）→ CLAIM 部件（:856-858）→ shardAccess
   *  （:863-865，单分片 N/A）→ 邻接（:866-868）→ controller 类型（:869-871）
   *  → level>0（:872-874）→ 他人预定（:875-877）→ 本房 safeMode（:878-880，
   *  blockedBySafeMode 同形）→ 计数 +1（:882）→ intents.set（:884）。 */
  globalThis.Creep.prototype.claimController = function claimController(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    const controllersClaimed = myClaimedRoomCount() + (gw.controllersClaimedInTick || 0);
    if (
      controllersClaimed &&
      (!gclPoints() || gclPoints() < calcNeededGcl(controllersClaimed + 1))
    ) {
      return C.ERR_GCL_NOT_ENOUGH;
    }
    if (!(target && target.id && target instanceof globalThis.Structure)) {
      return C.ERR_INVALID_TARGET;
    }
    if (countActiveParts(this.body, C.CLAIM) === 0) return C.ERR_NO_BODYPART;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (target.structureType !== 'controller') return C.ERR_INVALID_TARGET;
    if (target.level > 0) return C.ERR_INVALID_TARGET;
    if (target.reservation && target.reservation.username !== gw.viewerName) {
      return C.ERR_INVALID_TARGET;
    }
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    gw.controllersClaimedInTick = (gw.controllersClaimedInTick || 0) + 1;
    gw.intents.set(this.id, 'claimController', { id: target.id });
    return C.OK;
  };

  /** Creep.attackController：官方 creeps.js:888-920。目标须 StructureController
   /*  实例（:897）；CLAIM 部件（:901——官方 _getActiveBodyparts 复数形，零部件
   *  同判 NO_BODYPART）；邻接（:905）；原始房（无主无预定）不可攻（:908-910）；
   *  upgradeBlocked 显式回查原始投影（:908-910——`data(target.id)` 的我方对齐
   *  gw.rawData，绝对 tick > 当前 tick 判 TIRED）；本房 safeMode（:914-916）；
   *  EFFECT_INVULNERABILITY 免疫（:917-919，power 体系 N/A）。 */
  globalThis.Creep.prototype.attackController = function attackController(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (!(target && target.id && target instanceof globalThis.StructureController)) {
      return C.ERR_INVALID_TARGET;
    }
    if (countActiveParts(this.body, C.CLAIM) === 0) return C.ERR_NO_BODYPART;
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (!target.owner && !target.reservation) return C.ERR_INVALID_TARGET;
    if (gw.rawData(target.id).upgradeBlocked > gw.world.time) return C.ERR_TIRED;
    if (blockedBySafeMode(this)) return C.ERR_NO_BODYPART;
    gw.intents.set(this.id, 'attackController', { id: target.id });
    return C.OK;
  };

  /** 依次检查权限、孵化、WORK、能量、控制器类型、升级封锁、3 格距离与目标归属。
   * 检查顺序会决定多个条件同时失败时的返回码。来源：上游 engine/src/game/creeps.js 的 upgradeController。 */
  globalThis.Creep.prototype.upgradeController = function upgradeController(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (countActiveParts(this.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
    if (!this.carry.energy) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (!target || !target.id || !(target instanceof globalThis.StructureController)) {
      return C.ERR_INVALID_TARGET;
    }
    if (target.upgradeBlocked && target.upgradeBlocked > 0) return C.ERR_INVALID_TARGET;
    if (!target.pos.inRangeTo(this.pos, 3)) return C.ERR_NOT_IN_RANGE;
    if (!target.my) return C.ERR_NOT_OWNER;
    if (!target.level || !target.owner) return C.ERR_INVALID_TARGET;
    gw.intents.set(this.id, 'upgradeController', { id: target.id });
    return C.OK;
  };

  globalThis.Creep.prototype.reserveController = function reserveController(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (!(target && target.id && target instanceof globalThis.Structure)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (target.structureType !== 'controller') return C.ERR_INVALID_TARGET;
    if (target.owner) return C.ERR_INVALID_TARGET;
    if (target.reservation && target.reservation.username !== gw.viewerName) {
      return C.ERR_INVALID_TARGET;
    }
    if (countActiveParts(this.body, C.CLAIM) === 0) return C.ERR_NO_BODYPART;
    gw.intents.set(this.id, 'reserveController', { id: target.id });
    return C.OK;
  };

  /** Creep.generateSafeMode：官方 creeps.js:1057-1077。ghodium 消耗门（:1062-
   /*  1064，SAFE_MODE_COST=1000——显式回查原始投影 store，官方 `data(this.id)
   *  .store` 的对齐；无 store/不足皆 NOT_ENOUGH_RESOURCES，无部件门——BODYPARTS
   *  无关）→ 目标 StructureController（:1072-1075）→ 邻接（:1076-1078）→
   *  intents.set。 */
  globalThis.Creep.prototype.generateSafeMode = function generateSafeMode(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    const store = gw.rawData(this.id).store;
    if (!store || !(store[C.RESOURCE_GHODIUM] >= C.SAFE_MODE_COST)) {
      return C.ERR_NOT_ENOUGH_RESOURCES;
    }
    if (!(target && target.id && target instanceof globalThis.StructureController)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'generateSafeMode', { id: target.id });
    return C.OK;
  };

  /** Creep.signController：官方 creeps.js:1080-1100——**无 my 门**（他人 creep
   *  同样可签名，官方原样）。spawning → 目标 Structure 实例（:1083-1086）→
   *  邻接（:1087-1089）→ controller 类型（:1090-1092）→ intents.set 载荷
   *  `sign: ""+sign`（:1094——JS 字符串化：undefined → "undefined" 文本落库
   *  的官方 quirk 原样；清除签名走空串——userString 折叠 + 结算层判空，
   *  intent.rs 既有登记）。 */
  globalThis.Creep.prototype.signController = function signController(target, sign) {
    if (this.spawning) return C.ERR_BUSY;
    if (!(target && target.id && target instanceof globalThis.Structure)) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    if (target.structureType !== 'controller') return C.ERR_INVALID_TARGET;
    gw.intents.set(this.id, 'signController', { id: target.id, sign: '' + sign });
    return C.OK;
  };

  /** 权限与孵化检查后记录字符串消息及布尔 isPublic，say 不计指令费。
   * 消息截断分别由 Rust 指令清洗与结算处理；提前转换 isPublic 避免携带对象的 toJSON。
   * 来源：上游 engine/src/game/creeps.js 的 say 和 intents/system.js 的布尔转换。 */
  globalThis.Creep.prototype.say = function say(message, isPublic) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    // isPublic 出口前布尔折叠（官方 system.js:8 的 boolean transform 在 JS
    // 边界 !!value——我方出口序列化不摘对象自有 toJSON，对象形态可携 1MB
    // 载荷过通道（结算 to_bool 终值兜住）；折叠对齐官方边界语义
    gw.intents.set(this.id, 'say', { message: '' + message, isPublic: !!isPublic });
    return C.OK;
  };

  /** 官方 suicide（creeps.js:813-825）：my/spawning 后记空载荷 intent——
   *  执行器当 tick 即死（官方 processor intents/creeps/suicide.js → _die）。 */
  globalThis.Creep.prototype.suicide = function suicide() {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    gw.intents.set(this.id, 'suicide', {});
    return C.OK;
  };

  /** 官方 pull（creeps.js:1101-1126）：my/spawning 后目标必须是已出生、邻接、
   *  非自身的 Creep（官方 `register.creeps[target.id]` 注册表成员检查的等价
   *  物是 gw.rawById 当前 tick 投影成员；官方 assertTargetObject 的序列化壳
   *  探针不移植）。免费表已含 pull。位移的另一半：被拉 creep 须同 tick 记
   *  move(puller) intent（move 的朝相邻 creep 形态）——官方 movement 配对
   *  结算（执行器 intent_pull 对照 pull.js + movement.js addPulling）。 */
  globalThis.Creep.prototype.pull = function pull(target) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (
      !target ||
      !target.id ||
      !gw.rawById.has(target.id) ||
      !(target instanceof globalThis.Creep) ||
      target.spawning ||
      target.id == this.id
    ) {
      return C.ERR_INVALID_TARGET;
    }
    if (!target.pos.isNearTo(this.pos)) return C.ERR_NOT_IN_RANGE;
    gw.intents.set(this.id, 'pull', { id: target.id });
    return C.OK;
  };

  /** 官方 notifyWhenAttacked（creeps.js:996-1015）：布尔门 + 新鲜读判重
   * （data.notifyWhenAttacked——投影缺省 undefined 时 JS 宽松不等使任何
   * enabled 都过门记 intent，与官方同形）。结算通用段对 creep 生效
   * （processor.js:313）。 */
  globalThis.Creep.prototype.notifyWhenAttacked = function notifyWhenAttacked(enabled) {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.spawning) return C.ERR_BUSY;
    if (typeof enabled !== 'boolean') return C.ERR_INVALID_ARGS;
    if (enabled !== gw.rawData(this.id).notifyWhenAttacked) {
      gw.intents.set(this.id, 'notifyWhenAttacked', { enabled });
    }
    return C.OK;
  };

  // 字段与方法由 Creep.prototype 提供，装配函数保留 wrapObject 创建的基础对象。
  gw.assemblers.creep = (o, base) => base;
})();
