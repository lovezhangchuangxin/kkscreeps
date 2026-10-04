/* 根据当前世界数据装配可见 Room，提供 find、look、地形、创建指令与绘图接口。
 * 每 tick 重建房间对象和查询缓存；地形数组由 game.js 的 beginTick 注入。
 * findPath 走 PathFinder 地形搜索（官方 _findPath2 形态，结构代价归回调文档）；绘图数据仅保留在本 tick 的缓冲区。
 * 来源：上游 engine/src/game/rooms.js 与 console.js 的玩家接口。
 */
(() => {
  'use strict';

  const { countStub, probe, lodash: _ } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 装配时使用捕获的 Object 和 Room 构造器；仍读取 Room.prototype，使玩家原型扩展可见。
  const __Object = Object;
  const __RoomCtor = globalThis.Room;
  const __mapGet = Map.prototype.get;
  const __reflectApply = Reflect.apply;
  const __jsonStringify = JSON.stringify;
  const __jsonParse = JSON.parse;
  // 地形拷出使用捕获的 TypedArray 构造器与 set：内部数组可能是锚定
  // 跨玩家共享内存的视图（KKPV 公开束），不得经玩家可替换的全局构造器
  // 或动态派发的 .set 外泄（共享直读内存的安全要求）。真 set 在 V8 内部做
  // TypedArray 品牌校验——伪 destination 在进入拷贝前即抛 TypeError，
  // 不执行任何玩家代码。
  const __Uint8Array = Uint8Array;
  const __u8Set = Uint8Array.prototype.set;
  // look 索引装配使用捕获的 push，避免玩家替换数组原型方法破坏下一 tick。
  const __arrayPush = Array.prototype.push;
  // 捕获未知 look 类型的错误码与地形位码，不使用玩家后来改写的全局常量。
  const ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const TERRAIN_MASK_WALL = globalThis.TERRAIN_MASK_WALL;
  const TERRAIN_MASK_SWAMP = globalThis.TERRAIN_MASK_SWAMP;
  // 创建与直线寻路方法使用加载时捕获的常量。
  const C_CONTROLLER_STRUCTURES = globalThis.CONTROLLER_STRUCTURES;
  const C_CONSTRUCTION_COST = globalThis.CONSTRUCTION_COST;
  const C_MAX_CONSTRUCTION_SITES = globalThis.MAX_CONSTRUCTION_SITES;
  const C_FLAGS_LIMIT = globalThis.FLAGS_LIMIT;
  const C_COLORS_ALL = globalThis.COLORS_ALL;
  const C_COLOR_WHITE = globalThis.COLOR_WHITE;
  const C_ERR_NOT_OWNER = globalThis.ERR_NOT_OWNER;
  const C_ERR_RCL_NOT_ENOUGH = globalThis.ERR_RCL_NOT_ENOUGH;
  const C_ERR_INVALID_TARGET = globalThis.ERR_INVALID_TARGET;
  const C_ERR_FULL = globalThis.ERR_FULL;
  const C_ERR_NAME_EXISTS = globalThis.ERR_NAME_EXISTS;
  const C_OK = globalThis.OK;
  const C_TOP = globalThis.TOP;
  const C_TOP_LEFT = globalThis.TOP_LEFT;
  const C_TOP_RIGHT = globalThis.TOP_RIGHT;
  const C_BOTTOM = globalThis.BOTTOM;
  const C_BOTTOM_LEFT = globalThis.BOTTOM_LEFT;
  const C_BOTTOM_RIGHT = globalThis.BOTTOM_RIGHT;
  const C_LEFT = globalThis.LEFT;
  const C_RIGHT = globalThis.RIGHT;

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
        // 经捕获的真 set 拷贝：destination 须是真 TypedArray（V8 品牌
        // 校验，伪造对象在此抛错），拷贝是引擎内部 memmove——两条路径
        // 都不把内部数组（可能是共享视图）交给玩家代码。
        __reflectApply(__u8Set, destinationArray, [array]);
        return destinationArray;
      }
      return new __Uint8Array(array);
    };
  }
  // 官方 Terrain 挂法：Room 的静态属性（rooms.js:1135 `new Room.Terrain(...)`），
  // 非裸全局
  globalThis.Room.Terrain = Terrain;
  gw.Terrain = Terrain;

  // 官方 utils.js:555-575 serializePath 逐字（Room 静态形态，rooms.js:528-530）：
  // 非数组抛 'path is not an array'、空数组 ''、首元素负坐标抛 'path
  // coordinates cannot be negative'、两位零填充起点 + 逐元素 direction 拼接。
  // deserializePath 静态在 creep.js 挂（实现段在那，加载序 creep 后于 room）
  function serializePath(path) {
    if (!_.isArray(path)) {
      throw new Error('path is not an array');
    }
    let result = '';
    if (!path.length) {
      return result;
    }
    if (path[0].x < 0 || path[0].y < 0) {
      throw new Error('path coordinates cannot be negative');
    }
    result += path[0].x > 9 ? path[0].x : '0' + path[0].x;
    result += path[0].y > 9 ? path[0].y : '0' + path[0].y;
    for (let i = 0; i < path.length; i++) {
      result += path[i].direction;
    }
    return result;
  }
  globalThis.Room.serializePath = serializePath;

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

  // 绘图字符串按房间名共享：同房间多个 RoomVisual 实例操作同一缓冲区。
  // 房间限制 500×1024、map 限制 1000×1024 个 UTF-16 码元，超限抛错；
  // beginTick 清空全部缓冲区。来源：上游 engine/src/game/console.js 的 addVisual。
  const visualStore = __Object.create(null);
  gw.addVisual = function addVisual(roomName, data) {
    roomName = roomName || '';
    if (!data) {
      return;
    }
    const sizeLimit = roomName === 'map' ? 1000 : 500;
    const dataString = typeof data === 'string' ? data : __jsonStringify(data) + '\n';
    if ((visualStore[roomName] || '').length + dataString.length > sizeLimit * 1024) {
      if (roomName === 'map') {
        throw new Error('MapVisual size has exceeded ' + sizeLimit + ' KB limit');
      }
      throw new Error('RoomVisual size in room ' + roomName + ' has exceeded ' + sizeLimit + ' KB limit');
    }
    visualStore[roomName] = (visualStore[roomName] || '') + dataString;
  };
  gw.getVisualSize = function getVisualSize(roomName) {
    roomName = roomName || '';
    return visualStore[roomName] ? visualStore[roomName].length : 0;
  };
  gw.clearVisual = function clearVisual(roomName) {
    visualStore[roomName || ''] = '';
  };
  gw.getVisual = function getVisual(roomName) {
    return visualStore[roomName || ''];
  };
  gw.visualReset = function visualReset() {
    for (const k in visualStore) delete visualStore[k];
  };

  // 官方 RoomVisual（rooms.js:1144-1218）逐字：六绘制方法（坐标/位置双形）+
  // getSize/clear/export/import，全部经 console.addVisual 族路由（我方 gw 桥）。
  class RoomVisual {
    constructor(roomName) { this.roomName = roomName; }
    circle(x, y, style) {
      if (typeof x === 'object') {
        style = y; y = x.y; x = x.x;
      }
      gw.addVisual(this.roomName, { t: 'c', x, y, s: style });
      return this;
    }
    line(x1, y1, x2, y2, style) {
      if (typeof x1 === 'object' && typeof y1 === 'object') {
        style = x2;
        x2 = y1.x; y2 = y1.y;
        y1 = x1.y; x1 = x1.x;
      }
      gw.addVisual(this.roomName, { t: 'l', x1, y1, x2, y2, s: style });
      return this;
    }
    rect(x, y, w, h, style) {
      if (typeof x === 'object') {
        style = h; h = w; w = y;
        y = x.y; x = x.x;
      }
      gw.addVisual(this.roomName, { t: 'r', x, y, w, h, s: style });
      return this;
    }
    poly(points, style) {
      if (Array.isArray(points) && points.some(Boolean)) {
        points = points.map((i) => (i.x !== undefined ? [i.x, i.y] : i));
        gw.addVisual(this.roomName, { t: 'p', points, s: style });
      }
      return this;
    }
    text(text, x, y, style) {
      if (typeof x === 'object') {
        style = y; y = x.y; x = x.x;
      }
      gw.addVisual(this.roomName, { t: 't', text, x, y, s: style });
      return this;
    }
    getSize() { return gw.getVisualSize(this.roomName); }
    clear() { gw.clearVisual(this.roomName); return this; }
    export() { return gw.getVisual(this.roomName); }
    import(data) { gw.addVisual(this.roomName, '' + data); return this; }
  }
  // RoomVisual 全局绑定不可写。来源：上游 engine/src/game/rooms.js 的 RoomVisual 导出。
  Object.defineProperty(globalThis, 'RoomVisual', {
    enumerable: true, value: RoomVisual,
  });

  // ---------------------------------------------------------------- Room 装配
  // 对照官方 rooms.js Room 类实例面（find/lookAt/lookForAt/getTerrain/memory...）。

  function makeRoom(name, r) {
    // 无 controller 房间的官方投影是 undefined（非 null）
    let controller;
    const worldObjs = gw.world.objects || [];
    // 一次扫描本房对象建立类型索引与 x*50+y 的空间索引，同时寻找 controller。
    // energy/resource 共用索引，lookAt 同格掉落资源会输出两项。
    // 来源：上游 engine/src/game/rooms.js 的 lookTypeRegisters/lookTypeSpatialRegisters。
    const lookReg = {
      creep: {}, source: {}, mineral: {}, deposit: {}, structure: {}, flag: {},
      constructionSite: {}, tombstone: {}, ruin: {}, nuke: {}, powerCreep: {},
    };
    lookReg.energy = {};
    lookReg.resource = lookReg.energy;
    const lookSpatial = {
      creep: [], source: [], mineral: [], deposit: [], structure: [], flag: [],
      constructionSite: [], tombstone: [], ruin: [], nuke: [], powerCreep: [],
    };
    // 保留本房原始对象供工地名额与占位检查；storage/terminal 另记录其玩家视图引用。
    const roomObjs = [];
    let storageObj, terminalObj;
    lookSpatial.energy = [];
    lookSpatial.resource = lookSpatial.energy;
    // Mod look 桶按玩家面目录预初始化（与内建 13 桶同等待遇）：空房
    // lookForAt 返回 [] 而不是 ERR_INVALID_ARGS；类型键来自注册目录
    //（恒含冒号），不与内建键或 Object.prototype 原型键冲突。
    const modLookTypes = [];
    if (gw.modApi !== undefined) {
      for (const typeKey in gw.modApi.types) {
        lookReg[typeKey] = {};
        lookSpatial[typeKey] = [];
        modLookTypes.push(typeKey);
      }
      modLookTypes.sort();
    }
    for (let i = 0; i < worldObjs.length; i++) {
      const o = worldObjs[i];
      if (o.room !== name) continue;
      roomObjs.push(o);
      if (o.type === 'structure' && o.structureType === 'controller') {
        controller = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      }
      if (o.type === 'structure' && o.structureType === 'storage') {
        storageObj = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      }
      if (o.type === 'structure' && o.structureType === 'terminal') {
        terminalObj = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      }
      // look 类型归类（官方装配分支同构：source/mineral 独立臂、spawn/controller
      // 属 structure、dropped resource 落 energy 臂）
      let t = null;
      if (o.type === 'creep') t = 'creep';
      else if (o.type === 'resource') t = 'energy';
      else if (o.type === 'structure') {
        if (o.structureType === 'source') t = 'source';
        else if (o.structureType === 'mineral') t = 'mineral';
        else t = 'structure';
      } else if (o.type === 'spawn') t = 'structure';
      else if (o.type === 'flag') t = 'flag';
      else if (o.type === 'constructionSite') t = 'constructionSite';
      else if (o.type === 'tombstone') t = 'tombstone';
      else if (o.type === 'ruin') t = 'ruin';
      else if (o.type === 'nuke') t = 'nuke';
      else if (o.type === 'powerCreep') t = 'powerCreep';
      else if (o.type === 'deposit') t = 'deposit';
      // Mod 对象按类型键进 look 桶（键带冒号，不与内建 look 键冲突；
      // 桶已在下方按玩家面目录预初始化）。未安装玩家面的分片不会有
      // mod 记录，此分支为死代码。
      else if (o.type === 'mod') t = o.modType;
      if (t === null) continue;
      const w = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      if (!w) continue;
      lookReg[t][o.id] = w;
      const index = o.x * 50 + o.y;
      if (lookSpatial[t][index] === undefined) {
        lookSpatial[t][index] = [w];
      } else {
        __reflectApply(__arrayPush, lookSpatial[t][index], [w]);
      }
    }
    const room = {
      name,
      controller,
      // 官方 game.js:325-328：非 off 的 spawn/extension 计入（spawn 300 固定、
      // extension 按 RCL 表）——由 Rust 侧 world_json 房间投影携带
      energyAvailable: r.energyAvailable ?? 0,
      energyCapacityAvailable: r.energyCapacityAvailable ?? 300,
      // 官方 game.js:330+：storage/terminal 为结构实例（无则 undefined）
      storage: storageObj,
      terminal: terminalObj,
      visual: new RoomVisual(name),
      mode: 'world',
    };
    // 方向出口缓存未过滤结果；FIND_EXIT 首次合并的缓存已经过当次 filter。
    // 再次调用的 filter 作用于该缓存，不能恢复首次已过滤掉的出口。
    // 返回新数组但复用元素引用；对象类查询每次扫描本房对象。
    // 来源：上游 engine/src/game/rooms.js 的 Room.find。
    const findCache = {};
    room.find = function (type, opts) {
      countStub('room.find:' + type);
      // 官方 rooms.js:592-601：FIND_EXIT = 四向臂 concat（官方序
      // TOP/BOTTOM/RIGHT/LEFT），子臂各自应用 opts（含 filter）；缓存后
      // 提前 return _.clone（不走通用尾滤——陈旧 filter 语义在此形成）
      if (type === 10 /* FIND_EXIT */) {
        if (findCache[10] !== undefined) {
          // 官方命中汇入通用尾（rooms.js:587-588→:648-655）：当次 filter
          // 作用于烘焙缓存（交集），无 filter 走 _.clone 新数组
          if (opts && opts.filter) return _.filter(findCache[10], opts.filter);
          return _.clone(findCache[10]);
        }
        const r = this.find(1, opts).concat(this.find(5, opts))
          .concat(this.find(3, opts)).concat(this.find(7, opts));
        findCache[10] = r;
        return _.clone(r);
      }
      // 官方 rooms.js:602-641：四向臂由地形 wall 位计算——沿边 50 格，
      // 非 wall 者经 getPositionAt 产出（top: (key,0)、bottom: (key,49)、
      // left: (0,key)、right: (49,key)）。terrain flat 数组行主序 y*50+x
      // （非转置）；未滤结果入缓存，命中走通用尾滤
      if (type === 1 || type === 3 || type === 5 || type === 7) {
        if (findCache[type] !== undefined) {
          if (opts && opts.filter) return _.filter(findCache[type], opts.filter);
          return _.clone(findCache[type]);
        }
        const terrain = gw.terrainByRoom[name];
        const exits = [];
        for (let i = 0; i < 50; i++) {
          let x = 0, y = 0;
          if (type === 7 || type === 3) {
            y = i;
          } else {
            x = i;
          }
          if (type === 3) {
            x = 49;
          }
          if (type === 5) {
            y = 49;
          }
          exits.push(!(terrain[y * 50 + x] & TERRAIN_MASK_WALL));
        }
        const out = [];
        for (let key = 0; key < exits.length; key++) {
          if (exits[key]) {
            if (type === 1) out.push(this.getPositionAt(key, 0));
            if (type === 5) out.push(this.getPositionAt(key, 49));
            if (type === 7) out.push(this.getPositionAt(0, key));
            if (type === 3) out.push(this.getPositionAt(49, key));
          }
        }
        findCache[type] = out;
        if (opts && opts.filter) return _.filter(out, opts.filter);
        return _.clone(out);
      }
      const out = [];
      // Mod 查询常量：FIND 下界与常量→类型映射都来自玩家面目录（Rust
      // 清单权威）；下界判定与映射在循环外完成。
      const modFindType = gw.modApi === undefined || type < gw.modApi.modFindBase
        ? undefined
        : gw.modApi.findByConst[type];
      // 没有查看者时 MY_*/HOSTILE_* 无法分类，返回空；FIND_CREEPS 在此模式保留孵化对象。
      const neutral = gw.viewerName === null;
      for (let i = 0; i < worldObjs.length; i++) {
        const o = worldObjs[i];
        if (o.room !== name) continue;
        const w = __reflectApply(__mapGet, gw.objectsById, [o.id]);
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
        // 这些查询分支从当前世界对象中收集墓碑、powerCreep、deposit 与废墟；无对象时返回空。
        else if (type === 118 /* FIND_TOMBSTONES */ && o.type === 'tombstone') out.push(w);
        else if (type === 119 /* FIND_POWER_CREEPS */ && o.type === 'powerCreep') out.push(w);
        else if (type === 122 /* FIND_DEPOSITS */ && o.type === 'deposit') out.push(w);
        else if (type === 123 /* FIND_RUINS */ && o.type === 'ruin') out.push(w);
        // Mod 查询常量（注册期分配，下界见 modFindBase）：命中对应类型。
        else if (modFindType !== undefined && o.type === 'mod' && o.modType === modFindType) {
          out.push(w);
        }
      }
      // 官方 rooms.js:648-654：opts.filter（函数或 lodash 对象谓词）经
      // _.filter 应用——bot 生态最常用的 find 形态
      if (opts && opts.filter) return _.filter(out, opts.filter);
      return out;
    };
    // 官方 rooms.js:659-707 _lookSpatialRegister 逐字：terrain 分支读静态地形
    //（越界索引落 flat 数组外 → undefined 掩码得 'plain'，官方怪癖保留；无地形
    // 房间自然 TypeError 同构）；非 terrain 越界抛官方文案；outArray 形态产出
    // {type,[type]:obj}（lookAtArea asArray 加 x/y），否则 [单词]/_.clone(数组)/[]
    function _lookSpatialRegister(typeName, x, y, outArray, withCoords) {
      let item;
      if (typeName == 'terrain') {
        const word = terrainWordAt(name, x, y);
        if (outArray) {
          item = { type: 'terrain', terrain: word };
          if (withCoords) {
            item.x = x;
            item.y = y;
          }
          outArray.push(item);
          return;
        }
        return [word];
      }
      if (x < 0 || y < 0 || x > 49 || y > 49) {
        throw new Error('look coords are out of bounds');
      }
      const typeResult = lookSpatial[typeName][x * 50 + y];
      if (typeResult) {
        if (outArray) {
          typeResult.forEach((i) => {
            item = { type: typeName };
            item[typeName] = i;
            if (withCoords) {
              item.x = x;
              item.y = y;
            }
            outArray.push(item);
          });
          return;
        }
        return _.clone(typeResult);
      }
      return [];
    }

    // 官方 rooms.js:709-764 _lookAreaMixedRegister 逐字：寄存器键数 < 面积 →
    // 按对象（checkInside 官方运算优先级原样）；否则逐格 spatial（asArray 带
    // withCoords）。未知 type 在键数比较处 keys.length 自然 TypeError——官方
    // lookForAtArea 无 ERR_INVALID_ARGS 门（与 lookForAt 不对称，原样保留）
    function _lookAreaMixedRegister(type, top, left, bottom, right, withType, asArray, result) {
      const typeRegister = lookReg[type];
      const keys = typeRegister && Object.keys(typeRegister);
      if (type != 'terrain' && keys.length < (bottom - top + 1) * (right - left + 1)) {
        const checkInside = (i) => {
          return (!i.pos && i.roomName == name || i.pos && i.pos.roomName == name)
            && i.pos && i.pos.y >= top && i.pos.y <= bottom && i.pos.x >= left && i.pos.x <= right
            || !i.pos && i.y >= top && i.y <= bottom && i.x >= left && i.x <= right;
        };
        let item;
        keys.forEach((key) => {
          const obj = typeRegister[key];
          if (checkInside(obj)) {
            if (withType) {
              item = { type: type };
              item[type] = obj;
              if (asArray) {
                result.push({ x: obj.x || obj.pos.x, y: obj.y || obj.pos.y, type: type, [type]: obj });
              } else {
                result[obj.y || obj.pos.y][obj.x || obj.pos.x].push(item);
              }
            } else {
              if (asArray) {
                result.push({ x: obj.x || obj.pos.x, y: obj.y || obj.pos.y, [type]: obj });
              } else {
                result[obj.y || obj.pos.y][obj.x || obj.pos.x] = result[obj.y || obj.pos.y][obj.x || obj.pos.x] || [];
                result[obj.y || obj.pos.y][obj.x || obj.pos.x].push(obj);
              }
            }
          }
        });
      } else {
        for (let y = top; y <= bottom; y++) {
          for (let x = left; x <= right; x++) {
            if (asArray) {
              _lookSpatialRegister(type, x, y, result, true);
            } else {
              if (result[y][x]) {
                _lookSpatialRegister(type, x, y, result[y][x]);
              } else {
                result[y][x] = _lookSpatialRegister(type, x, y, undefined);
              }
            }
          }
        }
      }
    }

    // 官方 rooms.js:768-796 lookAt 逐字：官方固定序 14 类型（creep→energy→
    // resource→source→mineral→deposit→structure→flag→constructionSite→
    // terrain 第 10 位→nuke→tombstone→ruin→powerCreep）；Mod 类型键（带
    // 冒号）按键序追加在内建固定序之后，官方 customObjectPrototypes 协议
    // 不接入
    room.lookAt = function (firstArg, secondArg) {
      const xy = gw.fetchXYArguments(firstArg, secondArg);
      const x = xy[0], y = xy[1];
      const result = [];
      _lookSpatialRegister('creep', x, y, result);
      _lookSpatialRegister('energy', x, y, result);
      _lookSpatialRegister('resource', x, y, result);
      _lookSpatialRegister('source', x, y, result);
      _lookSpatialRegister('mineral', x, y, result);
      _lookSpatialRegister('deposit', x, y, result);
      _lookSpatialRegister('structure', x, y, result);
      _lookSpatialRegister('flag', x, y, result);
      _lookSpatialRegister('constructionSite', x, y, result);
      _lookSpatialRegister('terrain', x, y, result);
      _lookSpatialRegister('nuke', x, y, result);
      _lookSpatialRegister('tombstone', x, y, result);
      _lookSpatialRegister('ruin', x, y, result);
      _lookSpatialRegister('powerCreep', x, y, result);
      for (let m = 0; m < modLookTypes.length; m++) {
        _lookSpatialRegister(modLookTypes[m], x, y, result);
      }
      return result;
    };
    // 官方 rooms.js:798-806 lookForAt 逐字：未知类型（非 terrain 且不在
    // spatial 寄存器键集）返 ERR_INVALID_ARGS；terrain 走 [单词]
    room.lookForAt = function (type, firstArg, secondArg) {
      const xy = gw.fetchXYArguments(firstArg, secondArg);
      const x = xy[0], y = xy[1];
      if (type != 'terrain' && !(type in lookSpatial)) {
        return ERR_INVALID_ARGS;
      }
      return _lookSpatialRegister(type, x, y);
    };
    // 官方 rooms.js:808-845 lookAtArea 逐字：非 asArray 预填稠密格阵 []，
    // asArray 得 [{x,y,type,[type]:obj}]（terrain 项带坐标）
    room.lookAtArea = function (top, left, bottom, right, asArray) {
      const result = asArray ? [] : {};
      if (!asArray) {
        for (let y = top; y <= bottom; y++) {
          result[y] = {};
          for (let x = left; x <= right; x++) {
            result[y][x] = [];
          }
        }
      }
      _lookAreaMixedRegister('creep', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('energy', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('resource', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('source', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('mineral', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('deposit', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('structure', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('flag', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('constructionSite', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('terrain', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('nuke', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('tombstone', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('ruin', top, left, bottom, right, true, asArray, result);
      _lookAreaMixedRegister('powerCreep', top, left, bottom, right, true, asArray, result);
      for (let m = 0; m < modLookTypes.length; m++) {
        _lookAreaMixedRegister(modLookTypes[m], top, left, bottom, right, true, asArray, result);
      }
      return result;
    };
    // 官方 rooms.js:847-860 lookForAtArea 逐字：非 asArray 只预填行对象（格
    // 惰性建——sparse map 原始对象无 type 包裹），asArray 得 [{x,y,[type]:obj}]
    room.lookForAtArea = function (type, top, left, bottom, right, asArray) {
      const result = asArray ? [] : {};
      if (!asArray) {
        for (let y = top; y <= bottom; y++) {
          result[y] = {};
        }
      }
      _lookAreaMixedRegister(type, top, left, bottom, right, false, asArray, result);
      return result;
    };
    room.getTerrain = function () { return new Terrain(name); };
    // 创建预检使用 raw structureType 区分结构类型，type 区分结构、spawn 与工地。
    /** 在自家所有 Game.spawns 和 Game.constructionSites 中检查 spawn 重名。
     * 跨房工地也参与检查；与上游 register.spawns 的差异是此处不检查他人 spawn 名。 */
    function spawnNameTaken(name) {
      return Object.values(gw.currentGame().spawns).some((s) => s.name === name)
        || Object.values(gw.currentGame().constructionSites).some(
          (o) => o.structureType === 'spawn' && o.name === name,
        );
    }
    /** 官方 utils.js:338-354 checkControllerAvailability：现存同类型（含
     *  工地）< CONTROLLER_STRUCTURES[type][rcl]（rcl：有主 controller 的
     *  level，无主/无 controller 为 0——官方 (user||owner) 真值门的等价物）。
     *  表缺类型/配额 undefined → NaN 比较恒 false，官方同形。 */
    function checkControllerAvailability(structureType, room) {
      const controller = room.controller;
      let rcl = 0;
      if (controller && controller.level && controller.owner) rcl = controller.level;
      let cnt = 0;
      for (const o of roomObjs) {
        if (o.structureType === structureType
          && (o.type === 'structure' || o.type === 'spawn' || o.type === 'constructionSite')) {
          cnt++;
        }
      }
      const quota = C_CONTROLLER_STRUCTURES[structureType];
      return cnt < (quota ? quota[rcl] : undefined);
    }
    /** 检查同格同类型或工地占位；extractor 需矿物，随后提前返回，不使用其他造价对象互斥检查。
     * 其他非 road/rampart 结构与造价对象互斥，房间边界拒绝建造。
     * 来源：上游 engine/src/utils.js 的 checkConstructionSite 对象分支。 */
    function checkConstructionSiteObjects(structureType, x, y) {
      const typeOf = (o) => ((o.type === 'structure' || o.type === 'spawn') && o.structureType)
        ? o.structureType : o.type;
      let hasMineral = false;
      let hasExtractor = false;
      const isExtractor = structureType === 'extractor';
      for (const o of roomObjs) {
        if (o.x !== x || o.y !== y) continue;
        const t = typeOf(o);
        if (t === structureType) return false;
        if (o.type === 'constructionSite') return false;
        if (t === 'mineral') hasMineral = true;
        if (t === 'extractor') hasExtractor = true;
        if (isExtractor) continue;
        if (structureType !== 'rampart' && structureType !== 'road'
          && t !== 'rampart' && t !== 'road' && C_CONSTRUCTION_COST[t]) {
          return false;
        }
      }
      if (isExtractor) return hasMineral && !hasExtractor;
      if (x <= 0 || y <= 0 || x >= 49 || y >= 49) return false;
      return true;
    }
    /** 非 road/container 在 1/48 边缘要求所选边界三格都是墙；extractor 提前通过，其他非 road 不可建墙上。
     * 来源：上游 engine/src/utils.js 的 checkConstructionSite 地形分支。 */
    function checkConstructionSiteTerrain(structureType, x, y) {
      const terrain = gw.terrainByRoom[name];
      if (!terrain) return false;
      const isWall = (tx, ty) => (terrain[ty * 50 + tx] & TERRAIN_MASK_WALL) !== 0;
      if (structureType !== 'road' && structureType !== 'container'
        && (x === 1 || x === 48 || y === 1 || y === 48)) {
        // 先检查 x 边，再检查 y 边；角落位置取最后命中的 y 边三格。
        // 来源：上游 engine/src/utils.js 的 checkConstructionSite 地形检查。
        let border;
        if (x === 1) border = [[0, y - 1], [0, y], [0, y + 1]];
        if (x === 48) border = [[49, y - 1], [49, y], [49, y + 1]];
        if (y === 1) border = [[x - 1, 0], [x, 0], [x + 1, 0]];
        if (y === 48) border = [[x - 1, 49], [x, 49], [x + 1, 49]];
        for (let i = 0; i < border.length; i++) {
          if (!isWall(border[i][0], border[i][1])) return false;
        }
      }
      if (structureType === 'extractor') return true;
      if (structureType !== 'road' && isWall(x, y)) return false;
      return true;
    }
    /** 官方 rooms.js:540-549 toJSON：for-in 枚举（含原型可枚举法——stringify
     *  时函数值自然丢弃）+ 跳过 _ 前缀与五键。 */
    room.toJSON = function () {
      const result = {};
      for (const i in this) {
        if (i[0] === '_' || i === 'toJSON' || i === 'toString'
          || i === 'controller' || i === 'storage' || i === 'terminal') {
          continue;
        }
        result[i] = this[i];
      }
      return result;
    };
    /** 官方 Room.findPath（rooms.js:863 起）：异房起点先出（serialize 访问
     * 原始 opts——不传 opts 且异房起点按官方抛 TypeError）；默认
     * _useNewPathFinder=true（game.js:68）走 _findPath2（rooms.js:223-305）：
     * PathFinder 地形搜索（range 缺省 1、跨房 maxOps 缺省 20000、非
     * ignoreRoads 时 plainCost 2/swampCost 10——道路本身须经代价矩阵生效）
     * 后按房界截断；无 range 且终点贴近时回填终点。opts.maxOps 的缺省写入
     * 原地修改调用方对象（官方同形）。costCallback/结构代价矩阵尚未接线
     * （docs/designs/pathfinder-terrain.md），当前为地形直通。 */
    room.findPath = function (fromPos, toPos, opts) {
      if (fromPos.roomName !== name) {
        return opts.serialize ? '' : [];
      }
      opts = opts || {};
      // _findPath2:227-229
      if (fromPos.isEqualTo(toPos)) {
        return opts.serialize ? '' : [];
      }
      // :239-241 跨房且未限时缺省 maxOps 20000（注意 maxRooms>1 条件）
      if (opts.maxOps === undefined && (opts.maxRooms === undefined || opts.maxRooms > 1)
        && fromPos.roomName !== toPos.roomName) {
        opts.maxOps = 20000;
      }
      const searchOpts = {
        maxOps: opts.maxOps,
        maxRooms: opts.maxRooms,
      };
      // :253-261 非 ignoreRoads 提高平地/沼泽代价（官方为道路绕行的代价设计）
      if (!opts.ignoreRoads) {
        searchOpts.plainCost = 2;
        searchOpts.swampCost = 10;
      }
      if (opts.plainCost) {
        searchOpts.plainCost = opts.plainCost;
      }
      if (opts.swampCost) {
        searchOpts.swampCost = opts.swampCost;
      }
      const ret = PathFinder.search(
        fromPos,
        { range: Math.max(1, opts.range || 0), pos: toPos },
        searchOpts,
      );
      // :266-271 无 range 时的终点贴近回填（与官方同含"零路径且起点贴近"臂）
      if (!opts.range && (
        ret.path.length && ret.path[ret.path.length - 1].isNearTo(toPos)
          && !ret.path[ret.path.length - 1].isEqualTo(toPos)
        || !ret.path.length && fromPos.isNearTo(toPos)
      )) {
        ret.path.push(toPos);
      }
      // :273-292 房界截断 + 步进 {x,y,dx,dy,direction}
      let curX = fromPos.x, curY = fromPos.y;
      const resultPath = [];
      for (let i = 0; i < ret.path.length; i++) {
        const pos = ret.path[i];
        if (pos.roomName !== name) {
          break;
        }
        const step = {
          x: pos.x,
          y: pos.y,
          dx: pos.x - curX,
          dy: pos.y - curY,
          direction: gw.getDirection(pos.x - curX, pos.y - curY),
        };
        curX = step.x;
        curY = step.y;
        resultPath.push(step);
      }
      return opts.serialize ? serializePath(resultPath) : resultPath;
    };
    /** 按坐标、类型、spawn 名、控制器权限、名额、占位与总工地数的顺序检查，然后记录创建指令。
     * 本 tick 创建计数与世界快照中的已有工地数分别统计。
     * 来源：上游 engine/src/game/rooms.js 的 createConstructionSite。 */
    room.createConstructionSite = function (firstArg, secondArg, structureType, name) {
      const xy = gw.fetchXYArguments(firstArg, secondArg);
      const x = xy[0], y = xy[1];
      if (x === undefined || y === undefined || x < 0 || x > 49 || y < 0 || y > 49) {
        return ERR_INVALID_ARGS;
      }
      if (typeof secondArg === 'string' && structureType === undefined) {
        structureType = secondArg;
      }
      if (!C_CONSTRUCTION_COST[structureType]) return ERR_INVALID_ARGS;
      if (structureType === 'spawn' && typeof name === 'string') {
        if (name.length > 100) return ERR_INVALID_ARGS;
        if (gw.createdSpawnNames.has(name)) return ERR_INVALID_ARGS;
        if (spawnNameTaken(name)) {
          return ERR_INVALID_ARGS;
        }
      }
      if (this.controller && this.controller.level > 0 && !this.controller.my) {
        return C_ERR_NOT_OWNER;
      }
      if (this.controller && this.controller.reservation
        && this.controller.reservation.username !== gw.viewerName) {
        return C_ERR_NOT_OWNER;
      }
      if (!checkControllerAvailability(structureType, this)) {
        return C_ERR_RCL_NOT_ENOUGH;
      }
      if (!checkConstructionSiteObjects(structureType, x, y)
        || !checkConstructionSiteTerrain(structureType, x, y)) {
        return C_ERR_INVALID_TARGET;
      }
      // 官方 rooms.js:1071-1073：自家工地总数 + 本 tick 已建数 < 帽（官方
      // 分开数是快照不随建增长；我方投影即本 tick 状态，两者合流同值）
      const ownSiteCount = Object.keys(gw.currentGame().constructionSites).length;
      if (ownSiteCount + gw.createdConstructionSites >= C_MAX_CONSTRUCTION_SITES) {
        return C_ERR_FULL;
      }
      const intent = { x, y, structureType };
      if (structureType === 'spawn') {
        if (typeof name !== 'string') {
          let cnt = 1;
          do {
            name = 'Spawn' + cnt;
            cnt++;
          } while (spawnNameTaken(name) || gw.createdSpawnNames.has(name));
        }
        gw.createdSpawnNames.add(name);
        intent.name = name;
      }
      // 官方 :1075 `const roomName = ""+this.name`——闭包参数 name 已被
      // spawn 名遮蔽，经 this.name 取房名
      intent.roomName = this.name;
      gw.createdConstructionSites++;
      gw.intents.pushByName('room', 'createConstructionSite', intent);
      return C_OK;
    };
    // 装配时分别保存事件 JSON 字符串和解析数组；修改解析数组不会反向改变 raw 返回值。
    // 同 tick 的非 raw 调用复用解析数组。来源：上游 engine/src/game/rooms.js 的 getEventLog。
    const eventLogRaw = __jsonStringify((r && r.events) || []);
    const eventLogParsed = __jsonParse(eventLogRaw);
    room.getEventLog = function (raw) {
      return raw ? eventLogRaw : eventLogParsed;
    };
    /** 官方 createFlag（rooms.js:978-1027，P2 转真）：fetchXY/边界 →
     *  FLAGS_LIMIT → 颜色白名单（缺省 COLOR_WHITE/secondary=color）→ 名自增
     *  'Flag1..' → 重名（register.flags + 本 tick 已建）→ 长度 ≤100 →
     *  本地 Game.flags 装配 + intent → 返回名。 */
    room.createFlag = function (firstArg, secondArg, name, color, secondaryColor) {
      const xy = gw.fetchXYArguments(firstArg, secondArg);
      let x = xy[0], y = xy[1];
      if (x === undefined || y === undefined || x < 0 || x > 49 || y < 0 || y > 49) {
        return ERR_INVALID_ARGS;
      }
      if (Object.keys(gw.currentGame().flags).length >= C_FLAGS_LIMIT) {
        return C_ERR_FULL;
      }
      if (firstArg !== null && typeof firstArg === 'object') {
        secondaryColor = color;
        color = name;
        name = secondArg;
      }
      if (!color) color = C_COLOR_WHITE;
      if (!secondaryColor) secondaryColor = color;
      if (!C_COLORS_ALL.includes(color)) return ERR_INVALID_ARGS;
      if (!C_COLORS_ALL.includes(secondaryColor)) return ERR_INVALID_ARGS;
      if (!name) {
        let cnt = 1;
        do {
          name = 'Flag' + cnt;
          cnt++;
        } while (gw.currentGame().flags[name] !== undefined || gw.createdFlagNames.has(name));
      }
      if (gw.currentGame().flags[name] !== undefined || gw.createdFlagNames.has(name)) {
        return C_ERR_NAME_EXISTS;
      }
      if (name.length > 100) return ERR_INVALID_ARGS;
      gw.createdFlagNames.add(name);
      // 新旗立即加入本 tick 的 Game.flags，但不加入 objectsById；下一 tick 由世界数据重新装配。
      // 旗名遮蔽局部 name，因此房间名要从 this.name 读取。
      const roomName = this.name;
      const shell = gw.wrapObject(
        { type: 'flag', id: undefined, room: roomName, x, y, name, color, secondaryColor },
        gw.world.time,
      );
      // 新旗帜不经过 buildGame 的回填循环，必须在这里设置其 room 引用。
      shell.room = this;
      gw.currentGame().flags[name] = shell;
      gw.intents.pushByName('room', 'createFlag', {
        roomName, x, y, name, color, secondaryColor,
      });
      return name;
    };
    // 官方 rooms.js:1128-1131：map.findExit 转发（无 opts 参——两参形态）
    room.findExitTo = function (target) {
      return gw.currentGame().map.findExit(name, target);
    };
    // 官方 rooms.js:971-976：越界坐标返 null（构造器抛错前拦截——不依赖
    // 'Invalid arguments' 通道）
    room.getPositionAt = (x, y) => {
      if (x < 0 || x > 49 || y < 0 || y > 49) return null;
      return new gw.RoomPosition(x, y, name);
    };
    // 官方 rooms.js:551-571：memory 为 getter/setter（gw.memoryFor 官方同形——
    // 含 'undefined' 病态检查与非对象分支），tooangel 等 bot 直接对 room.memory 赋值
    __Object.defineProperty(room, 'memory', {
      configurable: true,
      get() {
        return gw.memoryFor('rooms', name);
      },
      set(v) {
        gw.setMemoryFor('rooms', name, v, 'room');
      },
    });
    if (__RoomCtor && __RoomCtor.prototype) {
      __Object.setPrototypeOf(room, __RoomCtor.prototype);
    }
    return probe(room, 'Room[' + name + ']');
  }
  gw.makeRoom = makeRoom;
})();
