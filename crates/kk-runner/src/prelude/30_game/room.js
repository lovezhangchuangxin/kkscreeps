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
  globalThis.RoomVisual = RoomVisual;

  // ---------------------------------------------------------------- Room 装配
  // 对照官方 rooms.js Room 类实例面（find/lookAt/lookForAt/getTerrain/memory...）。

  function makeRoom(name, r) {
    // 无 controller 房间的官方投影是 undefined（非 null）
    let controller;
    for (const o of gw.world.objects || []) {
      if (o.room === name && o.type === 'structure' && o.structureType === 'controller') {
        controller = gw.objectsById.get(o.id);
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
      for (const o of gw.world.objects || []) {
        if (o.room !== name) continue;
        const w = gw.objectsById.get(o.id);
        if (!w) continue;
        const isStruct = o.type === 'structure' || o.type === 'spawn';
        // 官方 FIND_SOURCES_ACTIVE 条件 energy > 0（game.js:23）；初态缺省 3000
        // （与 wrapObject 的 source 投影缺省一致）
        if (type === 101 /* FIND_CREEPS */ && o.type === 'creep') out.push(w);
        else if (type === 102 /* FIND_MY_CREEPS */ && o.type === 'creep' && o.owner === gw.viewerName) out.push(w);
        else if (type === 104 /* FIND_SOURCES_ACTIVE */ && o.type === 'structure' && o.structureType === 'source' && (o.energy ?? 3000) > 0) out.push(w);
        else if (type === 105 /* FIND_SOURCES */ && o.type === 'structure' && o.structureType === 'source') out.push(w);
        else if (type === 106 /* FIND_DROPPED_RESOURCES */ && o.type === 'resource') out.push(w);
        // 官方 FIND_STRUCTURES/FIND_MY_STRUCTURES 不含 source/mineral：
        // game.js:229-251 structureTypes 表无此三类（独立装配分支 :329-345），
        // 与 Rust 侧 matches_find 两层一致
        else if (type === 107 /* FIND_STRUCTURES */ && isStruct && o.structureType !== 'source' && o.structureType !== 'mineral') out.push(w);
        else if (type === 108 /* FIND_MY_STRUCTURES */ && isStruct && o.structureType !== 'source' && o.structureType !== 'mineral' && o.owner === gw.viewerName) out.push(w);
        else if (type === 110 /* FIND_FLAGS */ && o.type === 'flag') out.push(w);
        else if (type === 111 /* FIND_CONSTRUCTION_SITES */ && o.type === 'constructionSite') out.push(w);
        else if (type === 112 /* FIND_MY_SPAWNS */ && o.type === 'spawn' && o.owner === gw.viewerName) out.push(w);
        else if (type === 116 /* FIND_MINERALS */ && o.type === 'structure' && o.structureType === 'mineral') out.push(w);
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
    room.createFlag = function () { countStub('room.createFlag'); return 'stub-flag'; };
    room.getPositionAt = (x, y) => new gw.RoomPosition(x, y, name);
    room.serializePath = () => '';
    room.deserializePath = () => [];
    Object.defineProperty(room, 'memory', {
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
    if (globalThis.Room && globalThis.Room.prototype) {
      Object.setPrototypeOf(room, globalThis.Room.prototype);
    }
    return probe(room, 'Room[' + name + ']');
  }
  gw.makeRoom = makeRoom;
})();
