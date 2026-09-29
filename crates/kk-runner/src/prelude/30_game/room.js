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

  const { countStub, probe, lodash: _ } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 预置捕获（A3c 装配批，同 game.js 原则）：makeRoom 每 tick 装配，Object
  // 交换会使原型链接/memory 定义走假实现；Room 构造器捕获自安装位（官方
  // globals.Room 闭包引用等价——.prototype 运行期解引用，玩家原型补丁/
  // 重挂语义保持可见，绑定交换不可）
  const __Object = Object;
  const __RoomCtor = globalThis.Room;
  const __mapGet = Map.prototype.get;
  const __reflectApply = Reflect.apply;
  const __jsonStringify = JSON.stringify;
  const __jsonParse = JSON.parse;
  // 装配路径 push（A5 片 3a 评审 P1 清偿）：look 索引构建在 beginTick 宿主
  // 装配路径执行，玩家可换的 Array.prototype.push 一旦被下毒即断管装配——
  // 加载期捕获（__mapSet/__arrayIncludes 同批先例）
  const __arrayPush = Array.prototype.push;
  // 常量加载期捕获（片 3a 评审 P2 清偿，官方闭包 C 免疫玩家覆写）：
  // ERR_INVALID_ARGS（lookForAt 未知类型返码）、TERRAIN_MASK_*（terrainWordAt
  // 自本片起为全部 look 面 terrain 判定的承重路径）
  const ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const TERRAIN_MASK_WALL = globalThis.TERRAIN_MASK_WALL;
  const TERRAIN_MASK_SWAMP = globalThis.TERRAIN_MASK_SWAMP;

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
    // look 索引（A5 片 3a，官方 game.js:38-47 addObjectToRegister +
    // rooms.js:485-513 lookTypeRegisters/lookTypeSpatialRegisters 映射）：
    // type → (id→对象)/(稀疏数组)，索引官方转置公式 x*50+y（读写两侧同式，
    // 官方原样）；energy/resource 双键同指（rooms.js:487-488/:501-502——
    // lookAt 固定序里同格资源产出两条 {type:'energy'} 与 {type:'resource'}）。
    // 与 controller 扫描合并单趟（评审 P3-2：每房每 tick 一趟全量扫描）
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
    lookSpatial.energy = [];
    lookSpatial.resource = lookSpatial.energy;
    for (let i = 0; i < worldObjs.length; i++) {
      const o = worldObjs[i];
      if (o.room !== name) continue;
      if (o.type === 'structure' && o.structureType === 'controller') {
        controller = __reflectApply(__mapGet, gw.objectsById, [o.id]);
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
      storage: undefined,
      terminal: undefined,
      visual: new RoomVisual(name),
      mode: 'world',
    };
    // findCache（官方 rooms.js:585-598 的 register.findCache 出口臂子集——
    // makeRoom 每 tick 重建，闭包内对象即天然 per-tick）：方向臂缓存**未滤**
    // 结果（官方 :641 缓存在尾滤前）；FIND_EXIT 缓存**已滤** concat（子臂先
    // 各自过 opts.filter——官方 :592-598 陈旧 filter 烘焙 quirk 原样保留）；
    // 命中/未命中统一经尾部两分支产**新数组**（官方 :648-655——filter 新
    // 数组或 _.clone 浅拷，元素同实例、本体从不暴露）。对象臂的官方装配期
    // findCacheFn 预填充面我方未移植（每调用新鲜扫描，纯性能差，登记灰区）
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
      // 索引迭代（装配批纪律）：for...of 经可交换的 Symbol.iterator。
      // 裸 run（viewerName=null，无 user 的注入面）= 视角中立面：与 Rust
      // matches_find 的 None 臂同构——MY_*/HOSTILE_* 不可判定返回空、
      // 101 保留孵化雏形（评审 1 F-1：两层中立语义同步收口）
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
    // terrain 第 10 位→nuke→tombstone→ruin→powerCreep）；官方 customObjectPrototypes
    // 扩展钩我方引擎无自定义原型面，不接入
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
    room.createConstructionSite = function () { countStub('room.createConstructionSite'); return 0; };
    // 官方 rooms.js:573-583：raw=true 返回**持久化原始字符串**（不可变——
    // 玩家改非 raw 拿到的解析数组不得反向改写 raw 输出，GPT 复审 P1-5 清偿：
    // 原实现 raw 分支即时序列化同一可变数组，改 data.amount 会污染 raw）；
    // 非 raw 返回解析缓存（官方 roomEventLogCache 同 run 同引用同构——装配期
    // 一次定格，同 tick 多次调用同引用）。字符串经预置 stringify（宿主数据
    // 编码不经可换全局）。B3 落账 events 上一 tick 覆写
    const eventLogRaw = __jsonStringify((r && r.events) || []);
    const eventLogParsed = __jsonParse(eventLogRaw);
    room.getEventLog = function (raw) {
      return raw ? eventLogRaw : eventLogParsed;
    };
    room.createFlag = function () { countStub('room.createFlag'); return 'stub-flag'; };
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
