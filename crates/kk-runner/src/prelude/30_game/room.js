/* kk-runner prelude · 30_game 段 3：Room 面（对照官方 engine/src/game/rooms.js
 * ——RoomVisual :1138-1220、Room.Terrain :1223-1241、Room 类实例面约 :500-980：
 * toString :536、memory :551、find filter :648-654、lookAt :768）。
 *
 * Terrain/terrainWordAt（房间地形，官方 Room.Terrain 闭包式对象）、RoomVisual 桩、
 * makeRoom 装配工厂（find 与 look 系、memory getter 等实例面）。
 * 依赖：gw.terrainByRoom（beginTick 重建）、gw.ensureMemory（memory getter 懒解析）、
 * gw.objectsById/gw.world（find/装配）、RoomPosition（全局，room-position 段已挂）。
 */
(() => {
  'use strict';

  const { countStub, probe } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 预置捕获（A3c 装配批，同 game.js 原则）：makeRoom 每 tick 装配，Object
  // 交换会使原型链接/memory 定义走假实现；Room 构造器捕获自安装位（官方
  // globals.Room 闭包引用等价——.prototype 运行期解引用，玩家原型补丁/
  // 重挂语义保持可见，绑定交换不可）
  const __Object = Object;
  const __RoomCtor = globalThis.Room;
  const __mapGet = Map.prototype.get;
  const __jsonStringify = JSON.stringify;

  // ---------------------------------------------------------------- Terrain
  // 官方 rooms.js:1223-1241 Room.Terrain 逐字移植：闭包式对象（非 class），
  // 无地形数据的房间构造即抛错；get 返回位码（wall=1/swamp=2/plain=0），
  // getRawBuffer 填充给定 TypedArray或新建 Uint8Array。地形数据经世界 JSON
  // 注入（terrainByRoom，beginTick 填充）。

  function Terrain(roomName) {
    const array = gw.terrainByRoom['' + roomName];
    if (!array) throw new Error('Could not access room ' + roomName);
    this.get = function (x, y) {
      const value = array[y * 50 + x];
      return (value & TERRAIN_MASK_WALL) || (value & TERRAIN_MASK_SWAMP) || 0;
    };
    this.getRawBuffer = function (destinationArray) {
      if (destinationArray) {
        destinationArray.set(array);
        return destinationArray;
      }
      return new Uint8Array(array);
    };
  }
  // 官方 Terrain 挂法：Room 的静态属性（rooms.js:1135 `new Room.Terrain(...)`），
  // 非裸全局
  globalThis.Room.Terrain = Terrain;
  gw.Terrain = Terrain;

  // 地形单词（look 面）：swamp 先判、wall 后覆盖（rooms.js:663-671）。
  // 无地形数据的房间（玩家自造未注册房间名）自然 TypeError——与官方
  // _lookSpatialRegister 直读 staticTerrainData[id]（rooms.js:665）的行为一致；
  // 世界内全部房间恒有地形（world_json 恒发 terrain）
  function terrainWordAt(roomName, x, y) {
    const t = gw.terrainByRoom[roomName][y * 50 + x];
    let result = 'plain';
    if (t & TERRAIN_MASK_SWAMP) result = 'swamp';
    if (t & TERRAIN_MASK_WALL) result = 'wall';
    return result;
  }
  gw.terrainWordAt = terrainWordAt;

  // ---------------------------------------------------------------- RoomVisual 桩

  class RoomVisual {
    constructor(roomName) { this.roomName = roomName; }
  }
  // 绘制方法全部 no-op：用 Proxy 原型惰性生成（class 的 prototype 不可写，故借普通函数桥接）
  const visualBridge = function VisualBridge() {};
  visualBridge.prototype = new Proxy({}, {
    get(t, prop) {
      if (prop in t) return t[prop];
      // 官方 rooms.js:1157：绘制方法返回 this（链式调用是官方文档承诺的行为）
      if (typeof prop === 'string') {
        return function () { countStub('RoomVisual.' + prop); return this; };
      }
      return undefined;
    },
  });
  Object.setPrototypeOf(RoomVisual.prototype, visualBridge.prototype);
  // 官方 rooms.js:1220 同为非可写 defineProperty（清偿复审 P3-N1）
  Object.defineProperty(globalThis, 'RoomVisual', {
    enumerable: true, value: RoomVisual,
  });

  // ---------------------------------------------------------------- Room 装配
  // 对照官方 rooms.js Room 类实例面（find/lookAt/lookForAt/getTerrain/memory...）。

  function makeRoom(name, r) {
    // 无 controller 房间的官方投影是 undefined（非 null）
    let controller;
    const worldObjs = gw.world.objects || [];
    for (let i = 0; i < worldObjs.length; i++) {
      const o = worldObjs[i];
      if (o.room === name && o.type === 'structure' && o.structureType === 'controller') {
        controller = __mapGet.call(gw.objectsById, o.id);
      }
    }
    const room = {
      name,
      controller,
      // 官方 game.js:325-328：非 off 的 spawn/extension 计入（spawn 300 固定、
      // extension 按 RCL 表）——由 Rust 侧 world_json 房间投影携带
      energyAvailable: r.energyAvailable ?? 0,
      energyCapacityAvailable: r.energyCapacityAvailable ?? 300,
      storage: undefined,
      terminal: undefined,
      visual: new RoomVisual(name),
      mode: 'world',
    };
    room.find = function (type, opts) {
      countStub('room.find:' + type);
      const out = [];
      // 索引迭代（装配批纪律）：for...of 经可交换的 Symbol.iterator。
      // 裸 run（viewerName=null，无 user 的注入面）= 视角中立面：与 Rust
      // matches_find 的 None 臂同构——MY_*/HOSTILE_* 不可判定返回空、
      // 101 保留孵化雏形（评审 1 F-1：两层中立语义同步收口）
      const neutral = gw.viewerName === null;
      for (let i = 0; i < worldObjs.length; i++) {
        const o = worldObjs[i];
        if (o.room !== name) continue;
        const w = __mapGet.call(gw.objectsById, o.id);
        if (!w) continue;
        const isStruct = o.type === 'structure' || o.type === 'spawn';
        // 官方 findCacheFn 表（game.js:17-29）viewer 相关 10 臂：FIND_CREEPS
        // 族滤孵化雏形（!i.spawning——中立面保留）；MY_*=真值 my、HOSTILE_*
        // =严格 false 混合（可达状态等价——creep/spawn owner 恒字符串、site
        // null 投影缺键）；109 官方另有 `&& i.owner`（owner getter 对 null/
        // 缺键归 undefined，structures.js:136-138）→ 敌对结构臂只认字符串
        // owner（与 Rust hostile_of 同口径；cleared controller 的 owner:null
        // 双层双臂均不进）
        const creepOk = o.type === 'creep' && (neutral || !o.spawning);
        if (type === 101 /* FIND_CREEPS */ && creepOk) out.push(w);
        else if (type === 102 /* FIND_MY_CREEPS */ && !neutral && creepOk && o.owner === gw.viewerName) out.push(w);
        else if (type === 103 /* FIND_HOSTILE_CREEPS */ && !neutral && creepOk && typeof o.owner === 'string' && o.owner !== gw.viewerName) out.push(w);
        else if (type === 104 /* FIND_SOURCES_ACTIVE */ && o.type === 'structure' && o.structureType === 'source' && (o.energy ?? 3000) > 0) out.push(w);
        else if (type === 105 /* FIND_SOURCES */ && o.type === 'structure' && o.structureType === 'source') out.push(w);
        else if (type === 106 /* FIND_DROPPED_RESOURCES */ && o.type === 'resource') out.push(w);
        // 官方 FIND_STRUCTURES/FIND_MY_STRUCTURES 不含 source/mineral：
        // game.js:229-251 structureTypes 表无此三类（独立装配分支 :329-345），
        // 与 Rust 侧 matches_find 两层一致
        else if (type === 107 /* FIND_STRUCTURES */ && isStruct && o.structureType !== 'source' && o.structureType !== 'mineral') out.push(w);
        else if (type === 108 /* FIND_MY_STRUCTURES */ && !neutral && isStruct && o.structureType !== 'source' && o.structureType !== 'mineral' && o.owner === gw.viewerName) out.push(w);
        else if (type === 109 /* FIND_HOSTILE_STRUCTURES */ && !neutral && isStruct && o.structureType !== 'source' && o.structureType !== 'mineral' && typeof o.owner === 'string' && o.owner !== gw.viewerName) out.push(w);
        else if (type === 110 /* FIND_FLAGS */ && o.type === 'flag') out.push(w);
        else if (type === 111 /* FIND_CONSTRUCTION_SITES */ && o.type === 'constructionSite') out.push(w);
        else if (type === 112 /* FIND_MY_SPAWNS */ && !neutral && o.type === 'spawn' && o.owner === gw.viewerName) out.push(w);
        else if (type === 113 /* FIND_HOSTILE_SPAWNS */ && !neutral && o.type === 'spawn' && typeof o.owner === 'string' && o.owner !== gw.viewerName) out.push(w);
        else if (type === 114 /* FIND_MY_CONSTRUCTION_SITES */ && !neutral && o.type === 'constructionSite' && o.owner === gw.viewerName) out.push(w);
        else if (type === 115 /* FIND_HOSTILE_CONSTRUCTION_SITES */ && !neutral && o.type === 'constructionSite' && typeof o.owner === 'string' && o.owner !== gw.viewerName) out.push(w);
        else if (type === 116 /* FIND_MINERALS */ && o.type === 'structure' && o.structureType === 'mineral') out.push(w);
        // B3 接线批：官方 game.js:345-360 装配墓碑/废墟/deposit 全族（119/122
        // 当前无对象恒空，臂在保持与 Rust matches_find 同构）
        else if (type === 118 /* FIND_TOMBSTONES */ && o.type === 'tombstone') out.push(w);
        else if (type === 119 /* FIND_POWER_CREEPS */ && o.type === 'powerCreep') out.push(w);
        else if (type === 122 /* FIND_DEPOSITS */ && o.type === 'deposit') out.push(w);
        else if (type === 123 /* FIND_RUINS */ && o.type === 'ruin') out.push(w);
      }
      // 官方 rooms.js:648-654：opts.filter（函数或 lodash 对象谓词）经
      // _.filter 应用——bot 生态最常用的 find 形态
      if (opts && opts.filter) return _.filter(out, opts.filter);
      return out;
    };
    // 地形 look 面（官方 rooms.js:659-679/798-830）：lookAt 恒含 terrain 项
    // （对象形态）；lookForAt(LOOK_TERRAIN) 是单词数组（官方 :681 `return [result]`）；
    // 其余 look 类型 M1 桩
    function unpackXY(firstArg, secondArg) {
      if (firstArg && typeof firstArg === 'object') return [firstArg.x, firstArg.y];
      return [firstArg, secondArg];
    }
    room.lookAt = function (x, y) {
      [x, y] = unpackXY(x, y);
      return [{ type: 'terrain', terrain: terrainWordAt(name, x, y) }];
    };
    room.lookForAt = function (type, x, y) {
      if (type === LOOK_TERRAIN) {
        [x, y] = unpackXY(x, y);
        return [terrainWordAt(name, x, y)];
      }
      countStub('room.lookForAt'); return [];
    };
    room.getTerrain = function () { return new Terrain(name); };
    room.createConstructionSite = function () { countStub('room.createConstructionSite'); return 0; };
    // 官方 rooms.js:573-583：raw=true 返回 JSON 字符串（持久化形态）、否则
    // 解析数组——我方投影 events 恒数组，raw 分支经预置 stringify（宿主数据
    // 编码不经可换全局——与 intents 出口同纪律）。B3 落账 events 上一 tick 覆写
    room.getEventLog = function (raw) {
      const ev = (r && r.events) || [];
      return raw ? __jsonStringify(ev) : ev;
    };
    room.createFlag = function () { countStub('room.createFlag'); return 'stub-flag'; };
    room.getPositionAt = (x, y) => new gw.RoomPosition(x, y, name);
    room.serializePath = () => '';
    room.deserializePath = () => [];
    __Object.defineProperty(room, 'memory', {
      configurable: true,
      get() {
        const m = gw.ensureMemory();
        if (!m.rooms) m.rooms = {};
        if (!m.rooms[name]) m.rooms[name] = {};
        return m.rooms[name];
      },
      // 官方 rooms.js:551 同时定义 setter——tooangel 等 bot 直接对 room.memory 赋值
      set(v) {
        const m = gw.ensureMemory();
        if (!m.rooms) m.rooms = {};
        m.rooms[name] = v;
      },
    });
    if (__RoomCtor && __RoomCtor.prototype) {
      __Object.setPrototypeOf(room, __RoomCtor.prototype);
    }
    return probe(room, 'Room[' + name + ']');
  }
  gw.makeRoom = makeRoom;
})();
