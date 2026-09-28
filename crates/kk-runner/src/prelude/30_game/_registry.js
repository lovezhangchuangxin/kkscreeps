/* kk-runner prelude · 30_game 段 1：注册表与装配序（对照官方 engine/src/game/game.js:50-438）。
 *
 * 30_game/ 各文件对照官方 game/ 文件结构组织（A3a 零行为拆分，纯移动）：
 *   _registry → room-position → room → creep → structures → sites-flags
 *   → resources → path-finder → game（buildGame 装配 + tick 生命周期）
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
    // 当前查看者（world.user，官方 runtimeData.user 等价物）——per-viewer
    // 装配（Game.collections/my/FIND_MY_*）的判定基准（外部复审 P1-3 修复）
    viewerName: null,
    // 类型装配注册表：o.type → assembler(o, base, pos, now)（各文件注册，
    // wrapObject 分发——对照官方各 game/ 文件在注册表登记类型）
    assemblers: Object.create(null),
  };

  function mkStubMethod(label) {
    return function () {
      countStub(label);
      return 0; // OK
    };
  }
  gw.mkStubMethod = mkStubMethod;

  // ---------------------------------------------------------------- 类全局（官方全集）
  // 官方运行时把 41 个类挂成裸全局（engine/src/game 各文件 defineProperty(globals,...)），
  // 其中 Spawn、Energy 是别名（官方 structures.js:1293 `globals.Spawn=StructureSpawn`、
  // resources.js:46 `globals.Energy=Resource`）——这里在类注册后直接赋同一引用，
  // spawn 对象 instanceof Spawn 与官方一致。tooangel 等 bot 直接引用 Room 等。
  // M0：桩类——可 new、可静态引用（未知静态成员进缺失 API 探测）。
  // Game 上的对象经原型链接挂在类原型链上（见 wrapObject），instanceof 已成立；
  // 实例属性面的差异由 M2 玩家 API 层对齐。

  const GAME_CLASS_NAMES = [
    'ConstructionSite', 'Creep', 'Deposit', 'Flag', 'Mineral', 'Nuke',
    'OwnedStructure', 'PowerCreep', 'Resource', 'Ruin', 'Room', 'RoomObject',
    'Source', 'Store', 'Structure', 'StructureContainer', 'StructureController',
    'StructureExtension', 'StructureExtractor', 'StructureFactory', 'StructureInvaderCore',
    'StructureKeeperLair', 'StructureLab', 'StructureLink', 'StructureNuker',
    'StructureObserver', 'StructurePortal', 'StructurePowerBank', 'StructurePowerSpawn',
    'StructureRampart', 'StructureRoad', 'StructureSpawn', 'StructureStorage',
    'StructureTerminal', 'StructureTower', 'StructureWall', 'Tombstone',
  ];
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
  for (const name of GAME_CLASS_NAMES) {
    if (globalThis[name] === undefined) globalThis[name] = makeStubClass(name);
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
  globalThis.Spawn = globalThis.StructureSpawn;
  globalThis.Energy = globalThis.Resource;

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

  function wrapObject(o, now) {
    // pos 套探测仪表：RoomPosition 实例面的官方缺口（如 findInRange）以
    // 'RoomPosition.*' 登记，不静默 undefined
    const pos = probe(new gw.RoomPosition(o.x, o.y, o.room), 'RoomPosition');
    const base = {
      id: o.id,
      pos,
      room: undefined, // 由 buildGame 回填（Game.rooms[name]）
      hits: o.hits ?? 0,
      hitsMax: o.hitsMax ?? 0,
    };
    const assemble = gw.assemblers[o.type];
    const obj = assemble ? assemble(o, base, pos, now) : base;
    const cls = classGlobalFor(o);
    if (cls && globalThis[cls] && globalThis[cls].prototype) {
      // 官方语义：Game 对象是类的实例；bot 生态普遍给官方原型打补丁
      //（tooangel 的 prototype_*.js 全系），原型链接通后补丁可见、instanceof 成立
      Object.setPrototypeOf(obj, globalThis[cls].prototype);
    }
    const name = o.name ? ':' + o.name : '';
    return probe(obj, (cls || o.type) + name);
  }
  gw.wrapObject = wrapObject;
})();
