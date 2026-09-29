/* kk-runner prelude · 30_game 段 5a：Structure 族装配（对照官方
 * B1 结构治理批：spawn 族面（Spawning/StructureSpawn 方法族/controller 真方法/
 * 装配/destroy/Tombstone/Ruin）与 per-tick 状态拆至段 5b（structures_spawn.js），
 * 段间共享经 gw 桥（C 表/now/Store），纯搬移零行为。
 * 同迁 5b 的散布定义：deprecated/assertTargetObject/isKnownCreep/
 * calcCreepCost/calcEnergyAvailable（用户全在 5b，闭包自洽零桥）。
 * engine/src/game/structures.js + sources.js + minerals.js + store.js）。
 *
 * A4-i Store 族 + spawn 方法族换真：
 *   · Store 类（官方 store.js:19-101 全语义）：构造挂 truthy 资源键（0 值
 *     不出现）、Proxy 的 RESOURCES_ALL 缺省 0、getCapacity/getUsedCapacity/
 *     getFreeCapacity 三函数族（capacityForResource，utils.js:671-675）、
 *     toString '[store]'；
 *   · StructureSpawn.Spawning（官方 structures.js:1301-1334）：name/needTime/
 *     remainingTime/directions own 定格 + 非 enumerable spawn 引用 +
 *     setDirections/cancel；
 *   · spawnCreep（:1063-1218 全预检序 + 同 tick 幽灵壳）/renewCreep
 *     （:1237-1267）/recycleCreep（:1269-1290）。
 * destroy（A5-lite）/notifyWhenAttacked（A4-iv）已换真；isActive 官方四门
 * 真化、setActive 幽灵 API 摘除（A6——官方无此方法）。
 *
 * A3b 纪律：字段表挂各类原型（官方 defineGameObjectProperties 挂 prototype
 * 的同构）；getter 经 gw.rawData(this.id) 回查当前 tick 投影；方法体官方
 * this.xxx 走 this（getter 通道）、官方 data(this.id).xxx 走 gw.rawData
 * 显式回查；玩家 tick 内执行路径不得经可交换全局（Object/Proxy/Math/
 * 常量面板全部预置捕获——官方 game 层的 C 来自 driver.constants 注入对象，
 * 玩家覆盖全局常量不影响 API 判定，game.js 头部 C = driver.constants 同款）。
 * 依赖：gw.rawData/gw.rawById/gw.objectsById/gw.viewerName/gw.currentGame/
 * gw.world/gw.intents/gw.ensureMemory/gw.RoomPosition/gw.mkStubMethod/
 * gw.defineGameObjectProperties/gw.__Object/gw.perTickReset、__kkp.lodash。
 * A3b 薄壳化：字段表挂各类原型（官方 defineGameObjectProperties 挂 prototype
 * 的同构），实例 own 面只剩 id/pos/room + 少量静态标记。getter 经
 * gw.rawData(this.id) 回查当前 tick 投影（消亡抛错/旧壳读新值）。
 * 值换算口径保持 A1 起的既有语义（None→0 归一等灰区口径不变——A3b 只改
 * 访问机制不改值语义；官方有而现状未暴露的字段（storage.storeCapacity、
 * road.ticksToDecay、reservation/sign 等）随 A4-iii/Store 族批次补）。
 * A4-iv：notifyWhenAttacked 换真（Structure 基类版 + StructureSpawn 覆盖版，
 * 官方 structures.js:89-101/:1220-1233）。
 * 依赖：gw.viewerName、gw.rawData、gw.mkStubMethod、gw.defineGameObjectProperties、
 * gw.intents。
 */
