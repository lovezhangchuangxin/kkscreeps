/* kk-runner prelude · 30_game 段 9：Game 装配与 tick 生命周期（对照官方
 * engine/src/game/game.js + driver/lib/runtime/runtime.js 的收集器）。
 *
 * RawMemory/Memory（官方 game.js:470-495 懒解析语义）、intents 收集器
 * （官方 runtime.js:60-100 同形闭包私有）、buildGame（Game 装配序，官方
 * game.js:273-345 collections）、refreshGame 与 __kkp.memoryWorld 导出
 * （beginTick/writeBackMemory/intentsPayload——90_host 宿主通道消费）。
 * 依赖：gw 全容器（_registry）、makeRoom/Terrain/terrainWordAt（room 段）、
 * wrapObject（_registry 段）、各类型 assembler（已注册）。
 */
(() => {
  'use strict';

  const { ops, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 预置 JSON 函数（官方 systemFunctions.JSON 同款）：Memory 写回/世界解码等
  // 宿主关键序列化不经玩家可覆写的全局（外部复审 P1 修复面；自审 P2 补 parse）
  const __jsonStringify = JSON.stringify;
  const __jsonParse = JSON.parse;
  // 预置 Object（官方 systemFunctions.Object 同位，runtime.js 头部捕获）：
  // 宿主路径的对象构造不得经可交换的 globalThis.Object（对抗评审 P1 实证：
  // 交换 Object 使 nullEnv 产出真原型对象，null-proto 防线整体绕过）
  const __Object = Object;
  const __Array = Array;

  // null 原型信封（官方 runtime.js:214 `outMessage = systemFunctions.Object.create(null)`
  // 同款）：宿主消息对象不得携带 Object.prototype——继承的 toJSON 会让预置
  // stringify 在序列化时被原型污染改写载荷（自审 P1：一条 defineProperty 可
  // 完整重放"伪造 intentsList 造工地"原探针；官方靠 null-proto + ExternalCopy
  // 出口双重免疫，我方出口是 stringify，null-proto 是等价结构位之一）
  function nullEnv(props) {
    return __Object.assign(__Object.create(null), props);
  }
  gw.nullEnv = nullEnv;
  gw.__Object = __Object;

  // 出口净化序列化（对抗评审 P1/P2 的根本闸）：序列化窗口内摘除两个根原型的
  // toJSON（含数组——stringify 对数组自身也查 toJSON，pushByName 数组通道同
  // 面潜伏）——官方出口是 ExternalCopy 结构拷贝，任何玩家 toJSON 都不参与
  // 宿主消息（runtime.js:296），摘除=官方等价。save/restore 保持玩家 tick 内
  // 的原型状态不变（玩家的 toJSON 只在自己的序列化场景可见——如 Memory 写回
  // 官方就调用，systemFunctions.JSON.stringify 是原生语义）。净化期间的异常
  // （freeze 原型使 delete 抛错等）一律上抛由出口 catch 归 ok:false
  function sanitizedStringify(v) {
    const OP = __Object.prototype;
    const AP = __Array.prototype;
    const so = OP.toJSON;
    const sa = AP.toJSON;
    // 摘除容错（对抗评审 P1）：configurable:false 的污染使 delete 抛 TypeError
    // ——降级赋 undefined（不可调用即不参与序列化）；freeze 使两者皆抛时上抛，
    // 由出口 catch 的字面量信封兜底（绝不得逃逸为硬中断）
    const lift = (proto) => {
      try { delete proto.toJSON; } catch (_) {
        try { proto.toJSON = undefined; } catch (_) { throw new Error('根原型已冻结'); }
      }
    };
    const restore = (proto, saved) => {
      if (saved === undefined) return;
      try { proto.toJSON = saved; } catch (_) { /* 玩家自见面，尽力恢复 */ }
    };
    try {
      lift(OP);
      lift(AP);
      return __jsonStringify(v);
    } finally {
      restore(OP, so);
      restore(AP, sa);
    }
  }
  gw.sanitizedStringify = sanitizedStringify;

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
    segments: __Object.create(null),
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
    __Object.defineProperty(globalThis, 'Memory', {
      configurable: true,
      enumerable: true,
      value: parsedMemory,
    });
    return parsedMemory;
  }
  gw.ensureMemory = ensureMemory;
  function writeBackMemory() {
    // 返回 {ok} | {ok:false,error}：screeps-ok 契约（adapter-spec:229）把 tick 末
    // 序列化失败归为 runPlayer 的 runtime 错误——不能只在 console 里报完就算成功。
    // 玩家侧写回错误（不可序列化 / Memory.toJSON 返回非字符串等）一律走这里，
    // 绝不抛出——抛出会被宿主误判为硬中断并按整份 tickLimit 计费
    if (parsedMemory === undefined) return nullEnv({ ok: true });
    try {
      const s = __jsonStringify(parsedMemory);
      if (typeof s !== 'string') {
        // Memory.toJSON 返回非字符串时 stringify 得 undefined——玩家侧写回失败
        console.error('Memory 写回失败（toJSON 返回非字符串），保留上一 tick 存档');
        return nullEnv({ ok: false, error: 'Memory.toJSON 返回了非字符串' });
      }
      if (s.length > RAW_MEMORY_LIMIT) {
        console.error('Memory 写回超过 2MB 上限，保留上一 tick 存档');
        return nullEnv({ ok: false, error: 'Memory 写回超过 2MB 上限' });
      }
      rawMemoryData = s;
      return nullEnv({ ok: true });
    } catch (e) {
      // 玩家异常对象的 message 可是污染 getter（抛错逃逸会被宿主误判硬中断）
      let why;
      try { why = String((e && e.message) || e); } catch (_) { why = 'unprintable player error'; }
      console.error('Memory 写回失败（不可序列化），保留上一 tick 存档: ' + why);
      return nullEnv({ ok: false, error: 'Memory 不可序列化: ' + why });
    }
  }
  // Memory 作为懒 getter（官方语义：访问才解析）。官方 game.js:470-473 两处
  // defineProperty 均 enumerable: true（初始 getter 与首访固化后的 value 属性）
  __Object.defineProperty(globalThis, 'Memory', {
    configurable: true,
    enumerable: true,
    get() { return ensureMemory(); },
  });

  // ---------------------------------------------------------------- Game 装配

  const { probe } = globalThis.__kkp;

  function buildGame() {
    gw.objectsById = new Map();
    const creeps = {};
    const spawns = {};
    const structures = {};
    const constructionSites = {};
    const flags = {};
    const resources = {};
    for (const o of gw.world.objects || []) {
      const w = gw.wrapObject(o, gw.world.time);
      w.room = undefined;
      gw.objectsById.set(o.id, w);
      const mine = o.owner === gw.viewerName;
      if (o.type === 'creep') {
        // 官方 game.js:273-280：Game.creeps 只收本人 creep
        if (mine) creeps[o.name] = w;
      } else if (o.type === 'spawn') {
        if (mine) {
          spawns[o.name] = w; // game.js:313-318
          // 官方 spawn 先经 structureTypes 表进 structures、再进 spawns
          //（game.js:298-303 structures 收一切本人 OwnedStructure，spawn 是其
          // 子类 structures.js:854）——两集合同时收录（自审 P3 修复：
          // 原只进 spawns 漏 structures）
          structures[o.id] = w;
        }
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
    // 可见房（官方 game.js:218-220 只注册 runtimeData.rooms）：查看者**非工地
    /// 非旗**自有对象所在房（对齐官方归集跳过表 data.js:113-116——对抗评审
    // P2：自家工地经 world 破例导出进 objects 后，此处若不排除会把工地所在
    // 房带回 Game.rooms，"工地不授视野"在房间存在性层被击穿）；工地全集经
    // Game.constructionSites 收录（官方 data.js:103 + game.js:366-374），两语义正交
    const visibleRooms = new Set(
      (gw.world.objects || []).filter((o) => o.owner === gw.viewerName
        && o.type !== 'constructionSite').map((o) => o.room),
    );
    const rooms = {};
    for (const [name, r] of __Object.entries(gw.world.rooms || {})) {
      if (gw.viewerName !== null && !visibleRooms.has(name)) continue;
      rooms[name] = gw.makeRoom(name, r);
    }
    // 回填对象上的 room 引用
    for (const [id, w] of gw.objectsById) {
      const orig = (gw.world.objects || []).find((x) => x.id === id);
      if (orig && rooms[orig.room]) w.room = rooms[orig.room];
    }

    const cpu = gw.world.cpu || { limit: 20, tickLimit: 500, bucket: 10000 };
    const game = {
      time: gw.world.time,
      shard: { name: 'shard0', type: 'normal', ptr: false },
      cpu: {
        limit: cpu.limit,
        tickLimit: cpu.tickLimit,
        bucket: cpu.bucket,
        shardLimits: { shard0: cpu.limit },
        // 官方口径：真实耗时 + intents.cpu（虚拟计费，runtime.js:202-204）
        getUsed: () => ops.op_kk_get_used() + (gw.intents ? gw.intents.cpu : 0),
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
        return gw.objectsById.get(id) || null;
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
        getRoomTerrain: (roomName) => new (gw.Terrain)(roomName),
        // 官方 map.js:235-258：返回 'wall'|'swamp'|'plain'（完整词，非缩写）；
        // 越界或无地形数据 → undefined。M0 世界仅在 createShard/setTerrain 提供
        // 地形时可用（bit 语义同官方 TERRAIN_MASK_*）
        getTerrainAt(x, y, roomName) {
          if (x && typeof x === 'object') { y = x.y; roomName = x.roomName; x = x.x; }
          if (x < 0 || x > 49 || y < 0 || y > 49) return undefined;
          const terrain = gw.terrainByRoom[roomName];
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
    __Object.defineProperty(globalThis, 'Game', {
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
  const FREE_INTENT_NAMES = __Object.create(null);
  FREE_INTENT_NAMES.say = true;
  FREE_INTENT_NAMES.pull = true;
  function resetIntents() {
    const list = __Object.create(null);
    let cpu = 0;
    gw.intents = {
      list,
      set(id, name, data) {
        list[id] = list[id] || __Object.create(null);
        if (!FREE_INTENT_NAMES[name] && !list[id][name]) cpu += 0.2;
        // egress data 剥原型：官方出口是 ExternalCopy 结构拷贝不经 stringify
        //（runtime.js:296），我方出口是 stringify——继承的 toJSON 会在序列化时
        // 改写载荷，剥原型是等价结构位（官方对注入侧同样剥顶层，:56）。
        // 数组元素经出口净化窗口兜底（sanitizedStringify）
        __Object.setPrototypeOf(data, null);
        list[id][name] = data;
      },
      pushByName(id, name, data) {
        list[id] = list[id] || __Object.create(null);
        __Object.setPrototypeOf(data, null);
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
    return gw.intents && { list: gw.intents.list, cpu: gw.intents.cpu };
  }

  globalThis.__kkp.memoryWorld = {
    /** 每 tick 注入世界快照（宿主以 JSON 字面量内嵌，此处已是对象）并重建 Game。 */
    beginTick(worldJson) {
      // 世界解码走预置 parse（官方 runtime.js:207 对宿主数据用
      // systemFunctions.JSON.parse 同款）：全局 parse 玩家可覆写，可伪造
      // 整份世界含 user → per-viewer 判定翻转（自审 P2）
      gw.world = typeof worldJson === 'string' ? __jsonParse(worldJson) : worldJson;
      // 注入侧剥顶层原型（官方 runtime.js:56 `systemFunctions.Object.setPrototypeOf
      //(data, null)` 同位）：顶层缺失键（cpu/user 兜底读）不被继承 getter 命中；
      // 内层对象官方同样不剥——缺失键走原型链的暴露面官方一致，不深剥
      __Object.setPrototypeOf(gw.world, null);
      gw.viewerName = gw.world.user ?? null;
      resetIntents(); // 官方每 tick 新建收集器（runtime.js _start 每 tick 执行）
      parsedMemory = undefined; // 官方语义：Memory 每 tick 从 RawMemory 懒解析
      // 上一 tick 首访后 Memory 已固化为普通值属性（官方同款），本 tick 重装懒 getter
      __Object.defineProperty(globalThis, 'Memory', {
        configurable: true,
        get() { return ensureMemory(); },
      });
      gw.terrainByRoom = {};
      for (const [n, r] of __Object.entries(gw.world.rooms || {})) {
        if (Array.isArray(r.terrain)) gw.terrainByRoom[n] = r.terrain;
      }
      refreshGame();
    },
    writeBackMemory,
    intentsPayload,
  };
})();
