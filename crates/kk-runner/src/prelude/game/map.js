/* 地图 API 与每 tick 路由/绘图缓存；Game 装配时创建独立实例。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const { deprecated } = globalThis.__kkp.deprecation;
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
  // getTerrainAt 使用本模块捕获的地形位码（上游 game/map.js）。
  const C_TERRAIN_MASK_WALL = globalThis.TERRAIN_MASK_WALL;
  const C_TERRAIN_MASK_SWAMP = globalThis.TERRAIN_MASK_SWAMP;
  function makeMap() {
    // 同一 tick 中惰性创建并复用 MapVisual；绘图缓冲区在 beginTick 清空。
    let _mapVisual;
    // 地图数据来自世界 rooms 键集，包括只携带地形的不可见房间。
    // 现有房间推导四向出口；不建模单边封闭。房间公共状态种子
    //（novice/respawn/closed + statusTimestamp）随房导出，驱动
    // getRoomStatus 与 launchNuke 等消费门；getWorldSize 按官方
    // calcWorldSize 从当前房集活算（含跨度包含式、E/S 半轴锚 0）。
    const worldRooms = gw.world.rooms || {};
    const ROOM_NAME_RE = /^(W|E)\d+(N|S)\d+$/;
    const ROUTE_RE = /(W|E)\d+(N|S)\d+$/;
    // 官方 map.js:9 kRouteGrid=30：A* 搜索网格半径（originX=fromX+30，
    // 60×60 索引界外即不可达）
    const K_ROUTE_GRID = 30;
    const GRID_W = K_ROUTE_GRID * 2;
    function describeExitsImpl(roomName) {
      if (
        !ROOM_NAME_RE.test(roomName) ||
        !Object.prototype.hasOwnProperty.call(worldRooms, roomName)
      ) {
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
            this.heap[(ii = ii >>> 1)] = tmp;
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
      const fromX = fromXY[0],
        fromY = fromXY[1];
      const toX2 = toXY[0],
        toY2 = toXY[1];
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
        return [originX - Math.floor(index / GRID_W), originY - (index % GRID_W)];
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
      const routeCallback =
        (opts && opts.routeCallback) ||
        function () {
          return 1;
        };
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
    // 官方 common/index.js calcWorldSize：现有房间坐标的包含跨度
    // max(maxX-minX+1, maxY-minY+1)；maxX/maxY 初值为 0（E0/S0 锚点），
    // 纯 W/N 世界会量到 E0/S0。空世界无此形态，兜底 1（登记）。
    // 挂 gw 供 market.js 的 calcTransactionCost 环绕同源消费。
    gw.computeWorldSpan = function () {
      let minX = Infinity,
        minY = Infinity,
        maxX = 0,
        maxY = 0,
        has = false;
      for (const name in worldRooms) {
        const xy = gw.roomNameToXY(name);
        if (!Number.isFinite(xy[0]) || !Number.isFinite(xy[1])) continue;
        has = true;
        if (xy[0] < minX) minX = xy[0];
        if (xy[1] < minY) minY = xy[1];
        if (xy[0] > maxX) maxX = xy[0];
        if (xy[1] > maxY) maxY = xy[1];
      }
      if (!has) return 1;
      return Math.max(maxX - minX + 1, maxY - minY + 1);
    };
    return {
      getWorldSize: () => gw.computeWorldSpan(),
      // 官方 utils.js:644-658 calcRoomsDistance：Chebyshev max(|dx|,|dy|)；
      // 连续形经 getWorldSize() 逐轴环绕 min(size−d,d)（我方 size 为当前
      // 房集活算跨度——liveWorldSize 能力键已开放）；无房名正则门
      //（垃圾输入 NaN 透传，官方同形）
      getRoomLinearDistance(roomName1, roomName2, continuous) {
        const a = gw.roomNameToXY(roomName1);
        const b = gw.roomNameToXY(roomName2);
        let dx = Math.abs(b[0] - a[0]);
        let dy = Math.abs(b[1] - a[1]);
        if (continuous) {
          const worldSize = gw.computeWorldSpan();
          dx = Math.min(worldSize - dx, dx);
          dy = Math.min(worldSize - dy, dy);
        }
        return Math.max(dx, dy);
      },
      // 官方 map.js:209-233：closed>novice>respawn 优先（种子域两两
      // 互斥，此处单键直读）、在场 normal、合法名 off-world closed
      //（timestamp null）、非法名 undefined（正则对非字符串的 .test
      // 强转语义官方同形）；novice/respawn/closed 的 timestamp 为种子
      // 墙钟毫秒（vanilla 适配同款未来 1 小时）。
      getRoomStatus(roomName) {
        if (!ROOM_NAME_RE.test(roomName)) return undefined;
        if (Object.prototype.hasOwnProperty.call(worldRooms, roomName)) {
          const status = worldRooms[roomName].status;
          if (status && status !== 'normal') {
            return { status, timestamp: worldRooms[roomName].statusTimestamp ?? null };
          }
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
      // 官方 map.js:199-207：弃用告警先于房名校验（非法名也告警），
      // 在场房 true（accessibleRooms=世界全房表）。
      isRoomAvailable(roomName) {
        deprecated(
          'Method `Game.map.isRoomAvailable` is deprecated and will be removed. Please use `Game.map.getRoomStatus` instead.',
        );
        if (!ROOM_NAME_RE.test(roomName)) return false;
        return Object.prototype.hasOwnProperty.call(worldRooms, roomName);
      },
      // 官方 map.js:262-264：getRoomTerrain 返回 Room.Terrain 实例
      getRoomTerrain: (roomName) => new gw.Terrain(roomName),
      // 绘图点要求 RoomPosition，并携带房间名；style 缺省为空对象。
      // 图形加入 map 缓冲区，限制由 addVisual 检查。
      get visual() {
        if (!_mapVisual) {
          _mapVisual = {};
          Object.defineProperties(_mapVisual, {
            circle: {
              value: function (pos, style) {
                if (!(pos instanceof globalThis.RoomPosition)) {
                  throw new Error('Invalid pos, RoomPosition expected');
                }
                gw.addVisual('map', {
                  t: 'c',
                  x: pos.x,
                  y: pos.y,
                  n: pos.roomName,
                  s: style || {},
                });
                return this;
              },
            },
            line: {
              value: function (pos1, pos2, style) {
                if (!(pos1 instanceof globalThis.RoomPosition)) {
                  throw new Error('Invalid pos1, RoomPosition expected');
                }
                if (!(pos2 instanceof globalThis.RoomPosition)) {
                  throw new Error('Invalid pos2, RoomPosition expected');
                }
                gw.addVisual('map', {
                  t: 'l',
                  x1: pos1.x,
                  y1: pos1.y,
                  n1: pos1.roomName,
                  x2: pos2.x,
                  y2: pos2.y,
                  n2: pos2.roomName,
                  s: style || {},
                });
                return this;
              },
            },
            rect: {
              value: function (pos, w, h, style) {
                if (!(pos instanceof globalThis.RoomPosition)) {
                  throw new Error('Invalid pos, RoomPosition expected');
                }
                gw.addVisual('map', {
                  t: 'r',
                  x: pos.x,
                  y: pos.y,
                  n: pos.roomName,
                  w,
                  h,
                  s: style || {},
                });
                return this;
              },
            },
            poly: {
              value: function (points, style) {
                if (Array.isArray(points) && points.some(Boolean)) {
                  points = points.map((i) => {
                    const p = i.pos || i;
                    return { x: p.x, y: p.y, n: p.roomName };
                  });
                  gw.addVisual('map', { t: 'p', points, s: style || {} });
                }
                return this;
              },
            },
            text: {
              value: function (text, pos, style) {
                if (!(pos instanceof globalThis.RoomPosition)) {
                  throw new Error('Invalid pos , RoomPosition expected');
                }
                gw.addVisual('map', {
                  t: 't',
                  text,
                  x: pos.x,
                  y: pos.y,
                  n: pos.roomName,
                  s: style || {},
                });
                return this;
              },
            },
            clear: {
              value: function () {
                gw.clearVisual('map');
                return this;
              },
            },
            getSize: {
              value: function () {
                return gw.getVisualSize('map');
              },
            },
            export: {
              value: function () {
                return gw.getVisual('map');
              },
            },
            import: {
              value: function (data) {
                gw.addVisual('map', '' + data);
                return this;
              },
            },
          });
        }
        return _mapVisual;
      },
      // 官方 map.js:235-258：弃用告警先于 pos 对象解构（两种调用形态
      // 同一消息、每 tick 去重为一条）；返回 'wall'|'swamp'|'plain'（完整
      // 词，非缩写）；越界或缺少该房间的地形数据时返回 undefined。
      getTerrainAt(x, y, roomName) {
        deprecated(
          'Method `Game.map.getTerrainAt` is deprecated and will be removed. Please use a faster method `Game.map.getRoomTerrain` instead.',
        );
        if (x && typeof x === 'object') {
          y = x.y;
          roomName = x.roomName;
          x = x.x;
        }
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
  }
  globalThis.__kkp.makeMap = makeMap;
})();
