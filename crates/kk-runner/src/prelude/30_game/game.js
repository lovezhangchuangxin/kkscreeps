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

  // 预置 JSON 函数（官方 systemFunctions.JSON 同款）：Memory 写回等宿主
  // 关键序列化不经玩家可覆写的全局（外部复审 P1 修复面）
  const __jsonStringify = JSON.stringify;

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
  gw.ensureMemory = ensureMemory;
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
      (gw.world.objects || []).filter((o) => o.owner === gw.viewerName).map((o) => o.room),
    );
    const rooms = {};
    for (const [name, r] of Object.entries(gw.world.rooms || {})) {
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
  function resetIntents() {
    const list = Object.create(null);
    let cpu = 0;
    gw.intents = {
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
    return gw.intents && { list: gw.intents.list, cpu: gw.intents.cpu };
  }

  globalThis.__kkp.memoryWorld = {
    /** 每 tick 注入世界快照（宿主以 JSON 字面量内嵌，此处已是对象）并重建 Game。 */
    beginTick(worldJson) {
      gw.world = typeof worldJson === 'string' ? JSON.parse(worldJson) : worldJson;
      gw.viewerName = gw.world.user ?? null;
      resetIntents(); // 官方每 tick 新建收集器（runtime.js _start 每 tick 执行）
      parsedMemory = undefined; // 官方语义：Memory 每 tick 从 RawMemory 懒解析
      // 上一 tick 首访后 Memory 已固化为普通值属性（官方同款），本 tick 重装懒 getter
      Object.defineProperty(globalThis, 'Memory', {
        configurable: true,
        get() { return ensureMemory(); },
      });
      gw.terrainByRoom = {};
      for (const [n, r] of Object.entries(gw.world.rooms || {})) {
        if (Array.isArray(r.terrain)) gw.terrainByRoom[n] = r.terrain;
      }
      refreshGame();
    },
    writeBackMemory,
    intentsPayload,
  };
})();
