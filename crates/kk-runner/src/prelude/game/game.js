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
  const __mapForEach = Map.prototype.forEach;
  const __setAdd = Set.prototype.add;
  const __setHas = Set.prototype.has;
  const __reflectApply = Reflect.apply;
  const __objectEntries = Object.entries;
  const __objectKeys = Object.keys;
  const __dateNow = Date.now;
  const __isArray = Array.isArray;
  const __numberIsInteger = Number.isInteger;
  const mathPow = Math.pow;
  const mathFloor = Math.floor;
  const cpuHalt = globalThis.__kkp.cpuHalt;
  const C_ERR_FULL = globalThis.ERR_FULL;
  const C_ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const C_OK = globalThis.OK;
  const GCL_MULTIPLY = globalThis.GCL_MULTIPLY;
  const GCL_POW = globalThis.GCL_POW;
  const POWER_LEVEL_MULTIPLY = globalThis.POWER_LEVEL_MULTIPLY;
  const POWER_LEVEL_POW = globalThis.POWER_LEVEL_POW;
  // ---------------------------------------------------------------- Game 装配

  const { probe, makeMarket, makeMap, createWorldTick } = globalThis.__kkp;
  const { resetIntents } = globalThis.__kkp.intentCollector;

  // Game.gpl（官方 game.js:164-167）：level=floor((power/1000)^(1/2))，
  // base=level^2*1000；Math.pow 精确浮点与官方逐位一致。
  function buildGpl() {
    const power =
      gw.world.power_account && typeof gw.world.power_account.power === 'number'
        ? gw.world.power_account.power
        : 0;
    const level = mathFloor(mathPow(power / POWER_LEVEL_MULTIPLY, 1 / POWER_LEVEL_POW));
    const base = mathPow(level, POWER_LEVEL_POW) * POWER_LEVEL_MULTIPLY;
    return {
      level,
      progress: power - base,
      progressTotal: mathPow(level + 1, POWER_LEVEL_POW) * POWER_LEVEL_MULTIPLY - base,
    };
  }

  // Game.powerCreeps：账号注册表按名装配；已生成侧的房间包装实例优先
  //（objectsById 同 ID），未生成侧 new PowerCreep(id)（构造器对 rawById
  // 缺席安全——无 room/pos，方法走 BUSY 分支）。
  function buildPowerCreeps() {
    const out = {};
    const account = gw.world.power_account;
    const records = account && account.powerCreeps;
    if (!records) return out;
    for (const id in records) {
      const wrapped = __reflectApply(__mapGet, gw.objectsById, [id]);
      const inst = wrapped || new globalThis.PowerCreep(id);
      // 未生成侧不进 objectsById：官方 game.js:255-258 只入 register.
      // powerCreeps（名字索引表），register._objects（getObjectById 源，
      // game.js:286-292）仅含房间对象——死亡的 pc 回未生成态后
      // getObjectById 为 null（NUKE-IMPACT-008 契约）。
      out[records[id].name] = inst;
    }
    return out;
  }

  function buildGame() {
    // 诊断子段（KKDIAG）：装配主环 / 房间与查询构造 / 回填清扫。
    const diagOn = gw.runDiag === true;
    const dWrap0 = diagOn ? __dateNow() : 0;
    gw.objectsById = new __Map();
    const creeps = {};
    const spawns = {};
    const structures = {};
    const constructionSites = {};
    const flags = {};
    const resources = {};
    const worldObjs = gw.world.objects || [];
    // 单次分桶：按房收集 {os, ws} 平行数组，供 makeRoomQueries 与 room 回填
    // 直接消费——省去逐可见房全量扫描与回填阶段的 objectsById 逐对象回查。
    // 尾部索引赋值不走 push 方法分派（玩家可替换数组原型）。
    const roomBuckets = new __Map();
    for (let i = 0; i < worldObjs.length; i++) {
      const o = worldObjs[i];
      // wrapper 常驻：id 存续且类型不变则复用包装（自有数据面已刷新），
      // 数据字段仍经 rawById 按拍回查。
      const w = gw.wrapObjectResident(o, gw.world.time);
      w.room = undefined;
      __reflectApply(__mapSet, gw.objectsById, [o.id, w]);
      let bucket = __reflectApply(__mapGet, roomBuckets, [o.room]);
      if (bucket === undefined) {
        bucket = { os: [], ws: [] };
        __reflectApply(__mapSet, roomBuckets, [o.room, bucket]);
      }
      bucket.os[bucket.os.length] = o;
      bucket.ws[bucket.ws.length] = w;
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
        if (
          own === gw.viewerName &&
          o.type !== 'constructionSite' &&
          o.type !== 'flag' &&
          o.type !== 'mod'
        ) {
          __reflectApply(__setAdd, visibleRooms, [o.room]);
        }
      }
    }
    const dRooms0 = diagOn ? __dateNow() : 0;
    const dWrap = dRooms0 - dWrap0;
    const rooms = {};
    const roomEntries = __objectEntries(gw.world.rooms || {});
    for (let i = 0; i < roomEntries.length; i++) {
      const name = roomEntries[i][0];
      if (gw.viewerName !== null && !__reflectApply(__setHas, visibleRooms, [name])) continue;
      rooms[name] = gw.makeRoom(
        name,
        roomEntries[i][1],
        __reflectApply(__mapGet, roomBuckets, [name]),
      );
    }
    const dTail0 = diagOn ? __dateNow() : 0;
    const dRooms = dTail0 - dRooms0;
    // 回填 room：直接遍历分桶的包装数组，不再全量扫描世界与逐对象 Map 回查。
    // 未建房间（rooms 无键）回填 undefined，与旧版「保持主循环重置值」等价。
    __reflectApply(__mapForEach, roomBuckets, [
      (bucket, bucketRoom) => {
        const room = rooms[bucketRoom];
        const ws = bucket.ws;
        for (let i = 0; i < ws.length; i++) ws[i].room = room;
      },
    ]);
    // 常驻表清扫：消亡对象出表（rawById 已是本拍权威集合）。
    gw.sweepResidentWrappers();
    if (diagOn) {
      gw.__kkdiagBuild = { wrap: dWrap, rooms: dRooms, tail: __dateNow() - dTail0 };
    }

    const cpu = gw.world.cpu || {
      limit: 20,
      tickLimit: 500,
      bucket: 10000,
      // 裸 run（无宿主 cpu 注入）的堆面兜底：全零形态（套件路径恒经
      // begin_tick 注入真值——sandbox 侧采样）
      heap: {
        total_heap_size: 0,
        total_heap_size_executable: 0,
        total_physical_size: 0,
        total_available_size: 0,
        used_heap_size: 0,
        heap_size_limit: 0,
        malloced_memory: 0,
        peak_malloced_memory: 0,
        does_zap_garbage: 0,
        externally_allocated_size: 0,
      },
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
      // 官方 game.js：由 userPowerCreeps 装配（本人账号记录；已生成侧复用
      // objectsById 的房间包装实例，未生成侧直接构造——原型 getter 读
      // 账号+房间合并面）。power_account 缺省（无注入）时恒为空对象。
      powerCreeps: buildPowerCreeps(),
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
        // 官方后端专有 API（OSS 引擎无实现），语义按套件契约：每日 CPU
        // 配额按 shard 分配。单 shard 世界读面恒 {shard0: limit}（与
        // Game.shard.name 同源常量）；setShardLimits 校验键集/非负整数/
        // 总和恒等，唯一合法分配即现状——成功为 no-op 返回 OK（分配
        // 持久化随 multiShard 簇接线，12h 冷却套件既缓未接）。
        get shardLimits() {
          return { shard0: cpu.limit };
        },
        setShardLimits(limits) {
          if (limits === null || typeof limits !== 'object' || __isArray(limits)) {
            return C_ERR_INVALID_ARGS;
          }
          let sum = 0;
          let hasShard0 = false;
          for (const name of __objectKeys(limits)) {
            const v = limits[name];
            if (name !== 'shard0') return C_ERR_INVALID_ARGS;
            if (typeof v !== 'number' || !__numberIsInteger(v) || v < 0) {
              return C_ERR_INVALID_ARGS;
            }
            hasShard0 = true;
            sum += v;
          }
          if (!hasShard0 || sum !== cpu.limit) return C_ERR_INVALID_ARGS;
          return 0;
        },
        generatePixel: () => {
          countStub('Game.cpu.generatePixel');
          return -9;
        },
        unlock: () => -4,
      },
      gcl: {
        level: gclLevel,
        progress: gclPoints - gclBaseProgress,
        progressTotal: mathPow(gclLevel, GCL_POW) * GCL_MULTIPLY - gclBaseProgress,
      },
      gpl: buildGpl(),
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
