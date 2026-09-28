/* kk-runner 桩环境 prelude · 段 3：RawMemory/Memory + RoomPosition + 类全局 + 世界投影。
 *
 * tick 生命周期状态（world/terrainByRoom/objectsById、parsedMemory）在本段闭包内，
 * beginTick/writeBackMemory 经 __kkp.memoryWorld 暴露给 90_host 的宿主通道。
 */
(() => {
  'use strict';

  // 预置 JSON 函数（官方 systemFunctions.JSON 同款）：Memory 写回等宿主
  // 关键序列化不经玩家可覆写的全局（外部复审 P1 修复面）
  const __jsonStringify = JSON.stringify;

  // 当前查看者（world.user，官方 runtimeData.user 等价物）——per-viewer
  // 装配（Game.collections/my/FIND_MY_*）的判定基准（外部复审 P1-3 修复）
  let viewerName = null;


  const { ops, probe, reportMissing, PROBE_IGNORE, countStub } = globalThis.__kkp;

  // ---------------------------------------------------------------- RawMemory / Memory
  // 官方语义：RawMemory 是 2MB 上限的原生存档字符串；Memory 是懒解析 getter，
  // 首次访问 JSON.parse(RawMemory.get())，tick 末写回。

  const RAW_MEMORY_LIMIT = 2 * 1024 * 1024;
  let rawMemoryData = '';
  let parsedMemory;
  const RawMemory = {
    get() { return rawMemoryData; },
    set(value) {
      const s = String(value);
      if (s.length > RAW_MEMORY_LIMIT) {
        throw new Error('RawMemory 超出 2MB 上限');
      }
      rawMemoryData = s;
    },
    segments: Object.create(null),
    setActiveSegments() { countStub('RawMemory.setActiveSegments'); return 0; },
    getActiveSegments() { countStub('RawMemory.getActiveSegments'); return []; },
    getSegment() { countStub('RawMemory.getSegment'); return ''; },
    setSegment() { countStub('RawMemory.setSegment'); return 0; },
    interShardSegment: null,
    setInterShardMemory() { countStub('RawMemory.setInterShardMemory'); return 0; },
    getInterShardMemory() { countStub('RawMemory.getInterShardMemory'); return ''; },
  };
  globalThis.RawMemory = RawMemory;

  function ensureMemory() {
    if (parsedMemory === undefined) {
      try {
        parsedMemory = rawMemoryData ? JSON.parse(rawMemoryData) : {};
      } catch (e) {
        console.error('Memory 解析失败，重置为空对象: ' + e.message);
        parsedMemory = {};
      }
      if (parsedMemory === null || typeof parsedMemory !== 'object') parsedMemory = {};
    }
    // 官方 game.js:492-495：首次访问后把自己重定义为值属性——注意官方**没有**
    // 写 writable（默认 false）：Memory 首访后是只读的，`Memory = {...}` 官方会
    // 静默失败（严格模式抛错）。对齐官方描述符，不自行放宽
    Object.defineProperty(globalThis, 'Memory', {
      configurable: true,
      enumerable: true,
      value: parsedMemory,
    });
    return parsedMemory;
  }
  function writeBackMemory() {
    // 返回 {ok} | {ok:false,error}：screeps-ok 契约（adapter-spec:229）把 tick 末
    // 序列化失败归为 runPlayer 的 runtime 错误——不能只在 console 里报完就算成功。
    // 玩家侧写回错误（不可序列化 / Memory.toJSON 返回非字符串等）一律走这里，
    // 绝不抛出——抛出会被宿主误判为硬中断并按整份 tickLimit 计费
    if (parsedMemory === undefined) return { ok: true };
    try {
      const s = __jsonStringify(parsedMemory);
      if (typeof s !== 'string') {
        // Memory.toJSON 返回非字符串时 stringify 得 undefined——玩家侧写回失败
        console.error('Memory 写回失败（toJSON 返回非字符串），保留上一 tick 存档');
        return { ok: false, error: 'Memory.toJSON 返回了非字符串' };
      }
      if (s.length > RAW_MEMORY_LIMIT) {
        console.error('Memory 写回超过 2MB 上限，保留上一 tick 存档');
        return { ok: false, error: 'Memory 写回超过 2MB 上限' };
      }
      rawMemoryData = s;
      return { ok: true };
    } catch (e) {
      console.error('Memory 写回失败（不可序列化），保留上一 tick 存档: ' + (e && e.message));
      return { ok: false, error: 'Memory 不可序列化: ' + (e && e.message) };
    }
  }
  // Memory 作为懒 getter（官方语义：访问才解析）。官方 game.js:470-473 两处
  // defineProperty 均 enumerable: true（初始 getter 与首访固化后的 value 属性）
  Object.defineProperty(globalThis, 'Memory', {
    configurable: true,
    enumerable: true,
    get() { return ensureMemory(); },
  });

  // ---------------------------------------------------------------- RoomPosition
  // 官方全局类；纯计算方法给真实现，副作用方法为桩。

  // 参数形态归一（官方 utils.js:34-63 fetchXYArguments 的桩版）：
  // 第二参是 number → 数值形态（x 直接当数字用，roomName=undefined 视为同房间）；
  // 否则对象形态（RoomObject 先解包 .pos，RoomPosition 用自身）
  function fetchXY(x, y) {
    if (typeof y === 'number') return { x: Number(x), y, roomName: undefined };
    const p = x && typeof x === 'object' && x.pos && typeof x.pos === 'object'
      && typeof x.pos.x === 'number' ? x.pos : x;
    return p;
  }

  // 官方 engine/src/utils.js roomNameToXY 逐字移植（玩家可见的宽松解析——
  // parseInt 认前缀数字、W/w 与 N/n 取负减一、其余方向字母当 E/S；与 kk-core
  // RoomName 的强类型构造校验是两层：embed 边界拒非法名，玩家面对齐官方行为）
  function roomNameToXY(name) {
    let xx = parseInt(name.substr(1), 10);
    let verticalPos = 2;
    if (xx >= 100) verticalPos = 4;
    else if (xx >= 10) verticalPos = 3;
    let yy = parseInt(name.substr(verticalPos + 1), 10);
    if (name.charAt(0) === 'W' || name.charAt(0) === 'w') xx = -xx - 1;
    if (name.charAt(verticalPos) === 'N' || name.charAt(verticalPos) === 'n') yy = -yy - 1;
    return [xx, yy];
  }

  class RoomPosition {
    constructor(x, y, roomName) {
      this.x = x;
      this.y = y;
      this.roomName = roomName;
    }    get room() { return Game.rooms[this.roomName] || undefined; }
    // 官方 rooms.js:1358：`[room ${roomName} pos ${x},${y}]`——bot 生态日志拼接的事实标准
    toString() { return '[room ' + this.roomName + ' pos ' + this.x + ',' + this.y + ']'; }
    // 官方 utils.js:34-63 fetchXYArguments：第二参是 number → 数值形态（roomName 为
    // undefined，视为同房间）；否则对象形态（RoomObject 先解包 .pos）
    isEqualTo(x, y) {
      // 官方 rooms.js:1579：目标带 roomName 时房间也必须相等（数值形态视为同房间）
      const p = fetchXY(x, y);
      return this.x === p.x && this.y === p.y && (!p.roomName || p.roomName === this.roomName);
    }
    isNearTo(x, y) { return this.getRangeTo(x, y) <= 1; }
    isInRangeTo(x, y, range) {
      // 官方 rooms.js:1362-1376：第三参缺省 → 两参形态（目标 + range），目标先
      // 解包 .pos；否则三参 (x, y, range)。逐轴 abs 比较（= Chebyshev）
      let tx = x, ty = y, r = range, roomName;
      if (r === undefined) {
        let p = x;
        if (p && p.pos) p = p.pos;
        tx = p.x; ty = p.y; roomName = p.roomName;
        r = y;
      }
      return Math.abs(tx - this.x) <= r && Math.abs(ty - this.y) <= r
        && (!roomName || roomName === this.roomName);
    }
    getRangeTo(x, y) {
      const p = fetchXY(x, y);
      // 官方 rooms.js:1588：仅目标带 roomName 且不等时才跨房间；数值形态视为同房间
      if (p.roomName && this.roomName !== p.roomName) return Infinity;
      return Math.max(Math.abs(this.x - p.x), Math.abs(this.y - p.y));
    }
    getDirectionTo(x, y) {
      // 官方 utils.js:64-103 getDirection 的逐字移植（阈值判定而非角度最近）：
      //   |dx| > 2|dy| → 左右；|dy| > 2|dx| → 上下；否则对角。
      // 官方对 (0,0) 无特判（落到 else 无命中分支 → undefined）。
      // 跨房间（rooms.js:1383-1393）：目标带 roomName 且不等时按世界坐标换算
      const p = fetchXY(x, y);
      let dx = p.x - this.x, dy = p.y - this.y;
      if (p.roomName && p.roomName !== this.roomName) {
        const [tx, ty] = roomNameToXY(p.roomName);
        const [mx, my] = roomNameToXY(this.roomName);
        dx = tx * 50 + p.x - mx * 50 - this.x;
        dy = ty * 50 + p.y - my * 50 - this.y;
      }
      const adx = Math.abs(dx), ady = Math.abs(dy);
      if (adx > ady * 2) return dx > 0 ? RIGHT : LEFT;
      if (ady > adx * 2) return dy > 0 ? BOTTOM : TOP;
      if (dx > 0 && dy > 0) return BOTTOM_RIGHT;
      if (dx > 0 && dy < 0) return TOP_RIGHT;
      if (dx < 0 && dy > 0) return BOTTOM_LEFT;
      if (dx < 0 && dy < 0) return TOP_LEFT;
      return undefined;
    }
    findClosestByRange() { countStub('RoomPosition.findClosestByRange'); return null; }
    findClosestByPath() { countStub('RoomPosition.findClosestByPath'); return null; }
    // 官方 rooms.js:1518-1543：数字 type 走 room.find（opts 透传含 filter）、
    // 数组 type 直接过滤；不可达房抛错；inRangeTo（Chebyshev）过滤
    findInRange(type, range, opts) {
      const room = Game.rooms[this.roomName];
      if (!room) throw new Error('Could not access room ' + this.roomName);
      const objects = typeof type === 'number'
        ? room.find(type, opts)
        : (opts && opts.filter ? _.filter(type, opts.filter) : type);
      return objects.filter((i) => this.inRangeTo(i, range));
    }
    findPathTo() { countStub('RoomPosition.findPathTo'); return []; }
    moveTo() { countStub('RoomPosition.moveTo'); return 0; }
    createConstructionSite() { countStub('RoomPosition.createConstructionSite'); return 0; }
    createFlag() { countStub('RoomPosition.createFlag'); return 'stub-flag'; }
    lookFor(type) {
      // 官方 rooms.js:1601-1610：terrain 分支直接读静态地形返回单词数组；
      // 其余 look 类型走 room.lookForAt（M1 桩）
      if (type === LOOK_TERRAIN) {
        return [terrainWordAt(this.roomName, this.x, this.y)];
      }
      countStub('RoomPosition.lookFor'); return [];
    }
    inRangeTo() { return this.isInRangeTo.apply(this, arguments); }
  }
  globalThis.RoomPosition = RoomPosition;

  // ---------------------------------------------------------------- Terrain
  // 官方 rooms.js:1223-1241 Room.Terrain 逐字移植：闭包式对象（非 class），
  // 无地形数据的房间构造即抛错；get 返回位码（wall=1/swamp=2/plain=0），
  // getRawBuffer 填充给定 TypedArray或新建 Uint8Array。地形数据经世界 JSON
  // 注入（terrainByRoom，beginTick 填充）。

  function Terrain(roomName) {
    const array = terrainByRoom['' + roomName];
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

  // 地形单词（look 面）：swamp 先判、wall 后覆盖（rooms.js:663-671）。
  // 无地形数据的房间（玩家自造未注册房间名）自然 TypeError——与官方
  // _lookSpatialRegister 直读 staticTerrainData[id]（rooms.js:665）的行为一致；
  // 世界内全部房间恒有地形（world_json 恒发 terrain）
  function terrainWordAt(roomName, x, y) {
    const t = terrainByRoom[roomName][y * 50 + x];
    let result = 'plain';
    if (t & TERRAIN_MASK_SWAMP) result = 'swamp';
    if (t & TERRAIN_MASK_WALL) result = 'wall';
    return result;
  }

  // 常量在 prelude 之前已平铺为裸全局（TOP/OK/FIND_* 等，官方形态）。

  // ---------------------------------------------------------------- PathFinder 桩
  // M0：直线步进 + incomplete:false 的假结果，保证 bot 流程可跑；
  // 真实现（FFI 官方 pf.cc，逐点一致）在 M2。

  class CostMatrix {
    constructor() { this._bits = new Uint8Array(2500); }
    set(x, y, v) { this._bits[y * 50 + x] = v; return this; }
    get(x, y) { return this._bits[y * 50 + x]; }
    copy() { const c = new CostMatrix(); c._bits.set(this._bits); return c; }
    serialize() { countStub('CostMatrix.serialize'); return { bits: Array.from(this._bits) }; }
    static deserialize() { countStub('CostMatrix.deserialize'); return new CostMatrix(); }
  }
  function straightPath(from, to, range) {
    // 官方 path-finder.js：search 返回的 path 元素是 **RoomPosition 实例**（绝对
    // 坐标）——bot 生态（tooangel 等）直接在这些元素上调用方法（含其自打的
    // 原型补丁），返回 {dx,dy} 步进对象会让 bot 每 tick 崩溃
    const positions = [];
    let x = from.x, y = from.y;
    const roomName = from.roomName;
    let guard = 0;
    // 官方 pf.cc:574：回溯在 pos == origin 处停止——path **不含起点**（bot 生态
    // 依赖此语义，tooangel 甚至自行 splice 把起点插回头部）；同起终点 → 空路径。
    // goal {pos, range}：进入目标的 range（Chebyshev）邻域即停，range 缺省 0——
    // 官方路径止于 range 边界，bot 自行拼端点（tooangel buildPath 的 push(end)）
    // 才不会产生重复项
    const dist = () => Math.max(Math.abs(to.x - x), Math.abs(to.y - y));
    while (dist() > range && guard++ < 100) {
      x += Math.sign(to.x - x);
      y += Math.sign(to.y - y);
      positions.push(new RoomPosition(x, y, roomName));
    }
    return positions;
  }
  const PathFinder = {
    CostMatrix,
    use() { countStub('PathFinder.use'); },
    search(origin, goal, opts) {
      countStub('PathFinder.search');
      const goals = Array.isArray(goal) ? goal : [goal];
      const g0 = goals[0];
      const to = g0 && g0.pos ? g0.pos : g0;
      // 官方 goal 形态：{pos, range} 或裸 RoomPosition（range 0）；负 range
      // 夹 0（官方 driver/lib/path-finder.js:106 Math.max(0, range|0)）
      const range = g0 && g0.pos && typeof g0.range === 'number'
        ? Math.max(0, g0.range) : 0;
      const path = straightPath(
        origin.pos ? origin.pos : origin,
        { x: to.x, y: to.y },
        range,
      );
      // 官方 ops 是搜索消耗计数（非空路径恒 ≥1，pf.cc:594）；直线桩以步数为近似
      return { path, ops: path.length, cost: path.length, incomplete: false };
    },
  };
  globalThis.PathFinder = PathFinder;

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
  // 官方 Terrain 挂法：Room 的静态属性（rooms.js:1135 `new Room.Terrain(...)`），
  // 非裸全局
  globalThis.Room.Terrain = Terrain;

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

  // ---------------------------------------------------------------- 世界投影（stub Game）

  let world = { time: 0, cpu: { limit: 20, tickLimit: 500, bucket: 10000 }, rooms: {}, objects: [] };
  let terrainByRoom = {};
  let objectsById = new Map();

  function creepMemoryFor(name) {
    const m = ensureMemory();
    if (!m.creeps) m.creeps = {};
    if (!m.creeps[name]) m.creeps[name] = {};
    return m.creeps[name];
  }

  function mkStubMethod(label) {
    return function () {
      countStub(label);
      return 0; // OK
    };
  }
  const CREEP_METHODS = ['move', 'moveByPath', 'moveTo', 'harvest', 'build', 'repair', 'dismantle', 'upgradeController', 'attack', 'rangedAttack', 'rangedMassAttack', 'heal', 'rangedHeal', 'claimController', 'reserveController', 'attackController', 'transfer', 'withdraw', 'drop', 'pickup', 'say', 'suicide', 'spawn', 'generateSafeMode', 'pull', 'recycleCreep', 'renewCreep', 'cancelOrder', 'moveHead'];

  // API 实现用的预置常量（官方 game 层的 C 来自 driver.constants 注入对象，
  // 玩家覆盖全局常量不影响 API 判定——game.js 头部 C = driver.constants 同款）
  // 对象型常量是全局对象的共享引用——冻结内容防玩家篡改（数值/字符串不可变无需处理）
  function freezeConst(obj) {
    if (obj && typeof obj === 'object') {
      Object.freeze(obj);
      for (const k of Object.keys(obj)) freezeConst(obj[k]);
    }
    return obj;
  }

  const C = {
    OK: globalThis.OK,
    ERR_NOT_OWNER: globalThis.ERR_NOT_OWNER,
    ERR_BUSY: globalThis.ERR_BUSY,
    ERR_TIRED: globalThis.ERR_TIRED,
    ERR_NO_BODYPART: globalThis.ERR_NO_BODYPART,
    ERR_INVALID_TARGET: globalThis.ERR_INVALID_TARGET,
    ERR_NOT_IN_RANGE: globalThis.ERR_NOT_IN_RANGE,
    ERR_NOT_ENOUGH_RESOURCES: globalThis.ERR_NOT_ENOUGH_RESOURCES,
    ERR_INVALID_ARGS: globalThis.ERR_INVALID_ARGS,
    MOVE: globalThis.MOVE,
    WORK: globalThis.WORK,
    ATTACK: globalThis.ATTACK,
    HEAL: globalThis.HEAL,
    ERR_NOT_FOUND: globalThis.ERR_NOT_FOUND,
    ERR_RCL_NOT_ENOUGH: globalThis.ERR_RCL_NOT_ENOUGH,
    OBSTACLE_OBJECT_TYPES: freezeConst(globalThis.OBSTACLE_OBJECT_TYPES),
    CONSTRUCTION_COST: freezeConst(globalThis.CONSTRUCTION_COST),
    CONTROLLER_STRUCTURES: freezeConst(globalThis.CONTROLLER_STRUCTURES),
  };

  // 官方 _hasActiveBodypart（game/creeps.js:9-17 的等价物）：hits>0 的指定部件数
  function countActiveParts(body, type) {
    let n = 0;
    for (const p of body || []) if (p.type === type && p.hits > 0) n += 1;
    return n;
  }

  /** Creep.move：官方 creeps.js:126-156——重预检（fatigue/bodypart/方向）后记
   *  intent 返回 OK；朝相邻 creep 形态（target 为 Creep 实例）同官方 :132-141。 */
  function mkCreepMove(creepObj) {
    return function move(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (target && target instanceof globalThis.Creep) {
        if (!target.pos.isNearTo(creepObj.pos)) return C.ERR_NOT_IN_RANGE;
        intents.set(creepObj.id, 'move', { id: target.id });
        return C.OK;
      }
      if (creepObj.fatigue > 0) return C.ERR_TIRED;
      if (countActiveParts(creepObj.body, C.MOVE) === 0) return C.ERR_NO_BODYPART;
      const direction = +target;
      if (!direction || direction < 1 || direction > 8) return C.ERR_INVALID_ARGS;
      intents.set(creepObj.id, 'move', { direction });
      return C.OK;
    };
  }

  /** Creep.harvest：官方 creeps.js:335-402 的 source 分支（mineral/deposit 随
   *  对应能力批）——目标必须是 Source 实例（register.sources 判定的等价物）。 */
  function mkCreepHarvest(creepObj) {
    return function harvest(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (countActiveParts(creepObj.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
      if (!target || !target.id) return C.ERR_INVALID_TARGET;
      if (target instanceof globalThis.Mineral) {
        // 官方 mineral 分支（creeps.js:362-385）：矿量/邻接/提取器存在性与
        // 归属/启用/冷却。deposit 分支随 B4。
        if (!target.mineralAmount) return C.ERR_NOT_ENOUGH_RESOURCES;
        if (!target.pos.isNearTo(creepObj.pos)) return C.ERR_NOT_IN_RANGE;
        let extractor = null;
        for (const o of world.objects || []) {
          if (o.room === target.pos.roomName && o.x === target.pos.x && o.y === target.pos.y
            && o.structureType === 'extractor') {
            extractor = objectsById.get(o.id);
            break;
          }
        }
        if (!extractor) return C.ERR_NOT_FOUND;
        if (extractor.owner && !extractor.my) return C.ERR_NOT_OWNER;
        if (!structureActiveAt(extractor, creepObj.room && creepObj.room.controller)) {
          return C.ERR_RCL_NOT_ENOUGH;
        }
        if (extractor.cooldown) return C.ERR_TIRED;
        intents.set(creepObj.id, 'harvest', { id: target.id });
        return C.OK;
      }
      if (!(target instanceof globalThis.Source)) {
        // 其余目标同官方 register.assertTargetObject
        return C.ERR_INVALID_TARGET;
      }
      if (!target.energy) return C.ERR_NOT_ENOUGH_RESOURCES;
      if (!target.pos.isNearTo(creepObj.pos)) return C.ERR_NOT_IN_RANGE;
      const ctl = creepObj.room && creepObj.room.controller;
      if (ctl && ctl.owner && ctl.owner.username !== creepObj.owner.username) {
        return C.ERR_NOT_OWNER; // reservation 分支缺字段（灰区登记，B2 补）
      }
      intents.set(creepObj.id, 'harvest', { id: target.id });
      return C.OK;
    };
  }

  // ---- 以下动作方法的预检逐条对照官方 creeps.js（file:line 见各函数）；
  // heal/rangedHeal 无执行器（B 批后续）但 intent 记录即参与压制图裁决 ----

  /** 官方 `controller && !controller.my && controller.safeMode` 判定
   *  （attack/heal/rangedHeal/dismantle 的 API 层门，creeps.js:602/699/723/1045）。 */
  function blockedBySafeMode(creepObj) {
    const ctl = creepObj.room && creepObj.room.controller;
    return !!(ctl && !ctl.my && ctl.safeMode);
  }

  function carriedEnergy(creepObj) {
    return (creepObj.store && creepObj.store.energy) || 0;
  }

  /** 单实例 isActive 近似（官方 utils.js checkStructureAgainstController 的
   *  单结构面）：无 controller 恒 active；owned 房按 CONTROLLER_STRUCTURES
   *  该类型在房 rcl 的配额 >0。isActive API 本体随 A4。 */
  function structureActiveAt(o, ctl) {
    if (!ctl) return true;
    const quota = (C.CONTROLLER_STRUCTURES || {})[o.structureType];
    if (!quota) return true;
    const level = Math.min(Math.max(ctl.level || 0, 0), 8);
    return (quota[level] || 0) > 0;
  }

  /** 官方 attack（creeps.js:593-624）：ATTACK 部件 + Creep/Structure 目标 +
   *  邻接 + safeMode 门；effects 检查随 power 批（M2 无 effects 面）。 */
  function mkCreepAttack(creepObj) {
    return function attack(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (countActiveParts(creepObj.body, C.ATTACK) === 0) return C.ERR_NO_BODYPART;
      if (blockedBySafeMode(creepObj)) return C.ERR_NO_BODYPART;
      const okTarget = target && target.id
        && (target instanceof globalThis.Creep
          || target instanceof globalThis.StructureSpawn
          || target instanceof globalThis.Structure);
      if (!okTarget) return C.ERR_INVALID_TARGET;
      if (!target.pos.isNearTo(creepObj.pos)) return C.ERR_NOT_IN_RANGE;
      intents.set(creepObj.id, 'attack', { id: target.id, x: target.pos.x, y: target.pos.y });
      return C.OK;
    };
  }

  /** 官方 build（creeps.js:762-808）：WORK + 能量 + 工地目标 + 3 格 +
   *  目标格静态障碍/挡路 creep 预检（safeMode 敌我分支 :794-805）。 */
  function mkCreepBuild(creepObj) {
    return function build(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (countActiveParts(creepObj.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
      if (!carriedEnergy(creepObj)) return C.ERR_NOT_ENOUGH_RESOURCES;
      if (!(target && target.id && target instanceof globalThis.ConstructionSite)) {
        return C.ERR_INVALID_TARGET;
      }
      if (!creepObj.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
      if ((C.OBSTACLE_OBJECT_TYPES || []).includes(target.structureType)) {
        let objectsInTile = false;
        let creepsInTile = false;
        let myCreepsInTile = false;
        for (const o of world.objects || []) {
          if (o.room !== target.pos.roomName || o.x !== target.pos.x || o.y !== target.pos.y) continue;
          const t = o.type === 'structure' || o.type === 'spawn'
            ? (o.structureType || 'spawn') : o.type;
          if (!(C.OBSTACLE_OBJECT_TYPES || []).includes(t)) continue;
          if (o.type === 'creep') {
            creepsInTile = true;
            if (o.owner === creepObj.owner.username) myCreepsInTile = true;
          } else {
            objectsInTile = true;
          }
        }
        if (objectsInTile) return C.ERR_INVALID_TARGET;
        const ctl = creepObj.room && creepObj.room.controller;
        const blocking = ctl && ctl.my && ctl.safeMode ? myCreepsInTile : creepsInTile;
        if (blocking) return C.ERR_INVALID_TARGET;
      }
      intents.set(creepObj.id, 'build', { id: target.id, x: target.pos.x, y: target.pos.y });
      return C.OK;
    };
  }

  /** 官方 repair（creeps.js:734-761）：WORK + 能量 + Structure 目标 + 3 格。 */
  function mkCreepRepair(creepObj) {
    return function repair(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (countActiveParts(creepObj.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
      if (!carriedEnergy(creepObj)) return C.ERR_NOT_ENOUGH_RESOURCES;
      const okTarget = target && target.id
        && (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn);
      if (!okTarget) return C.ERR_INVALID_TARGET;
      if (!creepObj.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
      intents.set(creepObj.id, 'repair', { id: target.id, x: target.pos.x, y: target.pos.y });
      return C.OK;
    };
  }

  /** 官方 dismantle（creeps.js:1024-1053）：WORK + 可拆结构
   *  （CONSTRUCTION_COST 有价）+ 邻接 + safeMode；effects 随 power 批。 */
  function mkCreepDismantle(creepObj) {
    return function dismantle(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (countActiveParts(creepObj.body, C.WORK) === 0) return C.ERR_NO_BODYPART;
      const okTarget = target && target.id
        && (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn)
        && C.CONSTRUCTION_COST && target.structureType in C.CONSTRUCTION_COST;
      if (!okTarget) return C.ERR_INVALID_TARGET;
      if (!target.pos.isNearTo(creepObj.pos)) return C.ERR_NOT_IN_RANGE;
      if (blockedBySafeMode(creepObj)) return C.ERR_NO_BODYPART;
      intents.set(creepObj.id, 'dismantle', { id: target.id });
      return C.OK;
    };
  }

  /** 官方 heal（creeps.js:678-705）：HEAL + Creep 目标 + 邻接 + safeMode。 */
  function mkCreepHeal(creepObj) {
    return function heal(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (countActiveParts(creepObj.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
      if (!(target && target.id && target instanceof globalThis.Creep)) {
        return C.ERR_INVALID_TARGET;
      }
      if (!target.pos.isNearTo(creepObj.pos)) return C.ERR_NOT_IN_RANGE;
      if (blockedBySafeMode(creepObj)) return C.ERR_NO_BODYPART;
      intents.set(creepObj.id, 'heal', { id: target.id, x: target.pos.x, y: target.pos.y });
      return C.OK;
    };
  }

  /** 官方 cancelOrder（creeps.js:1016-1022）：按动作名移除已记录的 intent，
   *  成功返回 OK（收集器退费），未记录返回 ERR_NOT_FOUND。 */
  function mkCreepCancelOrder(creepObj) {
    return function cancelOrder(name) {
      if (intents.remove(creepObj.id, name)) return C.OK;
      return C.ERR_NOT_FOUND;
    };
  }

  /** 官方 rangedHeal（creeps.js:706-733）：HEAL + Creep 目标 + 3 格 + safeMode。 */
  function mkCreepRangedHeal(creepObj) {
    return function rangedHeal(target) {
      if (!creepObj.my) return C.ERR_NOT_OWNER;
      if (creepObj.spawning) return C.ERR_BUSY;
      if (countActiveParts(creepObj.body, C.HEAL) === 0) return C.ERR_NO_BODYPART;
      if (!(target && target.id && target instanceof globalThis.Creep)) {
        return C.ERR_INVALID_TARGET;
      }
      if (blockedBySafeMode(creepObj)) return C.ERR_NO_BODYPART;
      if (!creepObj.pos.inRangeTo(target.pos, 3)) return C.ERR_NOT_IN_RANGE;
      intents.set(creepObj.id, 'rangedHeal', { id: target.id });
      return C.OK;
    };
  }
  const SPAWN_METHODS = ['spawnCreep', 'renewCreep', 'recycleCreep', 'destroy', 'setActive', 'notifyWhenAttacked'];
  const STRUCTURE_METHODS = ['destroy', 'setActive', 'notifyWhenAttacked'];
  const CONTROLLER_METHODS = ['activateSafeMode'];
  const SOURCE_METHODS = [];

  function makeStore(storeSpec) {
    const store = Object.assign({}, storeSpec);
    store.getUsedCapacity = (resource) => storeSpec[resource || 'energy'] || 0;
    store.getFreeCapacity = () => 0;
    store.getCapacity = () => null;
    return new Proxy(store, {
      get(t, prop) {
        if (prop in t) return t[prop];
        if (typeof prop === 'string') reportMissing('store.' + prop);
        return undefined;
      },
    });
  }

  function wrapObject(o, now) {
    // pos 套探测仪表：RoomPosition 实例面的官方缺口（如 findInRange）以
    // 'RoomPosition.*' 登记，不静默 undefined
    const pos = probe(new RoomPosition(o.x, o.y, o.room), 'RoomPosition');
    const base = {
      id: o.id,
      pos,
      room: undefined, // 由 buildGame 回填（Game.rooms[name]）
      hits: o.hits ?? 0,
      hitsMax: o.hitsMax ?? 0,
    };
    let obj;
    switch (o.type) {
      case 'creep': {
        obj = Object.assign(base, {
          name: o.name,
          my: o.owner === viewerName,
          owner: { username: o.owner || 'bot' },
          body: (o.body || []).map((p) => (typeof p === 'string' ? { type: p, hits: 100 } : p)),
          fatigue: o.fatigue || 0,
          saying: '',
          spawning: false,
          store: makeStore(o.store || { energy: 0 }),
          _memoryName: o.name,
        });
        Object.defineProperty(obj, 'memory', {
          configurable: true,
          get: () => creepMemoryFor(o.name),
          set: (v) => { const m = ensureMemory(); if (!m.creeps) m.creeps = {}; m.creeps[o.name] = v; },
        });
        // A2 起动作方法逐步从桩换真（预检 + intents.set，官方 creeps.js 形态）；
        // 未换真的方法仍是桩（stub_calls 计数，A4 批次清偿）
        for (const m of CREEP_METHODS) {
          if (m === 'move') obj.move = mkCreepMove(obj);
          else if (m === 'harvest') obj.harvest = mkCreepHarvest(obj);
          else if (m === 'attack') obj.attack = mkCreepAttack(obj);
          else if (m === 'build') obj.build = mkCreepBuild(obj);
          else if (m === 'repair') obj.repair = mkCreepRepair(obj);
          else if (m === 'dismantle') obj.dismantle = mkCreepDismantle(obj);
          else if (m === 'heal') obj.heal = mkCreepHeal(obj);
          else if (m === 'rangedHeal') obj.rangedHeal = mkCreepRangedHeal(obj);
          else if (m === 'cancelOrder') obj.cancelOrder = mkCreepCancelOrder(obj);
          else obj[m.trim()] = mkStubMethod('creep.' + m.trim());
        }
        break;
      }
      case 'spawn': {
        obj = Object.assign(base, {
          name: o.name,
          my: o.owner === viewerName,
          owner: { username: o.owner || 'bot' },
          structureType: o.structureType || 'spawn',
          store: makeStore(o.store || { energy: 0 }),
          energy: (o.store && o.store.energy) || 0,
          energyCapacity: o.energyCapacity || 300,
          spawning: null,
          isStructure: true,
        });
        obj.isActive = () => { countStub('structure.isActive'); return true; };
        for (const m of SPAWN_METHODS) obj[m] = mkStubMethod('spawn.' + m);
        break;
      }
      case 'structure': {
        obj = Object.assign(base, {
          structureType: o.structureType,
          my: o.owner === viewerName,
          owner: o.owner ? { username: o.owner } : undefined,
          isStructure: true,
          store: makeStore(o.store || {}),
        });
        if (o.structureType === 'controller') {
          obj.level = o.level || 0;
          obj.progress = o.progress || 0;
          obj.progressTotal = o.progressTotal || 0;
          obj.reservation = undefined;
          // 官方 getter（structures.js:174-186）：绝对存储 − 当前 tick = 剩余量；
          // 无值/已过期归 0（None→0 投影归一，灰区登记口径）
          obj.safeMode = o.safeMode && o.safeMode > now ? o.safeMode - now : 0;
          obj.safeModeAvailable = o.safeModeAvailable ?? 0;
          obj.safeModeCooldown = 0;
          obj.ticksToDowngrade = o.downgradeTime != null ? o.downgradeTime - now : 0;
          obj.activationDate = undefined;
          // 官方 controller.isPowerEnabled（power API 本体 M2+，字段先行透传）
          obj.isPowerEnabled = o.isPowerEnabled === true;
          for (const m of CONTROLLER_METHODS) obj[m] = mkStubMethod('controller.' + m);
        } else if (o.structureType === 'source') {
          obj.energy = o.energy ?? 3000;
          obj.energyCapacity = o.energyCapacity ?? 3000;
          obj.ticksToRegeneration = o.ticksToRegeneration ?? 0;
        } else if (o.structureType === 'mineral') {
          obj.mineralType = o.mineralType || 'XUH2O';
          obj.mineralAmount = o.mineralAmount ?? 0;
          obj.ticksToRegeneration = o.ticksToRegeneration ?? 0;
          obj.density = o.density ?? 3;
        } else {
          obj.energy = (o.store && o.store.energy) || 0;
          obj.energyCapacity = o.energyCapacity || 0;
          obj.isActive = () => { countStub('structure.isActive'); return true; };
          for (const m of STRUCTURE_METHODS) obj[m] = mkStubMethod('structure.' + m);
        }
        break;
      }
      case 'constructionSite': {
        obj = Object.assign(base, {
          structureType: o.structureType,
          progress: o.progress || 0,
          progressTotal: o.progressTotal || 100,
          my: o.owner === viewerName,
        });
        obj.remove = mkStubMethod('constructionSite.remove');
        break;
      }
      case 'flag': {
        obj = {
          name: o.name,
          color: o.color || 10,
          secondaryColor: o.secondaryColor || 10,
          pos,
        };
        // 官方 flags.js:31：memory 是懒 getter（赋值也允许——补 setter）
        Object.defineProperty(obj, 'memory', {
          configurable: true,
          get: () => flagMemoryFor(o.name),
          set: (v) => { const m = ensureMemory(); if (!m.flags) m.flags = {}; m.flags[o.name] = v; },
        });
        obj.remove = mkStubMethod('flag.remove');
        obj.setPosition = mkStubMethod('flag.setPosition');
        obj.setColor = mkStubMethod('flag.setColor');
        break;
      }
      case 'resource': {
        obj = Object.assign(base, {
          resourceType: o.resourceType || 'energy',
          amount: o.amount || 0,
        });
        break;
      }
      default: {
        obj = base;
      }
    }
    const cls = classGlobalFor(o);
    if (cls && globalThis[cls] && globalThis[cls].prototype) {
      // 官方语义：Game 对象是类的实例；bot 生态普遍给官方原型打补丁
      //（tooangel 的 prototype_*.js 全系），原型链接通后补丁可见、instanceof 成立
      Object.setPrototypeOf(obj, globalThis[cls].prototype);
    }
    const name = o.name ? ':' + o.name : '';
    return probe(obj, (cls || o.type) + name);
  }

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

  function flagMemoryFor(name) {
    const m = ensureMemory();
    if (!m.flags) m.flags = {};
    if (!m.flags[name]) m.flags[name] = {};
    return m.flags[name];
  }

  function buildGame() {
    objectsById = new Map();
    const creeps = {};
    const spawns = {};
    const structures = {};
    const constructionSites = {};
    const flags = {};
    const resources = {};
    for (const o of world.objects || []) {
      const w = wrapObject(o, world.time);
      w.room = undefined;
      objectsById.set(o.id, w);
      const mine = o.owner === viewerName;
      if (o.type === 'creep') {
        // 官方 game.js:273-280：Game.creeps 只收本人 creep
        if (mine) creeps[o.name] = w;
      } else if (o.type === 'spawn') {
        if (mine) spawns[o.name] = w; // game.js:313-318
      } else if (o.type === 'flag') flags[o.name] = w;
      else if (o.type === 'constructionSite') {
        if (mine) constructionSites[o.id] = w; // game.js:366-372
      } else if (o.type === 'resource') resources[o.id] = w;
      else if (o.type === 'structure') {
        // 官方 game.structures 只收自家 owned 结构（game.js:302-307
        /// OwnedStructure + userObjects）；source/mineral/road/wall 不入
        if (mine && o.owner !== undefined) structures[o.id] = w;
      }
    }
    // 可见房（官方 game.js:218-220 只注册 runtimeData.rooms）：查看者有对象
    /// 的房间；world.rooms 全量携带地图级地形（Game.map 面，不进 Game.rooms）
    const visibleRooms = new Set(
      (world.objects || []).filter((o) => o.owner === viewerName).map((o) => o.room),
    );
    const rooms = {};
    for (const [name, r] of Object.entries(world.rooms || {})) {
      if (viewerName !== null && !visibleRooms.has(name)) continue;
      // 无 controller 房间的官方投影是 undefined（非 null）
      let controller;
      for (const o of world.objects || []) {
        if (o.room === name && o.type === 'structure' && o.structureType === 'controller') {
          controller = objectsById.get(o.id);
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
        for (const o of world.objects || []) {
          if (o.room !== name) continue;
          const w = objectsById.get(o.id);
          if (!w) continue;
          const isStruct = o.type === 'structure' || o.type === 'spawn';
          // 官方 FIND_SOURCES_ACTIVE 条件 energy > 0（game.js:23）；初态缺省 3000
          // （与 wrapObject 的 source 投影缺省一致）
          if (type === 101 /* FIND_CREEPS */ && o.type === 'creep') out.push(w);
          else if (type === 102 /* FIND_MY_CREEPS */ && o.type === 'creep' && o.owner === viewerName) out.push(w);
          else if (type === 104 /* FIND_SOURCES_ACTIVE */ && o.type === 'structure' && o.structureType === 'source' && (o.energy ?? 3000) > 0) out.push(w);
          else if (type === 105 /* FIND_SOURCES */ && o.type === 'structure' && o.structureType === 'source') out.push(w);
          else if (type === 106 /* FIND_DROPPED_RESOURCES */ && o.type === 'resource') out.push(w);
          // 官方 FIND_STRUCTURES/FIND_MY_STRUCTURES 不含 source/mineral：
          // game.js:229-251 structureTypes 表无此三类（独立装配分支 :329-345），
          // 与 Rust 侧 matches_find 两层一致
          else if (type === 107 /* FIND_STRUCTURES */ && isStruct && o.structureType !== 'source' && o.structureType !== 'mineral') out.push(w);
          else if (type === 108 /* FIND_MY_STRUCTURES */ && isStruct && o.structureType !== 'source' && o.structureType !== 'mineral' && o.owner === viewerName) out.push(w);
          else if (type === 110 /* FIND_FLAGS */ && o.type === 'flag') out.push(w);
          else if (type === 111 /* FIND_CONSTRUCTION_SITES */ && o.type === 'constructionSite') out.push(w);
          else if (type === 112 /* FIND_MY_SPAWNS */ && o.type === 'spawn' && o.owner === viewerName) out.push(w);
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
      room.getPositionAt = (x, y) => new RoomPosition(x, y, name);
      room.serializePath = () => '';
      room.deserializePath = () => [];
      Object.defineProperty(room, 'memory', {
        configurable: true,
        get() {
          const m = ensureMemory();
          if (!m.rooms) m.rooms = {};
          if (!m.rooms[name]) m.rooms[name] = {};
          return m.rooms[name];
        },
        // 官方 rooms.js:551 同时定义 setter——tooangel 等 bot 直接对 room.memory 赋值
        set(v) {
          const m = ensureMemory();
          if (!m.rooms) m.rooms = {};
          m.rooms[name] = v;
        },
      });
      if (globalThis.Room && globalThis.Room.prototype) {
        Object.setPrototypeOf(room, globalThis.Room.prototype);
      }
      rooms[name] = probe(room, 'Room[' + name + ']');
    }
    // 回填对象上的 room 引用
    for (const [id, w] of objectsById) {
      const orig = (world.objects || []).find((x) => x.id === id);
      if (orig && rooms[orig.room]) w.room = rooms[orig.room];
    }

    const cpu = world.cpu || { limit: 20, tickLimit: 500, bucket: 10000 };
    const game = {
      time: world.time,
      shard: { name: 'shard0', type: 'normal', ptr: false },
      cpu: {
        limit: cpu.limit,
        tickLimit: cpu.tickLimit,
        bucket: cpu.bucket,
        shardLimits: { shard0: cpu.limit },
        // 官方口径：真实耗时 + intents.cpu（虚拟计费，runtime.js:202-204）
        getUsed: () => ops.op_kk_get_used() + (intents ? intents.cpu : 0),
        getHeapStatistics: () => { countStub('Game.cpu.getHeapStatistics'); return { total_heap_size: 0, used_heap_size: 0, heap_size_limit: 0, externally_allocated_size: 0, physical_total: 0, malloced_memory: 0, peak_malloced_memory: 0, does_zap_garbage: 0, number_of_native_contexts: 1, number_of_detached_contexts: 0 }; },
        halt: () => { countStub('Game.cpu.halt'); },
        setShardLimits: () => 0,
        generatePixel: () => { countStub('Game.cpu.generatePixel'); return -9; },
        unlock: () => -4,
      },
      gcl: { level: 1, progress: 0, progressTotal: 1000000 },
      gpl: { level: 0, progress: 0, progressTotal: 1000 },
      creeps,
      spawns,
      structures,
      constructionSites,
      flags,
      resources,
      rooms,
      market: {
        credits: 0,
        getAllOrders: () => { countStub('Game.market.getAllOrders'); return []; },
        orders: {},
        incomingTransactions: [],
        outgoingTransactions: [],
        calcTransactionCost: () => 1,
        createOrder: () => -4,
        cancelOrder: () => -4,
        extendOrder: () => -4,
        deal: () => -4,
        getHistory: () => [],
      },
      getObjectById(id) {
        return objectsById.get(id) || null;
      },
      notify() { countStub('Game.notify'); },
      map: {
        getWorldSize: () => 1,
        getRoomLinearDistance: () => 0,
        getRoomStatus: () => ({ status: 'normal', timestamp: null }),
        describeExits: () => ({}),
        findExit: () => -1,
        findRoute: () => [],
        // 官方 map.js:262-264：getRoomTerrain 返回 Room.Terrain 实例
        getRoomTerrain: (roomName) => new Terrain(roomName),
        // 官方 map.js:235-258：返回 'wall'|'swamp'|'plain'（完整词，非缩写）；
        // 越界或无地形数据 → undefined。M0 世界仅在 createShard/setTerrain 提供
        // 地形时可用（bit 语义同官方 TERRAIN_MASK_*）
        getTerrainAt(x, y, roomName) {
          if (x && typeof x === 'object') { y = x.y; roomName = x.roomName; x = x.x; }
          if (x < 0 || x > 49 || y < 0 || y > 49) return undefined;
          const terrain = terrainByRoom[roomName];
          if (!terrain) return undefined;
          const t = terrain[y * 50 + x];
          if (t === undefined || t === null) return undefined;
          if (t & TERRAIN_MASK_WALL) return 'wall';
          if (t & TERRAIN_MASK_SWAMP) return 'swamp';
          return 'plain';
        },
      },
    };
    game.cpu = probe(game.cpu, 'Game.cpu');
    game.market = probe(game.market, 'Game.market');
    game.map = probe(game.map, 'Game.map');
    game.shard = probe(game.shard, 'Game.shard');
    game.gcl = probe(game.gcl, 'Game.gcl');
    game.gpl = probe(game.gpl, 'Game.gpl');
    return probe(game, 'Game');
  }

  // ---------------------------------------------------------------- tick 入口

  let gameInstance = null;
  function refreshGame() {
    gameInstance = buildGame();
    Object.defineProperty(globalThis, 'Game', {
      configurable: true,
      get: () => gameInstance,
    });
  }
  refreshGame();

  // ---------------------------------------------------------------- intents 收集器
  // 官方同形（driver/lib/runtime/runtime.js:60-100）：闭包私有对象，玩家拿不到
  // 引用——伪造/清空/抢跑均不可能（防篡改是约束②的结构性实现）。同名覆盖
  // 只计一次费（:68）、say/pull 免费（:61）、cancelOrder 退费不查免费表（:95，
  // 官方 quirk 原样保留）；list 按对象 id → 动作名组织，room/global 级走
  // 数组通道（M2A-A2 只实装对象级与 room 级，global 随 market 批次）。
  // null 原型 + 自有键（官方 runtime.js:60 同款）：原型污染不可使计费免费化
  const FREE_INTENT_NAMES = Object.create(null);
  FREE_INTENT_NAMES.say = true;
  FREE_INTENT_NAMES.pull = true;
  let intents = null;
  function resetIntents() {
    const list = Object.create(null);
    let cpu = 0;
    intents = {
      list,
      set(id, name, data) {
        list[id] = list[id] || Object.create(null);
        if (!FREE_INTENT_NAMES[name] && !list[id][name]) cpu += 0.2;
        list[id][name] = data;
      },
      pushByName(id, name, data) {
        list[id] = list[id] || Object.create(null);
        list[id][name] = list[id][name] || [];
        list[id][name].push(data);
      },
      remove(id, name) {
        if (list[id] && list[id][name] !== undefined) {
          delete list[id][name];
          cpu -= 0.2;
          return true;
        }
        return false;
      },
      get cpu() { return cpu; },
    };
  }
  resetIntents();
  /** 官方 run 闭包尾等价物：intentsList 原样带出（runtime.js:274-296）。 */
  function intentsPayload() {
    return intents && { list: intents.list, cpu: intents.cpu };
  }

  globalThis.__kkp.memoryWorld = {
    /** 每 tick 注入世界快照（宿主以 JSON 字面量内嵌，此处已是对象）并重建 Game。 */
    beginTick(worldJson) {
      world = typeof worldJson === 'string' ? JSON.parse(worldJson) : worldJson;
      viewerName = world.user ?? null;
      resetIntents(); // 官方每 tick 新建收集器（runtime.js _start 每 tick 执行）
      parsedMemory = undefined; // 官方语义：Memory 每 tick 从 RawMemory 懒解析
      // 上一 tick 首访后 Memory 已固化为普通值属性（官方同款），本 tick 重装懒 getter
      Object.defineProperty(globalThis, 'Memory', {
        configurable: true,
        get() { return ensureMemory(); },
      });
      terrainByRoom = {};
      for (const [n, r] of Object.entries(world.rooms || {})) {
        if (Array.isArray(r.terrain)) terrainByRoom[n] = r.terrain;
      }
      refreshGame();
    },
    writeBackMemory,
    intentsPayload,
  };
})();
