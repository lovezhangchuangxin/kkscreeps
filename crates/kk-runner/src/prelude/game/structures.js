/* 定义 Store、结构基础字段和 isActive，供结构动作实现复用。
 * Store 保留有量的资源键，已知资源缺键读 0，容量方法区分通用容量与资源专用容量。
 * isActive 按控制器等级、所有权和到控制器的距离分配结构名额；等距时按对象扫描顺序决定。
 */
(() => {
  'use strict';

  const { reportMissing } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;
  // 预置 lodash（runtime/shared.js 装配的同一实例，闭包引用——玩家重绑 globalThis._
  // 不影响本段；官方 API 闭包 require('lodash') 的同构）

  // ---------------------------------------------------------------- 预置捕获
  const __Proxy = Proxy;
  const __Object = gw.__Object;
  const __MathMax = Math.max;
  const __MathAbs = Math.abs;
  // 结构名额检查会在玩家执行时收集对象；捕获 push 与 Reflect.apply，避免受玩家替换影响。
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
    // 塔与 link 动作所需的能量、类型及 look 常量。
    TOWER_ENERGY_COST: globalThis.TOWER_ENERGY_COST,
    STRUCTURE_LINK: globalThis.STRUCTURE_LINK,
    STRUCTURE_RAMPART: globalThis.STRUCTURE_RAMPART,
    LOOK_STRUCTURES: globalThis.LOOK_STRUCTURES,
    // terminal 发送使用捕获的能量名称及私有资源白名单。
    RESOURCE_ENERGY: globalThis.RESOURCE_ENERGY,
    // 安全模式检查使用降级阈值表与当前世界控制器状态。
    ERR_NOT_ENOUGH_RESOURCES: globalThis.ERR_NOT_ENOUGH_RESOURCES,
    ERR_TIRED: globalThis.ERR_TIRED,
    ERR_BUSY: globalThis.ERR_BUSY,
    CONTROLLER_DOWNGRADE: freezeConst(globalThis.CONTROLLER_DOWNGRADE),
    CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD: globalThis.CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD,
    // isActive 的配额表（官方 utils.js:470 读 driver.constants 同款注入）
    CONTROLLER_STRUCTURES: freezeConst(globalThis.CONTROLLER_STRUCTURES),
  };
  gw.__structuresC = C; // structure-api.js 复用此常量表。

  // Store 的资源缺省值与容量规则参考上游 engine/src/game/store.js 的 Store。

  /** 累加自有可枚举值；对象缺失时返回 0。
   * 容量数据应由数值组成，此函数不对每个值进行类型校验或归零。 */
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

  /** 创建 Store：资源真值作为自有键，已知资源缺键读 0，其他缺键读 undefined。
   * 方法为非枚举属性；总量 _sum 在实例内首次读取时缓存。
   * 游戏对象的 store getter 还受 registry.js 的字段缓存影响，不能假定每次访问都创建新 Store。
   * 来源：上游 engine/src/game/store.js 的 Store。 */
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
  // writable）：非可写绑定——registry.js 不再装 Store 桩（非可写会挡掉本安装）
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

  // 按控制器等级和结构距离判断是否启用。
  /** 取得结构类型；spawn 通常为 type=spawn，其他结构使用 structureType。
   * 名额扫描还必须比较 raw type，否则工地会被当作现存结构计数。 */
  function structureTypeOf(o) {
    return o.structureType || o.type;
  }

  /** 按同房、同类、同主结构到控制器的 Chebyshev 距离分配名额。
   * 等距对象中，先扫描到的对象占用名额；rawData 与 world.objects 共用对象引用，用引用识别自身。
   * 来源：上游 engine/src/utils.js 的 checkStructureAgainstController。 */
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

    // 配额可覆盖或在换包后降低，RCL8 单件上限不能保证实际只有一件。
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

  /** 无主结构或名额表外类型返回 true；其余需要房间和控制器，再按等级与距离分配名额。
   * 来源：上游 engine/src/game/structures.js 的 Structure.isActive。 */
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

  // Source（官方 sources.js:38-43）。ticksToRegeneration 官方为
  // `nextRegenerationTime ? 剩余 : undefined`——无再生钟保持 undefined，
  // 投影侧已把绝对钟折叠为相对值（无钟为 null）。
  gw.defineGameObjectProperties(globalThis.Source.prototype, gw.rawData, {
    energy: (o) => o.energy ?? 3000,
    energyCapacity: (o) => o.energyCapacity ?? 3000,
    ticksToRegeneration: (o) => (o.ticksToRegeneration == null
      ? undefined
      : o.ticksToRegeneration),
  });

  // 矿物字段读取原始数据；缺失字段使用下方默认值。来源：上游 engine/src/game/minerals.js。
  // ticksToRegeneration 同 source：无钟 undefined（minerals.js:39 真值判）。
  gw.defineGameObjectProperties(globalThis.Mineral.prototype, gw.rawData, {
    mineralType: (o) => o.mineralType || 'XUH2O',
    mineralAmount: (o) => o.mineralAmount ?? 0,
    density: (o) => o.density ?? 3,
    ticksToRegeneration: (o) => (o.ticksToRegeneration == null
      ? undefined
      : o.ticksToRegeneration),
  });

  // 控制器的绝对 tick 字段转换为剩余 tick；结束的 cooldown/upgradeBlocked/safeMode
  // 为 undefined（官方未生效 safeMode 读 undefined，快照序列化为 null）。本实现缺少
  // downgradeTime 时 ticksToDowngrade 为 0，与上游缺省 undefined 不完全相同，
  // activateSafeMode 的预检返回码可能不同。
  gw.defineGameObjectProperties(globalThis.StructureController.prototype, gw.rawData, {
    level: (o) => o.level || 0,
    // RCL 0 不暴露 progress，RCL 0 或 8 不暴露 progressTotal；不要将 undefined 改成 0。
    // 来源：上游 engine/src/game/structures.js 的 StructureController 属性定义。
    progress: (o) => (o.level > 0 ? o.progress : undefined),
    progressTotal: (o) => (o.level > 0 && o.level < 8 ? o.progressTotal : undefined),
    ticksToDowngrade: (o) => (o.downgradeTime != null ? o.downgradeTime - now() : 0),
    safeMode: (o) => (o.safeMode && o.safeMode > now() ? o.safeMode - now() : undefined),
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
          // 从签名时保存的墙钟构造 Date，重复读取同一签名的时间不漂移。
          // 手写世界缺 datetime 时回退到当前墙钟。来源：上游 engine/src/game/structures.js 的 sign getter。
          datetime: o.sign.datetime != null ? new Date(o.sign.datetime) : new Date(),
        }
      : undefined),
    upgradeBlocked: (o) => (o.upgradeBlocked && o.upgradeBlocked > now()
      ? o.upgradeBlocked - now()
      : undefined),
  });

  // 当前 tick（getter 体内换算用——官方 getter 读 runtimeData.time 同款）
  function now() {
    return gw.world.time;
  }
  gw.now = now; // structure-api.js 的 controller 方法复用此时间读取函数。

})();
