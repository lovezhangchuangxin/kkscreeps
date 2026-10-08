/* 保存当前 tick 的世界数据、对象索引、查看者和类型装配函数。
 * wrapObject 创建 id/pos/room，再调用装配函数并连接类原型；probe 记录缺失访问。
 * 字段 getter 首次读取时计算并缓存真值；未缓存字段通过 rawData 读取当前 tick，
 * 已消亡 id 会抛错。部分构造器仅记录调用，并未实现完整的玩家手动构造行为。
 */
(() => {
  'use strict';

  const { probe, reportMissing, PROBE_IGNORE, countStub } = globalThis.__kkp;

  // 各段共用此容器；beginTick 更新世界、查看者及索引，类装配函数在加载时注册。
  const gw = (globalThis.__kkp.gw = {
    world: { time: 0, cpu: { limit: 20, tickLimit: 500, bucket: 10000 }, rooms: {}, objects: [] },
    terrainByRoom: {},
    objectsById: new Map(),
    // 较早加载的 Store 等实现也需要 Object 引用，不能等到 game.js 再捕获。
    __Object: Object,
    // 每 tick 重建的原始数据索引；未缓存的对象字段通过 rawData 回查它。
    rawById: new Map(),
    // 当前查看者（world.user，官方 runtimeData.user 等价物）——per-viewer
    // 装配（Game.collections/my/FIND_MY_*）的判定基准（外部复审 P1-3 修复）
    viewerName: null,
    // 类型装配注册表：o.type → assembler(o, base, pos, now)（各文件注册，
    // wrapObject 分发——对照官方各 game/ 文件在注册表登记类型）
    assemblers: Object.create(null),
  });

  // rawData 查不到当前 tick 的 id 时抛错，防止消亡对象继续读取陈旧数据。
  // 捕获 Map.get 与 Reflect.apply，回查不受玩家替换这些方法影响。
  // 来源：上游 engine/src/game/creeps.js 的 data 函数。
  const __mapGet = Map.prototype.get;
  const __reflectApply = Reflect.apply;
  gw.rawData = function rawData(id) {
    const o = __reflectApply(__mapGet, gw.rawById, [id]);
    if (o === undefined) {
      throw new Error('Could not find an object with ID ' + id);
    }
    return o;
  };

  function mkStubMethod(label) {
    return function () {
      countStub(label);
      return 0; // OK
    };
  }
  gw.mkStubMethod = mkStubMethod;

  // 字段 getter 计算 properties[name](dataFn(this.id),this.id)，以 _name 保存真值。
  // 0、空串、null 等假值不会命中缓存，跨 tick 的旧对象因此可能重新读取这些字段。
  // enumerable 缺省 true、configurable 缺省 false；canSet 允许写入 _name。
  // toJSON 枚举自身和继承属性，排除 _ 前缀、toJSON/toString，其余读取可能触发 getter。
  // 此函数用于类原型和部分实例，不应假定 obj 一定是 prototype。
  // 来源：上游 engine/src/utils.js 的 defineGameObjectProperties。
  const __objectKeys = Object.keys;
  const __objectDefineProperties = Object.defineProperties;
  gw.defineGameObjectProperties = function defineGameObjectProperties(
    obj,
    dataFn,
    properties,
    opts,
  ) {
    opts = opts || {};
    const enumerable = opts.enumerable === undefined ? true : !!opts.enumerable;
    const info = {};
    const propNames = __objectKeys(properties);
    for (let pi = 0; pi < propNames.length; pi++) {
      const name = propNames[pi];
      const compute = properties[name];
      info[name] = {
        configurable: !!opts.configurable,
        enumerable,
        get() {
          if (!this['_' + name]) {
            this['_' + name] = compute(dataFn(this.id), this.id);
          }
          return this['_' + name];
        },
        set: opts.canSet
          ? function (value) {
              this['_' + name] = value;
            }
          : undefined,
      };
    }
    __objectDefineProperties(obj, info);
    obj.toJSON = function () {
      const result = {};
      for (const i in this) {
        if (i[0] === '_' || i === 'toJSON' || i === 'toString') continue;
        result[i] = this[i];
      }
      return result;
    };
  };

  // 先安装游戏类的非可写全局绑定，再建立原型继承关系。
  // 绑定不能替换，但类对象与原型没有因此冻结，玩家仍能补充原型方法。

  const GAME_CLASS_NAMES = [
    'ConstructionSite',
    'Creep',
    'Deposit',
    'Flag',
    'Mineral',
    'Nuke',
    'OwnedStructure',
    'PowerCreep',
    'Resource',
    'Ruin',
    'Room',
    'RoomObject',
    'Source',
    'Structure',
    'StructureContainer',
    'StructureController',
    'StructureExtension',
    'StructureExtractor',
    'StructureFactory',
    'StructureInvaderCore',
    'StructureKeeperLair',
    'StructureLab',
    'StructureLink',
    'StructureNuker',
    'StructureObserver',
    'StructurePortal',
    'StructurePowerBank',
    'StructurePowerSpawn',
    'StructureRampart',
    'StructureRoad',
    'StructureSpawn',
    'StructureStorage',
    'StructureTerminal',
    'StructureTower',
    'StructureWall',
    'Tombstone',
  ];
  // Store 不在本表：真类由 structures 段安装（官方 store.js:99 defineProperty
  // 同形）——本表的非可写安装会挡掉那次唯一重绑
  //
  // id 构造器（官方 game 各文件的 class 定义形）按类分四款签名：
  // - guardedId（Creep/Structure/OwnedStructure/Structure*，creeps.js:60-65、
  //   structures.js:39-56）：`if(id)` 假参空实例；Structure 族先挂 id 再挂
  //   room/pos（官方键序），Creep 族反之。
  // - strictId（Source/Mineral/Deposit/Resource/Tombstone/Ruin/Nuke/
  //   ConstructionSite，sources.js:19-31 等）：无守卫，假参走 rawData 抛
  //   官方文案（`Could not find an object with ID undefined`）。
  // - powerCreep（power-creeps.js:34-40）：不抛、无条件挂 id，仅记录带
  //   room 时才挂 room/pos（power 域未启用，语义先行对齐）。
  // - flag（flags.js:20-26）：六参 (name,color,secondaryColor,roomName,x,y)，
  //   不挂 id——sites-flags.js 已按此口径装配旗实例。
  // 位置初始化与官方 RoomObject 构造器（rooms.js:1648-1658）同构：room 取
  // 当前 Game.rooms 实例（官方 register.rooms 同款引用语义）、pos 官方
  // RoomPosition、effects 真值即挂（空数组也挂空表——官方同款）。
  // 已知有意分歧：官方 Structure 构造器会重指 room.controller/storage/
  // terminal 到新实例（structures.js:46/50/54），本实现不重指——装配实例
  // 与 getObjectById 保持同引用；官方手动构造会翻转该身份，无套件或
  // bot 依赖，登记缺口。
  const ROOM_OBJECT_EFFECT_KEYS = ['power', 'effect', 'level', 'ticksRemaining'];
  function roomObjectInit(x, y, roomName, effects) {
    const game = gw.currentGame && gw.currentGame();
    this.room = game && game.rooms ? game.rooms[roomName] : undefined;
    this.pos = new gw.RoomPosition(x, y, roomName);
    initEffects.call(this, effects);
  }
  // 手动构造和 Game 装配都要附带脱离权威记录的效果数组。
  function initEffects(effects) {
    if (effects) {
      const live = [];
      for (let i = 0; i < effects.length; i++) {
        const e = effects[i];
        if (e && e.ticksRemaining > 0) {
          const item = {};
          for (let k = 0; k < ROOM_OBJECT_EFFECT_KEYS.length; k++) {
            item[ROOM_OBJECT_EFFECT_KEYS[k]] = e[ROOM_OBJECT_EFFECT_KEYS[k]];
          }
          live.push(item);
        }
      }
      this.effects = live;
    }
  }
  function classShell(name, ctor) {
    Object.defineProperty(ctor, 'name', { value: name });
    return new Proxy(ctor, {
      get(t, prop) {
        if (typeof prop === 'string' && !(prop in t) && !PROBE_IGNORE.has(prop)) {
          reportMissing(name + '.' + prop);
        }
        return t[prop];
      },
    });
  }
  const STRICT_ID_CLASSES = new Set([
    'Source',
    'Mineral',
    'Deposit',
    'Resource',
    'Tombstone',
    'Ruin',
    'Nuke',
    'ConstructionSite',
  ]);
  function makeGameObjectClass(name, signature) {
    let ctor;
    if (signature === 'roomObjectArgs') {
      ctor = function (x, y, roomName, effects) {
        roomObjectInit.call(this, x, y, roomName, effects);
      };
    } else if (signature === 'flag') {
      ctor = function (flagName, color, secondaryColor, roomName, x, y) {
        roomObjectInit.call(this, Number(x), Number(y), roomName);
        this.name = flagName;
        this.color = Number(color);
        this.secondaryColor = Number(secondaryColor || color);
      };
    } else if (signature === 'powerCreep') {
      ctor = function (id) {
        const raw = __reflectApply(__mapGet, gw.rawById, [id]);
        if (raw && raw.room) {
          roomObjectInit.call(this, raw.x, raw.y, raw.room);
        }
        this.id = id;
      };
    } else {
      const strict = STRICT_ID_CLASSES.has(name);
      const idFirst =
        name === 'Structure' || name === 'OwnedStructure' || name.startsWith('Structure');
      ctor = function (id) {
        if (strict || id) {
          const raw = gw.rawData(id);
          if (idFirst) this.id = id;
          roomObjectInit.call(this, raw.x, raw.y, raw.room, raw.effects);
          if (!idFirst) this.id = id;
        }
      };
    }
    return classShell(name, ctor);
  }
  function makeStubClass(name) {
    return classShell(name, function () {
      countStub('new ' + name);
    });
  }
  // 官方安装形（creeps.js:1123 defineProperty(globals,'Creep',{enumerable:true,
  // value}) 无 writable）：类全局绑定不可交换——玩家侧 instanceof/new 走真类，
  // 原型补丁（bot 生态唯一官方支持的扩展形）照常生效（绑定非可写 ≠ 对象冻结）
  for (const name of GAME_CLASS_NAMES) {
    if (globalThis[name] === undefined) {
      const signature =
        name === 'RoomObject'
          ? 'roomObjectArgs'
          : name === 'Room'
            ? undefined
            : name === 'Flag'
              ? 'flag'
              : name === 'PowerCreep'
                ? 'powerCreep'
                : 'id';
      Object.defineProperty(globalThis, name, {
        enumerable: true,
        value: signature === undefined ? makeStubClass(name) : makeGameObjectClass(name, signature),
      });
    }
  }
  // 官方类层级（RoomObject 为根；bot 生态跨层级打补丁，如 RoomObject.prototype.ref）：
  // Structure* 大多 extends OwnedStructure，wall/road/container/portal 直接 extends Structure
  const CLASS_PARENTS = {
    Creep: 'RoomObject',
    PowerCreep: 'RoomObject',
    Structure: 'RoomObject',
    OwnedStructure: 'Structure',
    Source: 'RoomObject',
    Mineral: 'RoomObject',
    Deposit: 'RoomObject',
    Resource: 'RoomObject',
    Tombstone: 'RoomObject',
    Ruin: 'RoomObject',
    Nuke: 'RoomObject',
    ConstructionSite: 'RoomObject',
    StructureContainer: 'Structure',
    StructureRoad: 'Structure',
    StructureWall: 'Structure',
    StructurePortal: 'Structure',
  };
  for (const [cls, parent] of Object.entries(CLASS_PARENTS)) {
    const c = globalThis[cls],
      p = globalThis[parent];
    if (
      c &&
      p &&
      c.prototype &&
      p.prototype &&
      Object.getPrototypeOf(c.prototype) === Object.prototype
    ) {
      Object.setPrototypeOf(c.prototype, p.prototype);
    }
  }
  for (const name of GAME_CLASS_NAMES) {
    if (
      name.startsWith('Structure') &&
      name !== 'Structure' &&
      name !== 'OwnedStructure' &&
      !CLASS_PARENTS[name]
    ) {
      const c = globalThis[name],
        p = globalThis.OwnedStructure;
      if (c && p && Object.getPrototypeOf(c.prototype) === Object.prototype) {
        Object.setPrototypeOf(c.prototype, p.prototype);
      }
    }
  }
  // Spawn 与 StructureSpawn、Energy 与 Resource 分别共用构造器引用，绑定不可写。
  // 来源：上游 engine/src/game/structures.js 与 resources.js 的别名定义。
  Object.defineProperty(globalThis, 'Spawn', {
    enumerable: true,
    value: globalThis.StructureSpawn,
  });
  Object.defineProperty(globalThis, 'Energy', {
    enumerable: true,
    value: globalThis.Resource,
  });

  // 官方 toString 家族（engine/src/game 各文件 prototype.toString，bot 生态日志
  // 拼接的事实标准格式）：缺失时 String(obj) 得 '[object Object]'。挂类原型，
  // wrapObject 的对象经 setPrototypeOf 自动获得；未列出的 Structure 子类按官方
  // 同构继承 Structure 根的 `[structure (type) #id]`。
  const OFFICIAL_TOSTRING = {
    // creeps.js:122 / power-creeps.js:102
    Creep: (o) => '[creep ' + o.name + ']',
    PowerCreep: (o) => '[powerCreep ' + o.name + ']',
    // rooms.js:536 / flags.js:53
    Room: (o) => '[room ' + o.name + ']',
    Flag: (o) => '[flag ' + o.name + ']',
    // Structure 使用类型与 id；当前 spawn 格式统一显示名称，包括他人 spawn。
    Structure: (o) => '[structure (' + o.structureType + ') #' + o.id + ']',
    StructureSpawn: (o) => '[spawn ' + o.name + ']',
    // sources.js:41 / minerals.js:42 / resources.js:41 / deposits.js:40
    Source: (o) => '[source #' + o.id + ']',
    Mineral: (o) => '[mineral (' + o.mineralType + ') #' + o.id + ']',
    Resource: (o) => '[resource (' + o.resourceType + ') #' + o.id + ']',
    Deposit: (o) => '[deposit (' + o.depositType + ') #' + o.id + ']',
    // construction-sites.js:46 / tombstones.js:236 / ruins.js:68 / nukes.js:40
    ConstructionSite: (o) => '[construction site (' + o.structureType + ') #' + o.id + ']',
    Tombstone: (o) => '[Tombstone #' + o.id + ']',
    Ruin: (o) => '[ruin (' + (o.structure && o.structure.structureType) + ') #' + o.id + ']',
    Nuke: (o) => '[nuke #' + o.id + ']',
  };
  for (const [name, fmt] of Object.entries(OFFICIAL_TOSTRING)) {
    const c = globalThis[name];
    if (c && c.prototype) {
      c.prototype.toString = function () {
        return fmt(this);
      };
    }
  }

  // ---------------------------------------------------------------- 对象装配（makeGameObject 装配序）
  // 各类型的具体装配在各文件注册（gw.assemblers[type]）；此处持骨架：pos 探测、
  // base 对象、原型链接与整体 probe——对照官方 game.js:50-438 的统一装配序。

  function classGlobalFor(o) {
    switch (o.type) {
      case 'creep':
        return 'Creep';
      case 'spawn':
        return 'StructureSpawn';
      case 'flag':
        return 'Flag';
      case 'constructionSite':
        return 'ConstructionSite';
      case 'resource':
        return 'Resource';
      case 'tombstone':
        return 'Tombstone';
      case 'ruin':
        return 'Ruin';
      case 'deposit':
        return 'Deposit';
      case 'powerCreep':
        return 'PowerCreep';
      case 'nuke':
        return 'Nuke';
      case 'structure': {
        const m = {
          controller: 'StructureController',
          source: 'Source',
          mineral: 'Mineral',
          storage: 'StructureStorage',
          terminal: 'StructureTerminal',
          tower: 'StructureTower',
          extension: 'StructureExtension',
          link: 'StructureLink',
          lab: 'StructureLab',
          rampart: 'StructureRampart',
          road: 'StructureRoad',
          wall: 'StructureWall',
          constructedWall: 'StructureWall',
          container: 'StructureContainer',
          extractor: 'StructureExtractor',
          factory: 'StructureFactory',
          nuker: 'StructureNuker',
          observer: 'StructureObserver',
          powerBank: 'StructurePowerBank',
          powerSpawn: 'StructurePowerSpawn',
          portal: 'StructurePortal',
          invaderCore: 'StructureInvaderCore',
          keeperLair: 'StructureKeeperLair',
        };
        return m[o.structureType] || 'Structure';
      }
      default:
        return null;
    }
  }
  gw.classGlobalFor = classGlobalFor;

  // 装配期捕获构造器引用，避免首次出现某种对象时才从玩家全局查找。
  // 使用时仍读取构造器的 prototype，因此玩家的原型扩展可见。
  const classTable = gw.__Object.create(null);
  const CLASS_TABLE_NAMES = [
    'Creep',
    'StructureSpawn',
    'Flag',
    'ConstructionSite',
    'Resource',
    'Tombstone',
    'Ruin',
    'Deposit',
    'PowerCreep',
    'Nuke',
    'StructureController',
    'Source',
    'Mineral',
    'StructureStorage',
    'StructureTerminal',
    'StructureTower',
    'StructureExtension',
    'StructureLink',
    'StructureLab',
    'StructureRampart',
    'StructureRoad',
    'StructureWall',
    'StructureContainer',
    'StructureExtractor',
    'StructureFactory',
    'StructureNuker',
    'StructureObserver',
    'StructurePowerBank',
    'StructurePowerSpawn',
    'StructurePortal',
    'StructureInvaderCore',
    'StructureKeeperLair',
    'Structure',
  ];
  for (let i = 0; i < CLASS_TABLE_NAMES.length; i++) {
    classTable[CLASS_TABLE_NAMES[i]] = globalThis[CLASS_TABLE_NAMES[i]];
  }
  function classCtor(cls) {
    return classTable[cls];
  }

  function wrapObject(o, now) {
    // pos 套探测仪表：RoomPosition 实例面的官方缺口（如 findInRange）以
    // 'RoomPosition.*' 登记，不静默 undefined
    const pos = probe(new gw.RoomPosition(o.x, o.y, o.room), 'RoomPosition');
    // id 与 pos 在装配时保存，room 随后由 buildGame 回填。
    // 业务字段放在原型 getter 上；真值缓存后保留旧值，未缓存字段回查当前 tick。
    const base = {
      id: o.id,
      pos,
      room: undefined, // 由 buildGame 回填（Game.rooms[name]）
    };
    // Mod 的 effects 可是声明的业务字段，不能覆盖其玩家面 getter。
    if (o.type !== 'mod') initEffects.call(base, o.effects);
    const assemble = gw.assemblers[o.type];
    const obj = assemble ? assemble(o, base, pos, now) : base;
    // Mod 对象的类由玩家模块安装（gw.modApi.types[typeKey].cls）；探测
    // 标签用类型键。未安装面（防御：vanilla 世界不产 mod 记录）得到
    // 无原型 base。
    let ctor = null;
    let label;
    if (o.type === 'mod') {
      const entry = gw.modApi === undefined ? undefined : gw.modApi.types[o.modType];
      ctor = entry === undefined ? null : entry.cls;
      label = o.modType;
    } else if (
      o.type === 'structure' &&
      gw.modApi !== undefined &&
      gw.modApi.buildings !== undefined &&
      gw.modApi.buildings[o.structureType] !== undefined
    ) {
      // Mod 建筑的玩家类由玩家模块安装（gw.modApi.buildings[key].cls）。
      const entry = gw.modApi.buildings[o.structureType];
      ctor = entry.cls;
      label = o.structureType;
    } else {
      const cls = classGlobalFor(o);
      ctor = cls ? classCtor(cls) : null;
      label = cls || o.type;
    }
    if (ctor && ctor.prototype) {
      // 官方语义：Game 对象是类的实例；bot 生态普遍给官方原型打补丁
      //（tooangel 的 prototype_*.js 全系），原型链接通后补丁可见、instanceof 成立
      gw.__Object.setPrototypeOf(obj, ctor.prototype);
    }
    const name = o.name ? ':' + o.name : '';
    return probe(obj, label + name);
  }
  gw.wrapObject = wrapObject;
})();
