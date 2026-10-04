/* 收集玩家指令，注入世界并装配每 tick 的 Game。
 * Memory 与市场分别通过加载期捕获的闭包桥连接；Rust 从 memoryWorld 捕获宿主入口。
 */
(() => {
  'use strict';

  const { ops, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 世界解码使用加载期捕获的 JSON.parse，避免玩家补丁改变注入数据。
  const __jsonParse = JSON.parse;
  // 捕获 Object 与 Array，宿主消息的构造和编码不从玩家全局重新查找构造器。
  const __Object = Object;
  const __Array = Array;
  // beginTick 与 buildGame 在同一个 V8 上下文执行；捕获所需方法，减少玩家补丁对装配的影响。
  const __Map = Map;
  const __Set = Set;
  const __mapGet = Map.prototype.get;
  const __mapSet = Map.prototype.set;
  const __setAdd = Set.prototype.add;
  const __setHas = Set.prototype.has;
  // 调用已捕获的原型方法时使用捕获的 Reflect.apply，避免动态查找 Function.prototype.call。
  const __reflectApply = Reflect.apply;
  const __objectEntries = Object.entries;
  const mathPow = Math.pow;
  const mathFloor = Math.floor;
  // 捕获地图和 CPU 常量，以及主动中断 op 的闭包引用。
  const cpuHalt = globalThis.__kkp.cpuHalt;
  const C_TOP = globalThis.TOP;
  const C_BOTTOM = globalThis.BOTTOM;
  const C_LEFT = globalThis.LEFT;
  const C_RIGHT = globalThis.RIGHT;
  const C_FIND_EXIT_TOP = globalThis.FIND_EXIT_TOP;
  const C_FIND_EXIT_RIGHT = globalThis.FIND_EXIT_RIGHT;
  const C_FIND_EXIT_BOTTOM = globalThis.FIND_EXIT_BOTTOM;
  const C_FIND_EXIT_LEFT = globalThis.FIND_EXIT_LEFT;
  const C_ERR_NO_PATH = globalThis.ERR_NO_PATH;
  const C_ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const C_ERR_FULL = globalThis.ERR_FULL;
  const C_OK = globalThis.OK;
  // getTerrainAt 的位码（官方 map.js:255-258 经闭包 C——搬入 game 段后
  // 须本段捕获；room.js 段的捕获不跨段）
  const C_TERRAIN_MASK_WALL = globalThis.TERRAIN_MASK_WALL;
  const C_TERRAIN_MASK_SWAMP = globalThis.TERRAIN_MASK_SWAMP;
  const GCL_MULTIPLY = globalThis.GCL_MULTIPLY;
  const GCL_POW = globalThis.GCL_POW;
  // ---------------------------------------------------------------- Game 装配

  const { probe, lodash, memory, makeMarket } = globalThis.__kkp;
  // 加载时捕获 CJS 钩子；运行时 __kkp 已被 Rust 删除，不能再从全局查找。
  const { ensureRequire, setCurrentUser } = globalThis.__kkp.cjs;

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
      // 官方 game.js:145：cpuLimit = runtimeData.cpu（tick 窗口同值）
      cpuLimit: cpu.tickLimit,
      cpu: {
        limit: cpu.limit,
        tickLimit: cpu.tickLimit,
        bucket: cpu.bucket,
        shardLimits: { shard0: cpu.limit },
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
      map: (() => {
        // 同一 tick 中惰性创建并复用 MapVisual；绘图缓冲区在 beginTick 清空。
        let _mapVisual;
        // 地图数据来自世界 rooms 键集，包括只携带地形的不可见房间。
        // 现有房间推导四向出口；不建模单边封闭、novice 或 respawn 状态。
        // getRoomStatus 只返回 normal/closed，getWorldSize 固定为 1；距离计算不环绕。
        const worldRooms = gw.world.rooms || {};
        const ROOM_NAME_RE = /^(W|E)\d+(N|S)\d+$/;
        const ROUTE_RE = /(W|E)\d+(N|S)\d+$/;
        // 官方 map.js:9 kRouteGrid=30：A* 搜索网格半径（originX=fromX+30，
        // 60×60 索引界外即不可达）
        const K_ROUTE_GRID = 30;
        const GRID_W = K_ROUTE_GRID * 2;
        function describeExitsImpl(roomName) {
          if (!ROOM_NAME_RE.test(roomName)
            || !Object.prototype.hasOwnProperty.call(worldRooms, roomName)) {
            return null;
          }
          const xy = gw.roomNameToXY(roomName);
          // 官方 :26-40 四向标记门（我方恒开——见上登记）；键序 t/b/l/r 官方同
          const exits = {};
          exits[C_TOP] = gw.getRoomNameFromXY(xy[0], xy[1] - 1);
          exits[C_BOTTOM] = gw.getRoomNameFromXY(xy[0], xy[1] + 1);
          exits[C_LEFT] = gw.getRoomNameFromXY(xy[0] - 1, xy[1]);
          exits[C_RIGHT] = gw.getRoomNameFromXY(xy[0] + 1, xy[1]);
          return exits;
        }
          // 堆在相等优先级时仍执行上浮或下沉，更新操作线性查找已有节点。
          // 这些比较符号影响等成本路线的返回顺序，不能仅按最短成本相同就替换。
          // 来源：上游 engine/src/game/path-utils.js 的 Heap/OpenClosed。
          class OpenClosed {
            constructor(size) {
              this.list = new Uint8Array(size);
              this.marker = 1;
            }
            clear() {
              if (this.marker >= 253) {
                this.list = new Uint8Array(this.list.length);
                this.marker = 1;
              } else {
                this.marker += 2;
              }
            }
            isOpen(index) {
              return this.list[index] === this.marker;
            }
            isClosed(index) {
              return this.list[index] === this.marker + 1;
            }
            open(index) {
              this.list[index] = this.marker;
            }
            close(index) {
              this.list[index] = this.marker + 1;
            }
          }
          class Heap {
            constructor(size, ctor) {
              this.priorities = new (ctor || Uint16Array)(size + 1);
              this.heap = new Uint16Array(size + 1);
              this.size_ = 0;
            }
            minPriority() {
              return this.priorities[this.heap[1]];
            }
            min() {
              return this.heap[1];
            }
            size() {
              return this.size_;
            }
            priority(index) {
              return this.priorities[index];
            }
            pop() {
              this.heap[1] = this.heap[this.size_];
              --this.size_;
              let vv = 1;
              do {
                let uu = vv;
                if ((uu << 1) + 1 <= this.size_) {
                  if (this.priorities[this.heap[uu]] >= this.priorities[this.heap[uu << 1]]) {
                    vv = uu << 1;
                  }
                  if (this.priorities[this.heap[vv]] >= this.priorities[this.heap[(uu << 1) + 1]]) {
                    vv = (uu << 1) + 1;
                  }
                } else if (uu << 1 <= this.size_) {
                  if (this.priorities[this.heap[uu]] >= this.priorities[this.heap[uu << 1]]) {
                    vv = uu << 1;
                  }
                }
                if (uu !== vv) {
                  let tmp = this.heap[uu];
                  this.heap[uu] = this.heap[vv];
                  this.heap[vv] = tmp;
                } else {
                  return;
                }
              } while (true);
            }
            push(index, priority) {
              this.priorities[index] = priority;
              let ii = ++this.size_;
              this.heap[ii] = index;
              this.bubbleUp(ii);
            }
            update(index, priority) {
              for (let ii = this.size_; ii > 0; --ii) {
                if (this.heap[ii] === index) {
                  this.priorities[index] = priority;
                  this.bubbleUp(ii);
                  return;
                }
              }
            }
            bubbleUp(ii) {
              while (ii !== 1) {
                if (this.priorities[this.heap[ii]] <= this.priorities[this.heap[ii >>> 1]]) {
                  let tmp = this.heap[ii];
                  this.heap[ii] = this.heap[ii >>> 1];
                  this.heap[ii = ii >>> 1] = tmp;
                } else {
                  return;
                }
              }
            }
            clear() {
              this.size_ = 0;
            }
          }
          // 官方 map.js:13-15 模块级复用形态（同 run 内多次 findRoute 共享，
          // clear() 重置）——IIFE 闭包级同位
          let heap, openClosed, parents;
          function findRouteImpl(fromRoom, toRoom, opts) {
            if (fromRoom && typeof fromRoom === 'object') fromRoom = fromRoom.name;
            if (toRoom && typeof toRoom === 'object') toRoom = toRoom.name;
            if (fromRoom === toRoom) return [];
            if (!ROUTE_RE.test(fromRoom) || !ROUTE_RE.test(toRoom)) return C_ERR_NO_PATH;
            const fromXY = gw.roomNameToXY(fromRoom);
            const toXY = gw.roomNameToXY(toRoom);
            const fromX = fromXY[0], fromY = fromXY[1];
            const toX2 = toXY[0], toY2 = toXY[1];
            // 官方 :126-128：同名异写（同坐标不同房名）→ []
            if (fromX === toX2 && fromY === toY2) return [];
            const originX = fromX + K_ROUTE_GRID;
            const originY = fromY + K_ROUTE_GRID;
            // 官方 :48-59 索引形态（含界外 undefined 哨兵）
            function xyToIndex(xx, yy) {
              const ox = originX - xx;
              const oy = originY - yy;
              if (ox < 0 || ox >= GRID_W || oy < 0 || oy >= GRID_W) return undefined;
              return ox * GRID_W + oy;
            }
            function indexToXY(index) {
              return [originX - Math.floor(index / GRID_W), originY - index % GRID_W];
            }
            function heuristic(xx, yy) {
              return Math.abs(xx - toX2) + Math.abs(yy - toY2);
            }
            if (heap) {
              heap.clear();
              openClosed.clear();
            } else {
              heap = new Heap(GRID_W * GRID_W, Float64Array);
              openClosed = new OpenClosed(GRID_W * GRID_W);
            }
            if (!parents) {
              parents = new Uint16Array(GRID_W * GRID_W);
            }
            const fromIndex = xyToIndex(fromX, fromY);
            heap.push(fromIndex, heuristic(fromX, fromY));
            const routeCallback = (opts && opts.routeCallback) || function () { return 1; };
            // 官方 :134-194 Astar 主循环逐字（含 cost `Number(cb(...)) || 1`
            // 的 0→1 归一、Infinity 剪枝、decrease-key 更新序）
            while (heap.size()) {
              const index = heap.min();
              const fcost = heap.minPriority();
              heap.pop();
              openClosed.close(index);
              const xy = indexToXY(index);
              const hcost = heuristic(xy[0], xy[1]);
              const gcost = fcost - hcost;
              if (hcost === 0) {
                const route = [];
                let cur = index;
                while (cur !== fromIndex) {
                  const cxy = indexToXY(cur);
                  cur = parents[cur];
                  const pxy = indexToXY(cur);
                  let dir;
                  if (pxy[0] < cxy[0]) dir = C_FIND_EXIT_RIGHT;
                  else if (pxy[0] > cxy[0]) dir = C_FIND_EXIT_LEFT;
                  else if (pxy[1] < cxy[1]) dir = C_FIND_EXIT_BOTTOM;
                  else dir = C_FIND_EXIT_TOP;
                  route.push({ exit: dir, room: gw.getRoomNameFromXY(cxy[0], cxy[1]) });
                }
                route.reverse();
                return route;
              }
              const fromRoomName = gw.getRoomNameFromXY(xy[0], xy[1]);
              const exits = describeExitsImpl(fromRoomName);
              for (let dir in exits) {
                const roomName = exits[dir];
                const nxy = gw.roomNameToXY(roomName);
                const neighborIndex = xyToIndex(nxy[0], nxy[1]);
                if (neighborIndex === undefined || openClosed.isClosed(neighborIndex)) continue;
                const cost = Number(routeCallback(roomName, fromRoomName)) || 1;
                if (cost === Infinity) continue;
                const nf = gcost + heuristic(nxy[0], nxy[1]) + cost;
                if (openClosed.isOpen(neighborIndex)) {
                  if (heap.priority(neighborIndex) > nf) {
                    heap.update(neighborIndex, nf);
                    parents[neighborIndex] = index;
                  }
                } else {
                  heap.push(neighborIndex, nf);
                  openClosed.open(neighborIndex);
                  parents[neighborIndex] = index;
                }
              }
            }
          return C_ERR_NO_PATH;
        }
        return {
          getWorldSize: () => 1,
          // 官方 utils.js:644-658 calcRoomsDistance：Chebyshev max(|dx|,|dy|)
          //（连续形按 worldSize 环绕——我方 worldSize 登记面恒 1，环绕臂
          // 形态保留）；无房名正则门（垃圾输入 NaN 透传，官方同形）
          getRoomLinearDistance(roomName1, roomName2, continuous) {
            const a = gw.roomNameToXY(roomName1);
            const b = gw.roomNameToXY(roomName2);
            let dx = Math.abs(b[0] - a[0]);
            let dy = Math.abs(b[1] - a[1]);
            // 2B 批 P2 勘正：continuous 环绕经 getWorldSize()（官方 map.js 同位
            // min(size−d,d)）——我方单机世界**不环绕**（getWorldSize=1 登记面；
            // 原 worldSize=1 字面套环绕公式会把邻房距离折成 0——MARKET-
            // QUERY-001 红），与 Rust settle/market.rs calc_rooms_distance/
            // calcTransactionCost 三处同判（liveWorldSize 能力键 false，
            // TERMINAL-SEND-015 wrap 计费维持 skip）
            if (continuous) {
              const worldSize = Infinity;
              dx = Math.min(worldSize - dx, dx);
              dy = Math.min(worldSize - dy, dy);
            }
            return Math.max(dx, dy);
          },
          // 官方 map.js:204-233：novice/respawn/closed 三态无数据源跳过
          //（登记）；在场房 normal、合法名 off-world closed、非法名 undefined
          //（正则对非字符串的 .test 强转语义官方同形）；官方 runtimeData
          // 无 roomStatusData 时抛错——我方恒有数据面不抛（登记）
          getRoomStatus(roomName) {
            if (!ROOM_NAME_RE.test(roomName)) return undefined;
            if (Object.prototype.hasOwnProperty.call(worldRooms, roomName)) {
              return { status: 'normal', timestamp: null };
            }
            return { status: 'closed', timestamp: null };
          },
          describeExits: describeExitsImpl,
          findRoute: findRouteImpl,
          findExit(fromRoom, toRoom, opts) {
            const route = findRouteImpl(fromRoom, toRoom, opts);
            if (!Array.isArray(route)) return route;
            if (!route.length) return C_ERR_INVALID_ARGS;
            return route[0].exit;
          },
          // 官方 map.js:207-213（deprecated）：在场房 true（accessibleRooms
          // =世界全房表）；官方 register.deprecated 提示不仿真（登记）
          isRoomAvailable(roomName) {
            if (!ROOM_NAME_RE.test(roomName)) return false;
            return Object.prototype.hasOwnProperty.call(worldRooms, roomName);
          },
          // 官方 map.js:262-264：getRoomTerrain 返回 Room.Terrain 实例
          getRoomTerrain: (roomName) => new (gw.Terrain)(roomName),
          // 绘图点要求 RoomPosition，并携带房间名；style 缺省为空对象。
          // 图形加入 map 缓冲区，限制由 addVisual 检查。
          get visual() {
            if (!_mapVisual) {
              _mapVisual = {};
              Object.defineProperties(_mapVisual, {
                circle: { value: function (pos, style) {
                  if (!(pos instanceof globalThis.RoomPosition)) {
                    throw new Error('Invalid pos, RoomPosition expected');
                  }
                  gw.addVisual('map', { t: 'c', x: pos.x, y: pos.y, n: pos.roomName, s: style || {} });
                  return this;
                } },
                line: { value: function (pos1, pos2, style) {
                  if (!(pos1 instanceof globalThis.RoomPosition)) {
                    throw new Error('Invalid pos1, RoomPosition expected');
                  }
                  if (!(pos2 instanceof globalThis.RoomPosition)) {
                    throw new Error('Invalid pos2, RoomPosition expected');
                  }
                  gw.addVisual('map', {
                    t: 'l',
                    x1: pos1.x, y1: pos1.y, n1: pos1.roomName,
                    x2: pos2.x, y2: pos2.y, n2: pos2.roomName,
                    s: style || {},
                  });
                  return this;
                } },
                rect: { value: function (pos, w, h, style) {
                  if (!(pos instanceof globalThis.RoomPosition)) {
                    throw new Error('Invalid pos, RoomPosition expected');
                  }
                  gw.addVisual('map', { t: 'r', x: pos.x, y: pos.y, n: pos.roomName, w, h, s: style || {} });
                  return this;
                } },
                poly: { value: function (points, style) {
                  if (Array.isArray(points) && points.some(Boolean)) {
                    points = points.map((i) => {
                      const p = i.pos || i;
                      return { x: p.x, y: p.y, n: p.roomName };
                    });
                    gw.addVisual('map', { t: 'p', points, s: style || {} });
                  }
                  return this;
                } },
                text: { value: function (text, pos, style) {
                  if (!(pos instanceof globalThis.RoomPosition)) {
                    throw new Error('Invalid pos , RoomPosition expected');
                  }
                  gw.addVisual('map', { t: 't', text, x: pos.x, y: pos.y, n: pos.roomName, s: style || {} });
                  return this;
                } },
                clear: { value: function () {
                  gw.clearVisual('map');
                  return this;
                } },
                getSize: { value: function () {
                  return gw.getVisualSize('map');
                } },
                export: { value: function () {
                  return gw.getVisual('map');
                } },
                import: { value: function (data) {
                  gw.addVisual('map', '' + data);
                  return this;
                } },
              });
            }
            return _mapVisual;
          },
          // 官方 map.js:235-258：返回 'wall'|'swamp'|'plain'（完整词，非缩写）；
          // 越界或缺少该房间的地形数据时返回 undefined；位码由 TERRAIN_MASK_* 解释。
          getTerrainAt(x, y, roomName) {
            if (x && typeof x === 'object') { y = x.y; roomName = x.roomName; x = x.x; }
            if (x < 0 || x > 49 || y < 0 || y > 49) return undefined;
            const terrain = gw.terrainByRoom[roomName];
            if (!terrain) return undefined;
            const t = terrain[y * 50 + x];
            if (t === undefined || t === null) return undefined;
            if (t & C_TERRAIN_MASK_WALL) return 'wall';
            if (t & C_TERRAIN_MASK_SWAMP) return 'swamp';
            return 'plain';
          },
        };
        })(),
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

  // 每 tick 重建闭包内的指令清单：对象动作按 id/动作名覆盖，重复设置不重复计费；
  // room/global 动作按数组元素计费。say/pull 不收费，remove 统一减 0.2。
  // JS cpu 供 getUsed 显示，Rust 在 end_tick 按最终保留清单重新计算费用。
  // 来源：上游 driver/lib/runtime/runtime.js 的 intents 与 make.js 的 intentsCount。
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
      // 扩展意图专用：载荷原样保留（null/原始值都合法——参数 schema 允许
      // nullable），不做原型剥离。继承的 toJSON 可改写出站载荷——与完整
      // 性无关（args 本就由玩家全权控制），结构校验在宿主结算侧按冻结
      // 计划独立进行。计费按键存在性判定（首次发射计 0.2），与宿主侧
      // "每键一次"同轨，不因载荷为 null/0/false 等假值重复计费。
      setExt(id, name, data) {
        list[id] = list[id] || __Object.create(null);
        if (list[id][name] === undefined) cpu += 0.2;
        list[id][name] = data;
      },
      // 全局 intent 通道（官方 runtime.js:78-86 逐字）：list 顶层按动作名
      // 键的数组（非对象 id 路由）；maxLen 上限返 false（Game.notify 20——
      // 每 tick 上限，beginTick 重建 list 即重置）
      push(name, data, maxLen) {
        list[name] = list[name] || [];
        if (maxLen && list[name].length >= maxLen) {
          return false;
        }
        list[name].push(data);
        cpu += 0.2;
        return true;
      },
      pushByName(id, name, data, maxLen) {
        list[id] = list[id] || __Object.create(null);
        __Object.setPrototypeOf(data, null);
        list[id][name] = list[id][name] || [];
        // 官方 runtime.js:86-88 maxLen 门：满返 false（market deal 10 /
        // cancelOrder·changeOrderPrice·extendOrder 50——2B 批 P2 消费）
        if (maxLen && list[id][name].length >= maxLen) {
          return false;
        }
        list[id][name].push(data);
        // 官方 room/global 级按元素数计费（make.js:100-106；免费表 say/pull
        // 是对象级语义）——Rust 侧 take_endtick_intents 同口径兜底，此处计入
        // 玩家可见的 getUsed（intents.cpu 显示面）
        cpu += 0.2;
        return true;
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
    /** 设置闭包 rawMemoryData，供下一次 beginTick 创建 RawMemory 时使用。
     * 不调用玩家可替换的全局 RawMemory.set，须在 beginTick 前完成。 */
    setRawMemoryHost: memory.setRawMemoryHost,
    /** 读取当前 RawMemory 数据作为待提交候选；真正保存到 PlayerState 由 Rust 协调器完成。 */
    rawMemoryDataHost: memory.rawMemoryDataHost,
    /** 设置活动 segment 窗口，下一次 beginTick 将内容复制到新的 RawMemory.segments。 */
    setSegmentsWindowHost: memory.setSegmentsWindowHost,
    /** 宿主私有通道（P6）：注入本 tick 的 foreignSegment（官方
     *  data.foreignMemorySegment——null 串表示无）。 */
    setForeignSegmentHost: memory.setForeignSegmentHost,
    /** 宿主私有通道（P6）：tick 尾 segments 信封（writeBackMemory 成功打包的
     *  写回+四请求快照；endTick 随 rawMemory 一并带出——失败轮为 null）。 */
    segmentsOutHost: memory.segmentsOutHost,
    /** 每 tick 注入世界快照（宿主经私有句柄以字符串参数传入——下方双分支
     * 兼容 string/对象两种形态）并重建 Game。 */
    beginTick(worldJson, cpuMeta) {
      beginTickImpl(worldJson, cpuMeta, null, null);
    },
    /** KKPV 通道注入：worldJson 是不含对象数组与地形的世界信封；对象记录
     * 由可信解码器从私有块束（ArrayBuffer）解出，地形由公开地形束解出
     * 零拷贝视图。装配后与 JSON 通道的等同面：gw.world.objects 的成员、
     * 数组顺序（块房序×ID 序）、字段值与缺失形态、Store/嵌套记录的键序
     * （载荷保序）、terrainByRoom 的逐格地形值；两者共用同一段
     * beginTickImpl 后续步骤。两个束须同时提供。 */
    beginTickKkpv(worldJson, bundle, publicBundle, cpuMeta) {
      beginTickImpl(worldJson, cpuMeta, bundle, publicBundle);
    },
    writeBackMemory: memory.writeBackMemory,
    intentsPayload,
  };

  /** beginTick/beginTickKkpv 的共用实现；bundle 非 null 时对象数组来自
   * KKPV 私有块束（信封不含 objects 键），publicBundle 非 null 时地形
   * 来自公开地形束的零拷贝视图（信封不含 terrain 键），其余步骤两条
   * 通道完全一致。两个束必须同时提供或同时为空——单侧出现即装配错误。 */
  function beginTickImpl(worldJson, cpuMeta, bundle, publicBundle) {
      const hasPrivate = bundle !== null && bundle !== undefined;
      const hasPublic = publicBundle !== null && publicBundle !== undefined;
      if (hasPrivate !== hasPublic) {
        throw new Error('KKPV 通道的公开束与私有束须同时提供或同时为空');
      }
      // 使用捕获的 JSON.parse 解码，避免玩家替换全局函数后改写世界与用户身份。
      gw.world = typeof worldJson === 'string' ? __jsonParse(worldJson) : worldJson;
      let terrainViews = null;
      if (hasPrivate) {
        // 块头 tick 与信封 time 在解码器内交叉校验（同一投影的同界快照）。
        gw.world.objects = gw.kkpv.decodeBundle(bundle, gw.world.time);
        terrainViews = gw.kkpv.decodePublicBundle(publicBundle, gw.world.time);
      }
      // Game.cpu 真值合并（宿主 begin_tick 第二参数——官方 make.js 先算窗口
      // 再装 runtimeData 的等价物；world.cpu 占位基准被此覆盖，键恒权威）
      if (cpuMeta !== undefined && cpuMeta !== null) {
        gw.world.cpu = typeof cpuMeta === 'string' ? __jsonParse(cpuMeta) : cpuMeta;
      }
      // 注入侧剥顶层原型（官方 runtime.js:56 `systemFunctions.Object.setPrototypeOf
      //(data, null)` 同位）：顶层缺失键（cpu/user 兜底读）不被继承 getter 命中；
      // 内层对象官方同样不剥——缺失键走原型链的暴露面官方一致，不深剥
      __Object.setPrototypeOf(gw.world, null);
      gw.viewerName = gw.world.user ?? null;
      // 先建立当前 tick 的原始对象索引，再装配 Game；装配期间的 getter 已会回查该索引。
      gw.rawById = new __Map();
      const tickObjs = gw.world.objects || [];
      for (let i = 0; i < tickObjs.length; i++) {
        const o = tickObjs[i];
        if (o && o.id !== undefined) __reflectApply(__mapSet, gw.rawById, [o.id, o]);
      }
      resetIntents(); // 官方每 tick 新建收集器（runtime.js _start 每 tick 执行）
      // 重置安全模式最后请求与本 tick 占领计数，避免上一 tick 的状态影响本轮预检。
      gw.lastActivateSafeMode = null;
      gw.controllersClaimedInTick = 0;
      // 重置本 tick 的旗帜名、spawn 名和工地计数；世界查询快照尚不包含新建指令。
      gw.createdFlagNames = new __Set();
      gw.createdConstructionSites = 0;
      gw.createdSpawnNames = new __Set();
      // 官方每 tick init（runtime.js:471-484 / make.js:160 等价物）：RawMemory
      // **重建**（官方 rawMemory 是 _start 局部变量每 tick 新建——_parsed 单
      // tick 生命周期；玩家可删可换全局绑定，每 tick 恢复）、Memory 重装懒
      // getter（上一 tick 首访后已固化为值属性，configurable 允许覆盖）。
      // 解析缓存只存当 tick RawMemory._parsed（set/delete 均失效它），无闭包旁路
      memory.resetRawMemoryHost();
      // 官方 game.js:504-512：require 每 tick 重绑四条件判定（缺失/timestamp
      // 变/main 非对象/main.loop 非函数 → 整体重绑）；moduleObject.user 注入
      ensureRequire();
      setCurrentUser(gw.viewerName);
      memory.installMemoryHost();
      gw.terrainByRoom = {};
      // 新 tick 开始时丢弃上一 tick 的绘图缓冲区。
      if (gw.visualReset) gw.visualReset();
      // 地形表一律经 defineProperty 填自有属性：房名形如 "__proto__" 时
      // 普通赋值会改写原型而不是建键（两条通道同一保护；引擎房名不可
      // 产生该形态，防御面针对手搓世界注入）。
      const setTerrain = (room, bytes) => {
        __Object.defineProperty(gw.terrainByRoom, room, {
          value: bytes, writable: true, enumerable: true, configurable: true,
        });
      };
      if (terrainViews !== null) {
        // KKPV 通道：公开束的零拷贝视图直接作为地形源——无 b64 解码、
        // 无逐 tick 重分配（视图锚定共享内存，旧代际随视图失活回收）。
        // rev 缓存路径不参与：束每代际整份更换，不存在同 rev 复用问题。
        for (const room in terrainViews) {
          setTerrain(room, terrainViews[room]);
        }
      } else {
        const terrainEntries = __objectEntries(gw.world.rooms || {});
        for (let i = 0; i < terrainEntries.length; i++) {
          const roomName = terrainEntries[i][0];
          const t = terrainEntries[i][1].terrain;
          if (t && typeof t === 'object' && typeof t.payload === 'string') {
            // 紧凑协议（P3 真路径）：rev 命中跨 tick 复用解码产物（地形首玩家
            // tick 后不可变，稳态零解码）；未命中解码 b64 → 2500 字节索引兼容
            const cached = gw.terrainCache[roomName];
            if (!cached || cached.rev !== t.rev) {
              gw.terrainCache[roomName] = { rev: t.rev, bytes: gw.b64ToBytes(t.payload) };
            }
            setTerrain(roomName, gw.terrainCache[roomName].bytes);
          } else if (__Array.isArray(t)) {
            // 旧数组形态（宿主手搓注入的夹具路径——bot_check 桩世界等；真
            // 路径已不产出），原样接收
            setTerrain(roomName, t);
          }
        }
      }
      // 调用各 API 注册的 tick 重置函数，例如本 tick 已使用的 creep 名。
      // 使用索引遍历，避免动态调用玩家可替换的数组迭代器。
      if (gw.perTickReset) {
        const fns = gw.perTickReset;
        for (let i = 0; i < fns.length; i++) fns[i]();
      }
      refreshGame();
  }
})();
