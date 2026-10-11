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
  const __mapDelete = Map.prototype.delete;
  const __mapForEach = Map.prototype.forEach;
  const __setAdd = Set.prototype.add;
  const __setHas = Set.prototype.has;
  const __setForEach = Set.prototype.forEach;
  const __arrayIncludes = Array.prototype.includes;
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
  // ---------------------------------------------------------------- 装配常驻（P2 增量读面）
  // 跨 tick 持久：对象索引、按房分桶（身份恒定、原地重填）、分类表与
  // 可见房列表。KKPV 通道 96% 行干净时，主环只处理解码器打戳的变更行
  // （新/脏/重建），干净行零工作；回退轮（跳拍/重试/块数变动）解码器
  // 自然全量打戳，装配自动退化为全量路径。JSON/手搓世界走全量并同步
  // 维护本状态（记录对象每拍新建 → meta.rec 失配 → 下拍全量重建自愈）。
  const asm = {
    ready: false,
    modChecked: false,
    modBlocked: undefined,
    objectsById: new __Map(),
    buckets: new __Map(),
    collections: null,
    visibleList: null,
  };

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

  // 分类行的官方判定（game.js:273-280/302-318/366-372）：creeps 只收本人、
  // spawn 双表、structure 认 owned、flag/resource 全收、sites 只收本人。
  function classifyRow(o, w, coll, viewer) {
    const mine = o.owner === viewer;
    if (o.type === 'creep') {
      if (mine) coll.creeps[o.name] = w;
    } else if (o.type === 'spawn') {
      if (mine) {
        coll.spawns[o.name] = w;
        // 本人的 spawn 同时属于 Game.spawns 与 Game.structures。
        // 来源：上游 engine/src/game/game.js 的 spawn 对象注册。
        coll.structures[o.id] = w;
      }
    } else if (o.type === 'flag') coll.flags[o.name] = w;
    else if (o.type === 'constructionSite') {
      if (mine) coll.constructionSites[o.id] = w;
    } else if (o.type === 'resource') coll.resources[o.id] = w;
    else if (o.type === 'structure') {
      // 官方 game.structures 只收自家 owned 结构（game.js:302-307
      /// OwnedStructure + userObjects）；source/mineral/road/wall 不入
      if (mine && o.owner !== undefined) coll.structures[o.id] = w;
    }
  }

  function buildGame() {
    // 诊断子段（KKDIAG）：装配主环 / 房间与查询构造 / 回填清扫。
    const diagOn = gw.runDiag === true;
    const dWrap0 = diagOn ? __dateNow() : 0;
    const worldObjs = gw.world.objects || [];
    const tick = gw.world.time;
    const viewer = gw.viewerName;
    // mod 玩家面一经装载即固定（manifest 装载期注入），首见时缓存判定；
    // 有 Mod 类型时禁用增量路径（Mod 对象包装非常驻，保守全量）。
    if (gw.modApi !== undefined && !asm.modChecked) {
      asm.modChecked = true;
      asm.modBlocked = __objectKeys(gw.modApi.types).length > 0;
    }
    const dec = gw.kkpv.lastDecode;
    const fast =
      asm.ready &&
      asm.collections !== null &&
      asm.modBlocked !== true &&
      dec !== undefined &&
      dec.tick === tick &&
      gw.world.visibleRooms !== undefined &&
      gw.world.visibleRooms !== null;
    const CHANGED = gw.kkpv.CHANGED;
    // 桶整建房（成员变动：新/死/跨房/记录重建/回退轮）；回填房（桶整建
    // ∪ 可见性变动——w.room 重指）；值变动行（同房就地：格位迁移）。
    const rebuildRooms = new __Set();
    const backfillRooms = new __Set();
    const valueMoves = [];
    let classDirty = !fast;
    let rebuildBuckets = !fast;
    if (!fast) {
      gw.objectsById = new __Map();
      asm.objectsById = gw.objectsById;
      __reflectApply(__mapForEach, asm.buckets, [
        (bucket) => {
          bucket.os.length = 0;
          bucket.ws.length = 0;
        },
      ]);
    } else {
      gw.objectsById = asm.objectsById;
    }

    // 变更行处理：常驻元数据在包装刷新时被就地覆写，旧值（位置/归属/
    // 名称）必须先捕获再差分。新对象/记录重建/跨房/包装换新都走「房整建」
    // 路径——桶重填自动按世界序入桶、换条目或摘旧包装。
    function touchRow(o) {
      const prevMeta = gw.residentMetaOf(o.id);
      if (prevMeta === undefined) {
        classDirty = true;
        rebuildBuckets = true;
        __reflectApply(__setAdd, rebuildRooms, [o.room]);
        __reflectApply(__mapSet, gw.objectsById, [o.id, gw.wrapObjectResident(o, tick)]);
        return;
      }
      const prevW = __reflectApply(__mapGet, gw.objectsById, [o.id]);
      if (prevW === undefined) {
        // 元数据在而索引缺项（防御，正常不出现）：按新对象处理。
        classDirty = true;
        rebuildBuckets = true;
        __reflectApply(__setAdd, rebuildRooms, [o.room]);
        __reflectApply(__mapSet, gw.objectsById, [o.id, gw.wrapObjectResident(o, tick)]);
        return;
      }
      const ox = prevMeta.x;
      const oy = prevMeta.y;
      const oOwner = prevMeta.owner;
      const oName = prevMeta.name;
      if (prevMeta.rec !== o || prevMeta.room !== o.room) {
        // 记录对象重建（签名不符）/ 跨房移动：新旧房都整建。
        classDirty = true;
        rebuildBuckets = true;
        __reflectApply(__setAdd, rebuildRooms, [prevMeta.room]);
        __reflectApply(__setAdd, rebuildRooms, [o.room]);
        __reflectApply(__mapSet, gw.objectsById, [o.id, gw.wrapObjectResident(o, tick)]);
        return;
      }
      const w = gw.wrapObjectResident(o, tick);
      __reflectApply(__mapSet, gw.objectsById, [o.id, w]);
      if (w !== prevW) {
        // 包装换新（类型/结构类型变更）：房整建换新包装。
        classDirty = true;
        rebuildBuckets = true;
        __reflectApply(__setAdd, rebuildRooms, [o.room]);
        return;
      }
      // 同房就地变更：分类键或格位可能变化。
      if (oOwner !== o.owner || oName !== o.name) classDirty = true;
      if (ox !== o.x || oy !== o.y) {
        valueMoves[valueMoves.length] = { room: o.room, ox, oy, o, w };
      }
    }

    if (fast) {
      for (let i = 0; i < worldObjs.length; i++) {
        const o = worldObjs[i];
        if (o[CHANGED] !== tick) {
          // 干净行：数据未变，只做 `_` 前缀记忆的逐拍清空。
          gw.tickResidentMemo(o);
          continue;
        }
        touchRow(o);
      }
      // 消亡行：解码器 deadIds（归并与清扫并集）驱动精准出表。
      const deadIds = dec.deadIds;
      for (let i = 0; i < deadIds.length; i++) {
        const id = deadIds[i];
        const meta = gw.residentMetaOf(id);
        if (meta !== undefined) {
          classDirty = true;
          rebuildBuckets = true;
          __reflectApply(__setAdd, rebuildRooms, [meta.room]);
          gw.dropResidentWrapper(id);
        }
        __reflectApply(__mapDelete, gw.objectsById, [id]);
      }
    } else {
      for (let i = 0; i < worldObjs.length; i++) touchRow(worldObjs[i]);
    }

    // 可见房集：与上一拍逐元素相等时免全信封遍历（visibleList 序 == 信封
    // rooms 键序：Rust 侧 BTreeSet 与 BTreeMap 同为字典序）。
    const visibleList = gw.world.visibleRooms;
    let sameVisible = false;
    if (fast && asm.visibleList !== null && asm.visibleList.length === visibleList.length) {
      sameVisible = true;
      for (let i = 0; i < visibleList.length; i++) {
        if (asm.visibleList[i] !== visibleList[i]) {
          sameVisible = false;
          break;
        }
      }
    }
    let visibleSet = null;
    if (!sameVisible) {
      visibleSet = new __Set();
      if (visibleList !== undefined && visibleList !== null) {
        for (let i = 0; i < visibleList.length; i++) {
          __reflectApply(__setAdd, visibleSet, [visibleList[i]]);
        }
      } else {
        // 手搓世界回退：按 owner/user 归属推断（旗帜/工地/Mod 不授予视野，
        // 与 Rust 侧 room_visible_to 同口径）。来源：上游 driver/lib/
        // runtime/data.js 的房间视野归集。
        for (let i = 0; i < worldObjs.length; i++) {
          const o = worldObjs[i];
          const own = o.owner !== undefined ? o.owner : o.user;
          if (
            own === viewer &&
            o.type !== 'constructionSite' &&
            o.type !== 'flag' &&
            o.type !== 'mod'
          ) {
            __reflectApply(__setAdd, visibleSet, [o.room]);
          }
        }
      }
      // 可见性变动房：桶回填（w.room 指向/摘除）。新旧集都小（可见房
      // 数量级个位到十位），includes 线性查即可。
      if (asm.visibleList !== null && visibleList !== undefined && visibleList !== null) {
        for (let i = 0; i < asm.visibleList.length; i++) {
          if (!__reflectApply(__setHas, visibleSet, [asm.visibleList[i]])) {
            __reflectApply(__setAdd, backfillRooms, [asm.visibleList[i]]);
          }
        }
        for (let i = 0; i < visibleList.length; i++) {
          if (!__reflectApply(__arrayIncludes, asm.visibleList, [visibleList[i]])) {
            __reflectApply(__setAdd, backfillRooms, [visibleList[i]]);
          }
        }
      }
    }

    // 桶整建与分类重建（单遍世界扫描完成两者）：桶只重填整建房（原地
    // length=0 后按世界序追加，数组身份恒定供查询/创建/寻路闭包捕获）；
    // 分类在 classDirty 时全量重建（保官方扫描序键序），否则逐拍浅拷贝。
    const dRooms0 = diagOn ? __dateNow() : 0;
    const dWrap = dRooms0 - dWrap0;
    let coll;
    if (classDirty) {
      coll = {
        creeps: {},
        spawns: {},
        structures: {},
        constructionSites: {},
        flags: {},
        resources: {},
      };
    } else {
      coll = asm.collections;
    }
    if (rebuildBuckets) {
      __reflectApply(__setForEach, rebuildRooms, [
        (room) => {
          const bucket = __reflectApply(__mapGet, asm.buckets, [room]);
          if (bucket !== undefined) {
            bucket.os.length = 0;
            bucket.ws.length = 0;
          }
        },
      ]);
    }
    if (rebuildBuckets || classDirty) {
      for (let i = 0; i < worldObjs.length; i++) {
        const o = worldObjs[i];
        if (rebuildBuckets && __reflectApply(__setHas, rebuildRooms, [o.room])) {
          let bucket = __reflectApply(__mapGet, asm.buckets, [o.room]);
          if (bucket === undefined) {
            bucket = { os: [], ws: [] };
            __reflectApply(__mapSet, asm.buckets, [o.room, bucket]);
          }
          let w = __reflectApply(__mapGet, gw.objectsById, [o.id]);
          if (w === undefined) {
            // 防御：整建房的行必须始终有包装（主环已处理全部变更行）；
            // 就地补包保持 os/ws 平行，不静默丢行。
            w = gw.wrapObjectResident(o, tick);
            __reflectApply(__mapSet, gw.objectsById, [o.id, w]);
          }
          bucket.os[bucket.os.length] = o;
          bucket.ws[bucket.ws.length] = w;
        }
        if (classDirty) {
          classifyRow(o, __reflectApply(__mapGet, gw.objectsById, [o.id]), coll, viewer);
        }
      }
      if (classDirty) asm.collections = coll;
    }
    const dTail0 = diagOn ? __dateNow() : 0;
    const dRooms = dTail0 - dRooms0;

    // 房间装配：可见房取常驻 entry 并逐拍刷新信封字段；整建房重建查询
    // 索引并重挂结构引用；值变动行做格位迁移（整建房跳过——重填已按新
    // 位置入格）。可见集未变时免 300 条目全信封遍历。
    const rooms = {};
    const roomEntries = sameVisible ? null : __objectEntries(gw.world.rooms || {});
    const visitRoom = (name, r) => {
      let bucket = __reflectApply(__mapGet, asm.buckets, [name]);
      if (bucket === undefined) {
        bucket = { os: [], ws: [] };
        __reflectApply(__mapSet, asm.buckets, [name, bucket]);
      }
      const entry = gw.ensureRoomEntry(name, bucket);
      gw.refreshRoom(name, entry, r);
      if (__reflectApply(__setHas, rebuildRooms, [name])) {
        entry.queries.rebuild(bucket);
        gw.remountStructRefs(entry);
      }
      rooms[name] = entry.result;
    };
    if (sameVisible) {
      for (let i = 0; i < asm.visibleList.length; i++) {
        const name = asm.visibleList[i];
        const r = gw.world.rooms[name];
        // 房条目消失（注销）：等同不可见，仅回填摘除 w.room。
        if (r === undefined) {
          __reflectApply(__setAdd, backfillRooms, [name]);
          continue;
        }
        visitRoom(name, r);
      }
    } else {
      for (let i = 0; i < roomEntries.length; i++) {
        const name = roomEntries[i][0];
        if (viewer !== null && !__reflectApply(__setHas, visibleSet, [name])) continue;
        visitRoom(name, roomEntries[i][1]);
      }
    }
    // 值变动行格位迁移（房间 entry 必在——变更行所在房恒可见；防御性跳过
    // 无 entry 的房，如不可见房中的自有旗）。
    for (let i = 0; i < valueMoves.length; i++) {
      const mv = valueMoves[i];
      if (__reflectApply(__setHas, rebuildRooms, [mv.room])) continue;
      const entry = gw.roomEntryOf(mv.room);
      if (entry !== undefined) entry.queries.moveCell(mv.ox, mv.oy, mv.o, mv.w);
    }

    // 回填 room：整建/可见性变动房的桶整体重指（rooms 无键 → undefined，
    // 与旧版「保持主循环重置值」等价）。
    __reflectApply(__setForEach, rebuildRooms, [
      (room) => {
        __reflectApply(__setAdd, backfillRooms, [room]);
      },
    ]);
    __reflectApply(__mapForEach, asm.buckets, [
      (bucket, bucketRoom) => {
        if (!__reflectApply(__setHas, backfillRooms, [bucketRoom])) return;
        const room = rooms[bucketRoom];
        const ws = bucket.ws;
        for (let i = 0; i < ws.length; i++) {
          if (ws[i] !== undefined) ws[i].room = room;
        }
      },
    ]);
    // 常驻清扫：快路径按 deadIds 精准出表（上方已做），全量路径全表扫描。
    if (!fast) gw.sweepResidentWrappers();
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
    // 分类表出 Game 面：classDirty 轮 coll 即本拍新建表（官方每拍新对象
    // 语义直接满足）；增量轮从常驻表浅拷贝——键序保持上次全量重建的官方
    // 扫描序（无成员变动时与当拍扫描序恒等），玩家挂在 Game.* 上的额外
    // 键随每拍新对象丢弃，与官方一致。
    const creeps = classDirty ? coll.creeps : { ...coll.creeps };
    const spawns = classDirty ? coll.spawns : { ...coll.spawns };
    const structures = classDirty ? coll.structures : { ...coll.structures };
    const constructionSites = classDirty ? coll.constructionSites : { ...coll.constructionSites };
    const flags = classDirty ? coll.flags : { ...coll.flags };
    const resources = classDirty ? coll.resources : { ...coll.resources };
    asm.visibleList = visibleList !== undefined && visibleList !== null ? visibleList : null;
    asm.ready = true;
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
