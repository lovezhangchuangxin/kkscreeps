/* 按 Room 身份持有本 tick 的代价矩阵；提供路径序列化和 findPath 工厂。
 * 缓存对象不进入玩家 Room 属性。来源：上游 rooms.js 的寻路实现。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const { lodash: _ } = globalThis.__kkp;
  const { deprecated } = globalThis.__kkp.deprecation;
  const __Object = Object;
  const __WeakMap = WeakMap;
  const __weakGet = WeakMap.prototype.get;
  const __weakSet = WeakMap.prototype.set;
  const __reflectApply = Reflect.apply;
  const pathRooms = new __WeakMap();
  const C_OBSTACLE_OBJECT_TYPES = globalThis.OBSTACLE_OBJECT_TYPES;
  // 每 tick 的 Room 身份对应独立缓存；不可见房仅使用公共地形。
  // 来源：官方 rooms.js 的 makePathfindingGrid2/getPathfindingGrid2。
  function getPathfindingGrid2(name, opts) {
    const room = gw.currentGame().rooms[name];
    const state = room && __reflectApply(__weakGet, pathRooms, [room]);
    if (!state) return new PathFinder.CostMatrix();
    opts = opts || {};
    let key = 'grid2';
    if (opts.ignoreCreeps) key += '_ignoreCreeps';
    if (opts.ignoreDestructibleStructures) key += '_ignoreDestructibleStructures';
    if (opts.ignoreRoads) key += '_ignoreRoads';
    if (state.grids[key]) return state.grids[key];
    const costs = new PathFinder.CostMatrix();
    let obstacles = _.clone(C_OBSTACLE_OBJECT_TYPES);
    obstacles.push('portal');
    if (opts.ignoreDestructibleStructures) {
      obstacles = _.without(obstacles, 'constructedWall', 'spawn', 'extension', 'link',
        'storage', 'observer', 'tower', 'powerBank', 'powerSpawn', 'lab', 'terminal');
    }
    const safeMode = room.controller && room.controller.safeMode && room.controller.my;
    if (opts.ignoreCreeps || safeMode) obstacles = _.without(obstacles, 'creep', 'powerCreep');
    // Mod 建筑按声明的寻路语义进矩阵：obstacle 阻断、road 按道路成本；
    // 与官方 makePathfindingGrid2 只按类型名查表不同，语义来自冻结计划。
    const modBuildings = gw.modApi === undefined ? undefined : gw.modApi.buildings;
    for (let i = 0; i < state.objects.length; i++) {
      const object = state.objects[i];
      let type = object.type === 'structure' ? object.structureType : object.type;
      if (type === 'wall') type = 'constructedWall';
      const modPathing = modBuildings === undefined ? undefined : modBuildings[type];
      if (_.contains(obstacles, type)
        || !opts.ignoreCreeps && safeMode && (type === 'creep' || type === 'powerCreep')
          && object.owner === gw.viewerName
        || !opts.ignoreDestructibleStructures && type === 'rampart' && !object.isPublic
          && object.owner !== gw.viewerName
        || !opts.ignoreDestructibleStructures && modPathing !== undefined
          && object.type === 'structure' && modPathing.pathing === 'obstacle'
        || !opts.ignoreDestructibleStructures && type === 'constructionSite'
          && object.owner === gw.viewerName
          && (_.contains(C_OBSTACLE_OBJECT_TYPES, object.structureType)
            || (modBuildings !== undefined && modBuildings[object.structureType] !== undefined
              && modBuildings[object.structureType].pathing === 'obstacle'))) {
        costs.set(object.x, object.y, 255);
      }
      if (type === 'swamp' && costs.get(object.x, object.y) === 0) {
        costs.set(object.x, object.y, opts.ignoreRoads ? 5 : 10);
      }
      if (!opts.ignoreRoads && (type === 'road'
          || (modPathing !== undefined && object.type === 'structure'
            && modPathing.pathing === 'road'))
        && costs.get(object.x, object.y) < 255) {
        costs.set(object.x, object.y, 1);
      }
    }
    state.grids[key] = costs;
    return costs;
  }
  gw.getPathfindingGrid2 = getPathfindingGrid2;

  // 两个 wrapper 共用：clone 隔离缓存；只有 CostMatrix 返回值替换矩阵。
  gw.pathfindingRoomCallback = function (opts, visibleOnly) {
    return function (name) {
      if (visibleOnly && !gw.currentGame().rooms[name]) return undefined;
      let matrix = getPathfindingGrid2(name, opts);
      if (typeof opts.costCallback === 'function') {
        matrix = matrix.clone();
        const result = opts.costCallback(name, matrix);
        if (result instanceof PathFinder.CostMatrix) matrix = result;
      }
      return matrix;
    };
  };
  gw.pathfindingDeprecated = function (opts, clear) {
    if (opts.avoid) {
      deprecated('`avoid` option cannot be used when `PathFinder.use()` is enabled. Use `costCallback` instead.');
      if (clear) opts.avoid = undefined;
    }
    if (opts.ignore) {
      deprecated('`ignore` option cannot be used when `PathFinder.use()` is enabled. Use `costCallback` instead.');
      if (clear) opts.ignore = undefined;
    }
  };
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

  gw.trackRoomPathfinding = function (room, objects) {
    __reflectApply(__weakSet, pathRooms, [room, { objects, grids: __Object.create(null) }]);
  };
  gw.makeRoomFindPath = function (name) {
    /** 官方 Room.findPath（rooms.js:863 起）：异房起点先出（serialize 访问
     * 原始 opts——不传 opts 且异房起点按官方抛 TypeError）；默认
     * _useNewPathFinder=true（game.js:68）走 _findPath2（rooms.js:223-305）：
     * PathFinder 地形搜索（range 缺省 1、跨房 maxOps 缺省 20000、非
     * ignoreRoads 时 plainCost 2/swampCost 10——道路本身须经代价矩阵生效）
     * 后按房界截断；无 range 且终点贴近时回填终点。opts.maxOps 的缺省写入
     * 原地修改调用方对象（官方同形）；costCallback 收到缓存矩阵的副本。 */
    return function (fromPos, toPos, opts) {
      if (fromPos.roomName !== name) {
        return opts.serialize ? '' : [];
      }
      opts = opts || {};
      // _findPath2:227-229
      if (fromPos.isEqualTo(toPos)) {
        return opts.serialize ? '' : [];
      }
      gw.pathfindingDeprecated(opts, true);
      // :239-241 跨房且未限时缺省 maxOps 20000（注意 maxRooms>1 条件）
      if (opts.maxOps === undefined && (opts.maxRooms === undefined || opts.maxRooms > 1)
        && fromPos.roomName !== toPos.roomName) {
        opts.maxOps = 20000;
      }
      const searchOpts = {
        roomCallback: gw.pathfindingRoomCallback(opts, false),
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
  };
})();