(() => {
  'use strict';

  const { reportMissing } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;
  // 预置 lodash（00_shared 装配的同一实例，闭包引用——玩家重绑 globalThis._
  // 不影响本段；官方 API 闭包 require('lodash') 的同构）

  // ---------------------------------------------------------------- 预置捕获
  const __Proxy = Proxy;
  const __Object = gw.__Object;
  const __MathMax = Math.max;
  const __MathAbs = Math.abs;
  // 数组 push 捕获（isActive 房对象收集经它——官方 _.keys 走私有 lodash，
  // 不经玩家可交换的 Array.prototype；room.js 装配扫描同款纪律）。经加载期
  // 捕获的 Reflect.apply 分派：动态 .call 走 Function.prototype.call 原型，
  // 玩家可换（A6 对抗评审 P2 的家族债之一——本处立 Reflect.apply 标杆，
  // 其余 .call 面家族式清偿归后续批）
  const __arrayPush = Array.prototype.push;
  const __reflectApply = Reflect.apply;

  function freezeConst(obj) {
    if (obj && typeof obj === 'object') {
      Object.freeze(obj);
      for (const k of Object.keys(obj)) freezeConst(obj[k]);
    }
    return obj;
  }

  const C = {
    OK: globalThis.OK,
    ERR_INVALID_ARGS: globalThis.ERR_INVALID_ARGS,
    ERR_NAME_EXISTS: globalThis.ERR_NAME_EXISTS,
    ERR_NOT_OWNER: globalThis.ERR_NOT_OWNER,
    ERR_BUSY: globalThis.ERR_BUSY,
    ERR_RCL_NOT_ENOUGH: globalThis.ERR_RCL_NOT_ENOUGH,
    ERR_NOT_ENOUGH_ENERGY: globalThis.ERR_NOT_ENOUGH_ENERGY,
    ERR_INVALID_TARGET: globalThis.ERR_INVALID_TARGET,
    ERR_NOT_IN_RANGE: globalThis.ERR_NOT_IN_RANGE,
    ERR_FULL: globalThis.ERR_FULL,
    MAX_CREEP_SIZE: globalThis.MAX_CREEP_SIZE,
    CREEP_LIFE_TIME: globalThis.CREEP_LIFE_TIME,
    CREEP_SPAWN_TIME: globalThis.CREEP_SPAWN_TIME,
    SPAWN_RENEW_RATIO: globalThis.SPAWN_RENEW_RATIO,
    CARRY_CAPACITY: globalThis.CARRY_CAPACITY,
    FIND_HOSTILE_CREEPS: globalThis.FIND_HOSTILE_CREEPS,
    FIND_HOSTILE_POWER_CREEPS: globalThis.FIND_HOSTILE_POWER_CREEPS,
    CARRY: globalThis.CARRY,
    CLAIM: globalThis.CLAIM,
    // 私有冻结副本（数组 slice / 对象浅拷）——玩家改写全局常量面板不影响判定
    BODYPARTS_ALL: freezeConst(globalThis.BODYPARTS_ALL.slice()),
    BODYPART_COST: freezeConst(__Object.assign({}, globalThis.BODYPART_COST)),
    RESOURCES_ALL: freezeConst(globalThis.RESOURCES_ALL.slice()),
    // controller 族（A4-iii）：activateSafeMode 门③阈值表与第六门扫描
    ERR_NOT_ENOUGH_RESOURCES: globalThis.ERR_NOT_ENOUGH_RESOURCES,
    ERR_TIRED: globalThis.ERR_TIRED,
    ERR_BUSY: globalThis.ERR_BUSY,
    CONTROLLER_DOWNGRADE: freezeConst(globalThis.CONTROLLER_DOWNGRADE),
    CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD: globalThis.CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD,
    // isActive 的配额表（官方 utils.js:470 读 driver.constants 同款注入）
    CONTROLLER_STRUCTURES: freezeConst(globalThis.CONTROLLER_STRUCTURES),
  };
  gw.__structuresC = C; // B1 段拆分桥：5b（structures_spawn.js）复用同一 C 表对象

  // ---------------------------------------------------------------- Store 类
  // 官方 store.js:19-101 逐行对照（A4-i 换真——原垫片 getUsedCapacity 只认
  // energy/getFreeCapacity 恒 0/getCapacity 恒 null）。

  /** _.sum(object)（官方 store.js 的 _.sum(object.store) / _.sum(storeCapacityResource)）：
   *  自有可枚举值求和；undefined 归 0。 */
  function sumValues(o) {
    if (!o) return 0;
    let sum = 0;
    for (const v of __Object.values(o)) sum += v;
    return sum;
  }

  /** 官方 utils.capacityForResource（utils.js:671-675）：
   *  `storeCapacityResource && storeCapacityResource[resourceType] ||
   *   Math.max(0, (storeCapacity||0) - _.sum(storeCapacityResource))`。 */
  function capacityForResource(object, resourceType) {
    return (
      (object.storeCapacityResource && object.storeCapacityResource[resourceType]) ||
      __MathMax(0, (object.storeCapacity || 0) - sumValues(object.storeCapacityResource))
    );
  }

  /** 官方 Store 构造器（store.js:19-97）：truthy 资源键挂 own（0 值不出现），
   *  三函数族 + toString 以非 enumerable 的 value 属性定义，返回包 this 的
   *  Proxy——RESOURCES_ALL 资源名缺省读 0（而非 undefined），其余未知名
   *  undefined。每次 .store 访问新建实例（官方 _storeGetter），_sum 惰性缓存
   *  （defineProperty value，非 writable）是实例内的。 */
  function Store(object) {
    const storeData = object.store || {};
    for (const [resourceType, resourceAmount] of __Object.entries(storeData)) {
      if (resourceAmount) {
        this[resourceType] = resourceAmount;
      }
    }
    __Object.defineProperties(this, {
      getCapacity: {
        value: function getCapacity(resource) {
          if (!resource) {
            return object.storeCapacityResource ? null : object.storeCapacity || null;
          }
          return capacityForResource(object, resource) || null;
        },
      },
      getUsedCapacity: {
        value: function getUsedCapacity(resource) {
          if (!resource) {
            if (!!object.storeCapacityResource && capacityForResource(object, resource) === 0) {
              return null;
            }
            if (this._sum === undefined) {
              __Object.defineProperty(this, '_sum', { value: sumValues(storeData) });
            }
            return this._sum;
          }
          return storeData[resource] || (!!object.storeCapacityResource && capacityForResource(object, resource) === 0 ? null : 0);
        },
      },
      getFreeCapacity: {
        value: function getFreeCapacity(resource) {
          if (capacityForResource(object, resource) === 0) {
            return null;
          }
          if (!object.storeCapacity) {
            return this.getCapacity(resource) - this.getUsedCapacity(resource);
          }
          const capacity = this.getCapacity(resource);
          if (!capacity) {
            return null;
          }
          if (object.storeCapacityResource) {
            return capacity - this.getUsedCapacity(resource);
          }
          if (this._sum === undefined) {
            __Object.defineProperty(this, '_sum', { value: sumValues(storeData) });
          }
          return capacity - this._sum;
        },
      },
      toString: {
        value: function toString() {
          return '[store]';
        },
      },
    });
    return new __Proxy(this, {
      get(target, name) {
        if (target[name] !== undefined) {
          return target[name];
        }
        if (typeof name === 'string' && C.RESOURCES_ALL.indexOf(name) !== -1) {
          return 0;
        }
        // 仪表：非资源的未知属性访问登记（返回值保持官方 undefined）
        if (typeof name === 'string') reportMissing('store.' + name);
      },
    });
  }
  gw.Store = Store;
  // 官方安装形（store.js:99 Object.defineProperty(globals,'Store',...) 无
  // writable）：非可写绑定——_registry 段不再装 Store 桩（非可写会挡掉本安装）
  Object.defineProperty(globalThis, 'Store', { enumerable: true, value: Store });

  /** 官方 _storeGetter（structures.js:62-64 / creeps.js:37-39）：吃整个 raw
   *  投影对象 o（store/storeCapacity/storeCapacityResource 三键的消费者）。 */
  function makeStore(object) {
    return new Store(object);
  }
  gw.makeStore = makeStore;

  // ---------------------------------------------------------------- 原型字段表
  // Structure 基类（官方 structures.js:60-65）：hits/hitsMax/structureType
  gw.defineGameObjectProperties(globalThis.Structure.prototype, gw.rawData, {
    hits: (o) => o.hits ?? 0,
    hitsMax: (o) => o.hitsMax ?? 0,
    structureType: (o) => o.structureType,
  });

  // ---------------------------------------------------------------- isActive（A6 真化）
  /** raw 投影的结构类型键：typed 变体两键齐发（spawn 投影 type:'spawn' +
   *  structureType:'spawn'）、generic 结构 type:'structure' + structureType
   *  （官方 DB 的 type 键即结构类型，我方投影归一后的等价读取）。距离扫描
   *  判据须另加 `compareObj.type === object.type`（官方 utils.js:481-484
   *  原文）——工地 raw 双键齐备（type:'constructionSite' + structureType）
   *  但 type 永不等于结构类型，被该判定排除、不计入配额；混合形态
   *  （手写 raw 用 generic 臂冒充 typed spawn）两形态 type 不同亦不互算
   *  （放置通道 spawn 恒走 typed 臂，混合形态仅手写世界可达）。 */
  function structureTypeOf(o) {
    return o.structureType || o.type;
  }

  /** 官方 checkStructureAgainstController（utils.js:456-505）逐字：同房同类型
   *  同主结构按到 controller 的 Chebyshev 距离计数，配额耗尽即 inactive；等距
   *  时扫描序在 self 之前的算更近（官方 _.keys(roomObjects) 插入序 = 落库序，
   *  我方 world.objects 数组序 = 放置序，同构）。roomObjects 是本玩家视野内
   *  的本房对象数组（官方 register.objectsByRoom 同为玩家 runtimeData 面，
   *  视野外结构不参与计数）。object === compareObj 引用同一性成立：
   *  rawData(id) 与 world.objects 元素同源（beginTick 直插同引用）。 */
  function checkStructureAgainstController(object, roomObjects, roomController) {
    // owner-less objects are always active
    if (!object.owner) {
      return true;
    }
    // eliminate some other easy cases
    if (!roomController || roomController.level < 1 || roomController.owner !== object.owner) {
      return false;
    }
    const quota = C.CONTROLLER_STRUCTURES[structureTypeOf(object)];

    let allowedRemaining = quota[roomController.level];

    if (allowedRemaining === 0) {
      return false;
    }

    // if only one object ever allowed, this is it
    if (quota[8] === 1) {
      return allowedRemaining !== 0;
    }

    // Scan through the room objects of the same type and count how many are closer.
    let foundSelf = false;
    let objectDist = __MathMax(__MathAbs(object.x - roomController.x), __MathAbs(object.y - roomController.y));
    for (let i = 0; i < roomObjects.length; i++) {
      const compareObj = roomObjects[i];
      if (compareObj.type === object.type && structureTypeOf(compareObj) === structureTypeOf(object)
        && compareObj.owner === object.owner) {
        let compareDist = __MathMax(__MathAbs(compareObj.x - roomController.x), __MathAbs(compareObj.y - roomController.y));

        if (compareDist < objectDist) {
          allowedRemaining--;
          if (allowedRemaining === 0) {
            return false;
          }
        } else if (!foundSelf && compareDist === objectDist) {
          // Objects of equal distance that are discovered before we scan over the selected object are considered closer
          if (object === compareObj) {
            foundSelf = true;
          } else {
            allowedRemaining--;
            if (allowedRemaining === 0) {
              return false;
            }
          }
        }
      }
    }

    return true;
  }

  /** 官方 Structure.isActive（structures.js:108-119）四门：①无主恒 active
   *  （this.owner 走 OwnedStructure getter 通道——plain Structure 子类
   *  （road/wall）原型链无 owner getter 得 undefined，官方同形）②配额表外
   *  类型恒 active ③无房/无 controller 恒 inactive ④checkStructureAgainst
   *  Controller 距离计数。房对象收集用索引迭代（Symbol.iterator 污染防御，
   *  activateSafeMode 门④ 同款纪律）。 */
  globalThis.Structure.prototype.isActive = function isActive() {
    if (!this.owner) {
      return true;
    }
    const o = gw.rawData(this.id);
    if (!C.CONTROLLER_STRUCTURES[structureTypeOf(o)]) {
      return true;
    }
    if (!this.room || !this.room.controller) {
      return false;
    }
    const ctl = gw.rawData(this.room.controller.id);
    const roomName = o.room;
    const all = gw.world.objects || [];
    const roomObjects = [];
    for (let i = 0; i < all.length; i++) {
      if (all[i].room === roomName) {
        __reflectApply(__arrayPush, roomObjects, [all[i]]);
      }
    }
    return checkStructureAgainstController(o, roomObjects, ctl);
  };


  // OwnedStructure（官方 structures.js:135-139）：owner 的 undefined 语义
  // （_.isUndefined(o.user) || o.user === null ? undefined——降级归零的
  // user:null 哨兵同样归 undefined）；my 三态（官方 :139，null 走比较得
  // false——我方 === viewerName 同构）
  gw.defineGameObjectProperties(globalThis.OwnedStructure.prototype, gw.rawData, {
    owner: (o) => (o.owner != null ? { username: o.owner } : undefined),
    my: (o) => (o.owner === undefined ? undefined : o.owner === gw.viewerName),
  });

  // Source（官方 sources.js:38-43）
  gw.defineGameObjectProperties(globalThis.Source.prototype, gw.rawData, {
    energy: (o) => o.energy ?? 3000,
    energyCapacity: (o) => o.energyCapacity ?? 3000,
    ticksToRegeneration: (o) => o.ticksToRegeneration ?? 0,
  });

  // Mineral（官方 minerals.js:40-46）——world_json 暂不投影 mineral 对象，
  // 表先挂（防御分支的现状口径）
  gw.defineGameObjectProperties(globalThis.Mineral.prototype, gw.rawData, {
    mineralType: (o) => o.mineralType || 'XUH2O',
    mineralAmount: (o) => o.mineralAmount ?? 0,
    density: (o) => o.density ?? 3,
    ticksToRegeneration: (o) => o.ticksToRegeneration ?? 0,
  });

  // Controller（官方 structures.js:139-174）。A4-iii 投影清偿：reservation/
  // sign/upgradeBlocked/safeModeCooldown 四 getter 换官方真形态（绝对 tick
  // 挂钟 → `> time ? 差值 : undefined`；{user,endTime} → {username,
  // ticksToEnd}——官方 :176-179/:185/:187-197）。ticksToDowngrade/safeMode
  // 维持既有 None→0 口径（灰区表登记，A3b 只换访问机制）——注意
  // activateSafeMode 门③ 读 this.ticksToDowngrade 时无钟房 0 < 阈值为真
  // （官方 undefined < 阈值为假——API 返回码差异，登记报告；终态一致：结算
  // 侧 B2 对无钟房同拒）。
  gw.defineGameObjectProperties(globalThis.StructureController.prototype, gw.rawData, {
    level: (o) => o.level || 0,
    // 官方 structures.js:180-181：RCL 0 的 progress/progressTotal 与 RCL 8 的
    // progressTotal 均 undefined（GPT 复审 P2-4 清偿——`|| 0` 折叠了缺省态）
    progress: (o) => (o.level > 0 ? o.progress : undefined),
    progressTotal: (o) => (o.level > 0 && o.level < 8 ? o.progressTotal : undefined),
    ticksToDowngrade: (o) => (o.downgradeTime != null ? o.downgradeTime - now() : 0),
    safeMode: (o) => (o.safeMode && o.safeMode > now() ? o.safeMode - now() : 0),
    safeModeAvailable: (o) => o.safeModeAvailable ?? 0,
    safeModeCooldown: (o) => (o.safeModeCooldown && o.safeModeCooldown > now()
      ? o.safeModeCooldown - now()
      : undefined),
    isPowerEnabled: (o) => o.isPowerEnabled === true,
    reservation: (o) => (o.reservation
      ? { username: o.reservation.user, ticksToEnd: o.reservation.endTime - now() }
      : undefined),
    sign: (o) => (o.sign
      ? {
          username: o.sign.user,
          text: o.sign.text,
          time: o.sign.time,
          // 官方第四字段 datetime: new Date(o.sign.datetime)（structures.js:192）
          // ——签名结算时落一次墙钟（signController.js:23，GPT 复审灰区纠错
          // 清偿：从持久值构造，同一签名的两次读取不再漂移；缺省回落当前
          // 墙钟是手写世界无 datetime 的防御，官方不可达）
          datetime: o.sign.datetime != null ? new Date(o.sign.datetime) : new Date(),
        }
      : undefined),
    upgradeBlocked: (o) => (o.upgradeBlocked && o.upgradeBlocked > now()
      ? o.upgradeBlocked - now()
      : undefined),
    activationDate: () => undefined,
  });

  // 当前 tick（getter 体内换算用——官方 getter 读 runtimeData.time 同款）
  function now() {
    return gw.world.time;
  }
  gw.now = now; // B1 段拆分桥：5b 复用同一函数（controller 真方法的 safeMode 门读数）

})();
