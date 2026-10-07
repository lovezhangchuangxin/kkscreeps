/* 装配每 tick 的 Game 与当前实例；地图、指令及世界注入各自持有职责。
 * 加载期连接闭包桥；Rust 从 memoryWorld 捕获宿主入口。
 */
(() => {
  'use strict';

  const { ops, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 装配使用加载期捕获的内建函数，避免玩家补丁改变对象索引。
  const __Map = Map;
  const __Set = Set;
  const __mapGet = Map.prototype.get;
  const __mapSet = Map.prototype.set;
  const __setAdd = Set.prototype.add;
  const __setHas = Set.prototype.has;
  const __reflectApply = Reflect.apply;
  const __objectEntries = Object.entries;
  const mathPow = Math.pow;
  const mathFloor = Math.floor;
  const cpuHalt = globalThis.__kkp.cpuHalt;
  const C_ERR_FULL = globalThis.ERR_FULL;
  const C_OK = globalThis.OK;
  const GCL_MULTIPLY = globalThis.GCL_MULTIPLY;
  const GCL_POW = globalThis.GCL_POW;
  // ---------------------------------------------------------------- Game 装配

  const { probe, makeMarket, makeMap, createWorldTick } = globalThis.__kkp;
  const { resetIntents } = globalThis.__kkp.intentCollector;

  function buildGame() {
    gw.objectsById = new __Map();
    const creeps = {};
    const spawns = {};
    const structures = {};
    const constructionSites = {};
    const flags = {};
    const resources = {};
    const worldObjs = gw.world.objects || [];
    for (let i = 0; i < worldObjs.length; i++) {
      const o = worldObjs[i];
      const w = gw.wrapObject(o, gw.world.time);
      w.room = undefined;
      __reflectApply(__mapSet, gw.objectsById, [o.id, w]);
      const mine = o.owner === gw.viewerName;
      if (o.type === 'creep') {
        // 官方 game.js:273-280：Game.creeps 只收本人 creep
        if (mine) creeps[o.name] = w;
      } else if (o.type === 'spawn') {
        if (mine) {
          spawns[o.name] = w; // game.js:313-318
          // 本人的 spawn 同时属于 Game.spawns 与 Game.structures。
          // 来源：上游 engine/src/game/game.js 的 spawn 对象注册。
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
    // 优先使用 Rust 注入的 visibleRooms。手写世界未提供它时，从 owner/user 所属对象推断。
    // 旗帜与工地不授予房间视野；自家工地仍可单独出现在 Game.constructionSites。
    const visibleRooms = new __Set();
    const visibleList = gw.world.visibleRooms;
    if (visibleList !== undefined && visibleList !== null) {
      for (let i = 0; i < visibleList.length; i++) {
        __reflectApply(__setAdd, visibleRooms, [visibleList[i]]);
      }
    } else {
      for (let i = 0; i < worldObjs.length; i++) {
        const o = worldObjs[i];
        // 旗帜与工地不授予房间视野，即使带有本人的 owner；Mod 对象同样
        // 不授予（Rust 侧 room_visible_to 同口径——权威可见房恒随
        // visibleRooms 注入，此回退只服务手搓世界）。
        // 来源：上游 driver/lib/runtime/data.js 的房间视野归集。
        const own = o.owner !== undefined ? o.owner : o.user;
        if (own === gw.viewerName && o.type !== 'constructionSite' && o.type !== 'flag'
          && o.type !== 'mod') {
          __reflectApply(__setAdd, visibleRooms, [o.room]);
        }
      }
    }
    const rooms = {};
    const roomEntries = __objectEntries(gw.world.rooms || {});
    for (let i = 0; i < roomEntries.length; i++) {
      const name = roomEntries[i][0];
      if (gw.viewerName !== null && !__reflectApply(__setHas, visibleRooms, [name])) continue;
      rooms[name] = gw.makeRoom(name, roomEntries[i][1]);
    }
    // 从已建立的对象索引回填 room，避免为每个对象重新扫描世界数组。
    for (let i = 0; i < worldObjs.length; i++) {
      const o = worldObjs[i];
      const w = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      if (w && rooms[o.room]) w.room = rooms[o.room];
    }

    const cpu = gw.world.cpu || {
      limit: 20, tickLimit: 500, bucket: 10000,
      // 裸 run（无宿主 cpu 注入）的堆面兜底：全零形态（套件路径恒经
      // begin_tick 注入真值——sandbox 侧采样）
      heap: { total_heap_size: 0, total_heap_size_executable: 0, total_physical_size: 0,
        total_available_size: 0, used_heap_size: 0, heap_size_limit: 0,
        malloced_memory: 0, peak_malloced_memory: 0, does_zap_garbage: 0,
        externally_allocated_size: 0 },
    };
    // Game.gcl 真值（官方 game.js:130-163 点数正变换）：world.gcl = 官方
    // runtimeData.user.gcl 点数（PlayerState 投影）；缺省 0 点 = 1 级满配
    // progressTotal（Math.pow 精确浮点，与官方逐位一致——勿取整）
    const gclPoints = typeof gw.world.gcl === 'number' ? gw.world.gcl : 0;
    const gclLevel = mathFloor(mathPow(gclPoints / GCL_MULTIPLY, 1 / GCL_POW)) + 1;
    const gclBaseProgress = mathPow(gclLevel - 1, GCL_POW) * GCL_MULTIPLY;
    const game = {
      time: gw.world.time,
      shard: { name: 'shard0', type: 'normal', ptr: false },
      // 官方 game.js:91/138：powerCreeps 恒为对象（power 体系未启用时为空）
      powerCreeps: {},
      // 官方 game.js:145：cpuLimit = runtimeData.cpu（tick 窗口同值）
      cpuLimit: cpu.tickLimit,
      cpu: {
        limit: cpu.limit,
        tickLimit: cpu.tickLimit,
        bucket: cpu.bucket,
        // 官方口径：真实耗时 + intents.cpu（虚拟计费，runtime.js:202-204）
        getUsed: () => ops.op_kk_get_used() + (gw.intents ? gw.intents.cpu : 0),
        // 返回 begin_tick 采样的堆统计浅拷贝，不是调用时的实时统计。
        // externally_allocated_size 未接入外部分配统计，固定为 0。
        getHeapStatistics: () => ({ ...cpu.heap }),
        // 主动 halt 请求 V8 中断；保留沙箱且本 tick 不计费，写回与指令丢弃。
        halt: cpuHalt,
        setShardLimits: () => 0,
        generatePixel: () => { countStub('Game.cpu.generatePixel'); return -9; },
        unlock: () => -4,
      },
      gcl: {
        level: gclLevel,
        progress: gclPoints - gclBaseProgress,
        progressTotal: mathPow(gclLevel, GCL_POW) * GCL_MULTIPLY - gclBaseProgress,
      },
      gpl: { level: 0, progress: 0, progressTotal: 1000 },
      creeps,
      spawns,
      structures,
      constructionSites,
      flags,
      resources,
      rooms,
      market: makeMarket(),
      getObjectById(id) {
        return __reflectApply(__mapGet, gw.objectsById, [id]) || null;
      },
      // 每 tick 最多 20 条通知，满时返回 ERR_FULL。结算后存入 PlayerState 的内存通知记录。
      // 来源：上游 engine/src/game/game.js 的 notify。
      notify(message, groupInterval) {
        if (gw.intents.push('notify', { message, groupInterval }, 20)) {
          return C_OK;
        }
        return C_ERR_FULL;
      },
      map: makeMap(),
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
    // 官方 game.js:502 `globals.Game = makeGameObject(...)`：每 tick 平赋值——
    // 可写数据属性，玩家 tick 内覆盖自担、下 tick 重装（Game 可写性跟官方，
    // UNDOC-GLOBAL-001 族）。内部 API 路径仍走 gw.currentGame() 闭包通道
    globalThis.Game = gameInstance;
  }
  refreshGame();
  // 官方 API 内部经闭包 globals 对象读 Game/Memory（structures.js:1073
  // `globals.Game.creeps[name]` 等）——玩家重定义 globalThis.Game 不影响官方
  // 路径。等价物：gw.currentGame() 返回当前 tick 的 Game 实例（闭包引用）。
  gw.currentGame = () => gameInstance;

  resetIntents();

  globalThis.__kkp.memoryWorld = createWorldTick(refreshGame);
})();
