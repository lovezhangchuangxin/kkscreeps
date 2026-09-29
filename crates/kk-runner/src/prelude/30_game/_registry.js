/* kk-runner prelude · 30_game 段 1：注册表与装配序（对照官方 engine/src/game/game.js:50-438）。
 *
 * 30_game/ 各文件对照官方 game/ 文件结构组织（A3a 零行为拆分 + B1 结构治理
 * 批 creep/structures 各拆资源族/spawn 族两段）：
 *   _registry → room-position → room → creep → creep-resources → structures
 *   → structures-spawn → sites-flags → resources → path-finder → game
 *   （buildGame 装配 + tick 生命周期）
 * 各文件独立 IIFE；跨文件共享状态经 __kkp.gw（game-world 容器：世界投影、
 * viewer、intents 收集器引用、类型装配注册表），90_host 执行完删除 __kkp。
 *
 * 本段：gw 容器、类全局装配（官方运行时把 41 个类挂成裸全局——engine/src/game
 * 各文件 defineProperty(globals,...)）、官方 toString 家族、wrapObject 骨架
 * 与 classGlobalFor（makeGameObject 装配序的等价物）。
 */
(() => {
  'use strict';

  const { probe, reportMissing, PROBE_IGNORE, countStub } = globalThis.__kkp;

  // ---------------------------------------------------------------- game-world 共享状态
  // tick 生命周期状态（world/terrainByRoom/objectsById/viewerName）与类型装配
  // 注册表。原 30_memory_world 单闭包 let 状态的拆分形态：各文件经 gw 读写，
  // 赋值/读取时序与拆分前逐点一致（beginTick 在 30_game/game.js 段重建）。
  const gw = globalThis.__kkp.gw = {
    world: { time: 0, cpu: { limit: 20, tickLimit: 500, bucket: 10000 }, rooms: {}, objects: [] },
    terrainByRoom: {},
    objectsById: new Map(),
    // 预置 Object 的最早捕获（game.js 段装配时再赋同值）：先于 game.js 装配的
    // 段（structures.js 的 Store 类等）在装配期即需引用——玩家 tick 内执行
    // 路径一律经它，不得经可交换的 globalThis.Object（对抗评审 P1 全家桶原则）
    __Object: Object,
    // 原始投影 id 索引（官方 runtimeData.roomObjects 的等价物）：beginTick 从
    // world.objects 重建。类原型上的字段 getter/方法经 rawData(id) 回查它——
    // 跨 tick 旧壳的未缓存字段读到当前 tick 值（官方 data 闭包引用模块级
    // 可变 runtimeData 的同构，creeps.js:44-56；"旧引用定格旧值"是错误推断）
    rawById: new Map(),
    // 当前查看者（world.user，官方 runtimeData.user 等价物）——per-viewer
    // 装配（Game.collections/my/FIND_MY_*）的判定基准（外部复审 P1-3 修复）
    viewerName: null,
    // 类型装配注册表：o.type → assembler(o, base, pos, now)（各文件注册，
    // wrapObject 分发——对照官方各 game/ 文件在注册表登记类型）
    assemblers: Object.create(null),
  };

  // 官方各 game/ 文件的 data 闭包等价物（creeps.js:44-56 等全部同形）：
  // 对象消亡（id 不在当前 tick 投影）时抛 runtime error——STALERECV-001
  // 语义的抛错点。文案与官方逐字（"Could not find an object with ID "+id）。
  // mapGet 预置捕获（A3c 装配批）：全部壳 getter/判定门的回查承重面，不得经
  // 可交换的 Map.prototype.get。Reflect.apply 加载期捕获（B0 WP4 .call 家族
  // 清偿——.call 分派走可交换的 Function.prototype.call）
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

  // ---------------------------------------------------------------- defineGameObjectProperties
  // 官方 engine/src/utils.js:509-545 对照移植（行为等价重写，不用官方的 eval）：
  // 把字段表装成 lazy getter + 首访缓存 `_${name}` own 属性。要点全部保留：
  //   · 值计算 properties[name](dataFn(this.id), this.id)——未缓存访问回查
  //     当前 tick 投影（消亡时经 rawData 抛错——A3b 消亡语义的唯一机制）；
  //   · `if(!this['_${name}'])` falsy 不缓存：0/''/null 每次访问重算（官方
  //     quirk 原样保留——同一 tick 内投影不变故无可观测差异，跨 tick 旧壳的
  //     falsy 字段读新值与官方一致）；
  //   · enumerable 默认 true、configurable 默认 false（玩家不可 redefine）、
  //     opts.canSet 时挂 set（写 `_${name}` 缓存）；
  //   · toJSON 挂目标原型：跳过 `_` 前缀缓存键与 toJSON/toString，其余键
  //     （含 getter，访问即求值并缓存）进输出——stringify(壳) 的官方形态。
  // 官方挂在各游戏类 prototype 上（本函数的 obj 参数是 prototype）。
  // keys/defineProperties 预置捕获（评审 2 P2-2）：本函数除类定义期外还在
  // wrapObject 装配路径执行（road/wall/未知结构 else 分支每 tick 走这里）
  const __objectKeys = Object.keys;
  const __objectDefineProperties = Object.defineProperties;
  gw.defineGameObjectProperties = function defineGameObjectProperties(obj, dataFn, properties, opts) {
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
          ? function (value) { this['_' + name] = value; }
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

  // ---------------------------------------------------------------- 类全局（官方全集）
  // 官方运行时把 41 个类挂成裸全局，安装形统一为非可写 defineProperty
  //（engine/src/game 各文件，含别名：structures.js:1293 `defineProperty(globals,
  // 'Spawn',...)`、resources.js:46 Energy——清偿复审 P3-N1 勘误：官方无一 plain
  // 赋值）。别名挂同一引用，spawn 对象 instanceof Spawn 与官方一致。
  // tooangel 等 bot 直接引用 Room 等。
  // M0：桩类——可 new、可静态引用（未知静态成员进缺失 API 探测）。
  // Game 上的对象经原型链接挂在类原型链上（见 wrapObject），instanceof 已成立；
  // 实例属性面的差异由 M2 玩家 API 层对齐。

  const GAME_CLASS_NAMES = [
    'ConstructionSite', 'Creep', 'Deposit', 'Flag', 'Mineral', 'Nuke',
    'OwnedStructure', 'PowerCreep', 'Resource', 'Ruin', 'Room', 'RoomObject',
    'Source', 'Structure', 'StructureContainer', 'StructureController',
    'StructureExtension', 'StructureExtractor', 'StructureFactory', 'StructureInvaderCore',
    'StructureKeeperLair', 'StructureLab', 'StructureLink', 'StructureNuker',
    'StructureObserver', 'StructurePortal', 'StructurePowerBank', 'StructurePowerSpawn',
    'StructureRampart', 'StructureRoad', 'StructureSpawn', 'StructureStorage',
    'StructureTerminal', 'StructureTower', 'StructureWall', 'Tombstone',
  ];
  // Store 不在本表：真类由 structures 段安装（官方 store.js:99 defineProperty
  // 同形）——本表的非可写安装会挡掉那次唯一重绑
  function makeStubClass(name) {
    const ctor = function () {
      countStub('new ' + name);
    };
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
  // 官方安装形（creeps.js:1123 defineProperty(globals,'Creep',{enumerable:true,
  // value}) 无 writable）：类全局绑定不可交换——玩家侧 instanceof/new 走真类，
  // 原型补丁（bot 生态唯一官方支持的扩展形）照常生效（绑定非可写 ≠ 对象冻结）
  for (const name of GAME_CLASS_NAMES) {
    if (globalThis[name] === undefined) {
      Object.defineProperty(globalThis, name, {
        enumerable: true,
        value: makeStubClass(name),
      });
    }
  }
  // 官方类层级（RoomObject 为根；bot 生态跨层级打补丁，如 RoomObject.prototype.ref）：
  // Structure* 大多 extends OwnedStructure，wall/road/container/portal 直接 extends Structure
  const CLASS_PARENTS = {
    Creep: 'RoomObject', PowerCreep: 'RoomObject', Structure: 'RoomObject',
    OwnedStructure: 'Structure', Source: 'RoomObject', Mineral: 'RoomObject',
    Deposit: 'RoomObject', Resource: 'RoomObject', Tombstone: 'RoomObject',
    Ruin: 'RoomObject', Nuke: 'RoomObject', ConstructionSite: 'RoomObject',
    StructureContainer: 'Structure', StructureRoad: 'Structure',
    StructureWall: 'Structure', StructurePortal: 'Structure',
  };
  for (const [cls, parent] of Object.entries(CLASS_PARENTS)) {
    const c = globalThis[cls], p = globalThis[parent];
    if (c && p && c.prototype && p.prototype
      && Object.getPrototypeOf(c.prototype) === Object.prototype) {
      Object.setPrototypeOf(c.prototype, p.prototype);
    }
  }
  for (const name of GAME_CLASS_NAMES) {
    if (name.startsWith('Structure') && name !== 'Structure' && name !== 'OwnedStructure'
      && !CLASS_PARENTS[name]) {
      const c = globalThis[name], p = globalThis.OwnedStructure;
      if (c && p && Object.getPrototypeOf(c.prototype) === Object.prototype) {
        Object.setPrototypeOf(c.prototype, p.prototype);
      }
    }
  }
  // 官方别名：同一引用（structures.js:1293 / resources.js:46），非独立 stub 类
  // 官方别名同为非可写 defineProperty（structures.js:1293/resources.js:46，
  // 清偿复审 P3-N1：原 plain 赋值可被玩家重绑——同一引用、官方同形安装）
  Object.defineProperty(globalThis, 'Spawn', {
    enumerable: true, value: globalThis.StructureSpawn,
  });
  Object.defineProperty(globalThis, 'Energy', {
    enumerable: true, value: globalThis.Resource,
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
    // structures.js:68 / 893（官方 spawn：自家显示名、他家 #id；M0 桩全 my，见正确性-3 登记）
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
      c.prototype.toString = function () { return fmt(this); };
    }
  }

  // ---------------------------------------------------------------- 对象装配（makeGameObject 装配序）
  // 各类型的具体装配在各文件注册（gw.assemblers[type]）；此处持骨架：pos 探测、
  // base 对象、原型链接与整体 probe——对照官方 game.js:50-438 的统一装配序。

  function classGlobalFor(o) {
    switch (o.type) {
      case 'creep': return 'Creep';
      case 'spawn': return 'StructureSpawn';
      case 'flag': return 'Flag';
      case 'constructionSite': return 'ConstructionSite';
      case 'resource': return 'Resource';
      case 'tombstone': return 'Tombstone';
      case 'ruin': return 'Ruin';
      case 'deposit': return 'Deposit';
      case 'powerCreep': return 'PowerCreep';
      case 'nuke': return 'Nuke';
      case 'structure': {
        const m = { controller: 'StructureController', source: 'Source', mineral: 'Mineral',
          storage: 'StructureStorage', terminal: 'StructureTerminal', tower: 'StructureTower',
          extension: 'StructureExtension', link: 'StructureLink', lab: 'StructureLab',
          rampart: 'StructureRampart', road: 'StructureRoad', wall: 'StructureWall',
          constructedWall: 'StructureWall',
          container: 'StructureContainer', extractor: 'StructureExtractor',
          factory: 'StructureFactory', nuker: 'StructureNuker', observer: 'StructureObserver',
          powerBank: 'StructurePowerBank', powerSpawn: 'StructurePowerSpawn',
          portal: 'StructurePortal', invaderCore: 'StructureInvaderCore',
          keeperLair: 'StructureKeeperLair' };
        return m[o.structureType] || 'Structure';
      }
      default: return null;
    }
  }
  gw.classGlobalFor = classGlobalFor;

  // wrapObject 的类解析捕获表（A3c 装配批，评审 2 P2-1 清偿）：官方经引擎
  // 闭包 globals.X 解析（game.js:285 `new globals.Source(i)` 等）——绑定交换
  // 不可见。**加载期立即定格**（本位置已在全部类安装循环之后；类全局非可写
  // 定义后无重绑路径）：惰性定格曾有窗口——game.js 加载期的 refreshGame()
  // 在空世界下零 wrapObject，空世界首 tick（新玩家/全灭玩家每 tick 如此）
  // 玩家代码可先于首次 wrapObject 执行并污染定格表（探针实证）。构造器
  // 引用固定、.prototype 运行期解引用（玩家原型补丁/重挂语义保持）
  const classTable = gw.__Object.create(null);
  const CLASS_TABLE_NAMES = ['Creep', 'StructureSpawn', 'Flag', 'ConstructionSite',
    'Resource', 'Tombstone', 'Ruin', 'Deposit', 'PowerCreep', 'Nuke',
    'StructureController', 'Source', 'Mineral', 'StructureStorage', 'StructureTerminal',
    'StructureTower', 'StructureExtension', 'StructureLink', 'StructureLab',
    'StructureRampart', 'StructureRoad', 'StructureWall', 'StructureContainer',
    'StructureExtractor', 'StructureFactory', 'StructureNuker', 'StructureObserver',
    'StructurePowerBank', 'StructurePowerSpawn', 'StructurePortal',
    'StructureInvaderCore', 'StructureKeeperLair', 'Structure'];
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
    // 官方 own 定格面（构造器语义，rooms.js:1648-1660 RoomObject.call 挂
    // room/pos + 各构造器 this.id = id）：id/pos 构造定格、room 由 buildGame
    // 回填。业务字段（hits/store/name/my...）不再值拷贝——各游戏类原型上的
    // lazy getter 经 rawData(id) 回查（A3b 薄壳化，官方 defineGameObjectProperties
    // 机制），跨 tick 旧壳读当前 tick 值、消亡时抛错
    const base = {
      id: o.id,
      pos,
      room: undefined, // 由 buildGame 回填（Game.rooms[name]）
    };
    const assemble = gw.assemblers[o.type];
    const obj = assemble ? assemble(o, base, pos, now) : base;
    const cls = classGlobalFor(o);
    const ctor = cls ? classCtor(cls) : null;
    if (ctor && ctor.prototype) {
      // 官方语义：Game 对象是类的实例；bot 生态普遍给官方原型打补丁
      //（tooangel 的 prototype_*.js 全系），原型链接通后补丁可见、instanceof 成立
      gw.__Object.setPrototypeOf(obj, ctor.prototype);
    }
    const name = o.name ? ':' + o.name : '';
    return probe(obj, (cls || o.type) + name);
  }
  gw.wrapObject = wrapObject;
})();
