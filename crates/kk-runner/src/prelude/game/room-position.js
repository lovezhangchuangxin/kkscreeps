/* 实现 RoomPosition 的坐标编码、距离与方向计算，以及 Room API 转发。
 * 位置保存在非枚举的 __packedPos 中，x/y/roomName 由原型访问器解码并校验修改。
 * findPathTo/findClosestByPath 委托带房间代价矩阵的 PathFinder 原生搜索。
 * 来源：上游 engine/src/game/rooms.js 的 RoomPosition。
 */
(() => {
  'use strict';

  const { lodash: _ } = globalThis.__kkp;
  // gw 引用在加载期捕获进闭包：Rust 装配收尾删除 __kkp 后，运行期（玩家 tick）
  // 仍经此引用读写共享状态——延迟查找 globalThis.__kkp 会失效
  const gw = globalThis.__kkp.gw;

  // 构造器参与每 tick 的世界装配；捕获所需内建函数，避免玩家替换它们后破坏下一 tick。
  const __defineProperty = Object.defineProperty;
  const __parseInt = parseInt;
  const __substr = String.prototype.substr;
  const __charAt = String.prototype.charAt;
  const __reflectApply = Reflect.apply;
  const __Error = Error;
  // 方向/look 常量加载期捕获：官方经闭包 C.RIGHT 免疫玩家覆写（utils.js:70-99），
  // 捕获平铺常量同形（常量段在 prelude 前安装，sandbox.rs 注入序）
  const RIGHT = globalThis.RIGHT,
    LEFT = globalThis.LEFT,
    TOP = globalThis.TOP,
    BOTTOM = globalThis.BOTTOM,
    TOP_LEFT = globalThis.TOP_LEFT,
    TOP_RIGHT = globalThis.TOP_RIGHT,
    BOTTOM_LEFT = globalThis.BOTTOM_LEFT,
    BOTTOM_RIGHT = globalThis.BOTTOM_RIGHT,
    LOOK_TERRAIN = globalThis.LOOK_TERRAIN;

  // 官方 rooms.js:1260-1261：世界尺寸常量（房间坐标 ±128 半径）
  const kMaxWorldSize = 256;
  const kMaxWorldSize2 = kMaxWorldSize >> 1; // 128

  // 官方 utils.js:34-63 fetchXYArguments 逐字移植（instanceof 判定走预置捕获
  // 的 gw.RoomPosition——玩家换 globalThis.RoomPosition 不影响解析，官方
  // globals.RoomPosition 闭包引用同构）。非对象/非实例目标 → undefined 三元组
  // （后续运算得 NaN/false，官方同形）；NaN 归一为 undefined
  function fetchXYArguments(firstArg, secondArg) {
    let x, y, roomName;
    if (_.isUndefined(secondArg) || !_.isNumber(secondArg)) {
      if (!_.isObject(firstArg)) {
        return [undefined, undefined, undefined];
      }
      if (firstArg instanceof gw.RoomPosition) {
        x = firstArg.x;
        y = firstArg.y;
        roomName = firstArg.roomName;
      }
      if (firstArg.pos && firstArg.pos instanceof gw.RoomPosition) {
        x = firstArg.pos.x;
        y = firstArg.pos.y;
        roomName = firstArg.pos.roomName;
      }
    } else {
      x = firstArg;
      y = secondArg;
    }
    if (_.isNaN(x)) {
      x = undefined;
    }
    if (_.isNaN(y)) {
      y = undefined;
    }
    return [x, y, roomName];
  }

  // 官方 utils.js:65-99 getDirection 逐字移植（阈值判定而非角度最近）：
  //   |dx| > 2|dy| → 左右；|dy| > 2|dx| → 上下；否则对角。
  // 官方对 (0,0) 无特判（落到 else 无命中分支 → undefined）
  function getDirection(dx, dy) {
    const adx = Math.abs(dx),
      ady = Math.abs(dy);
    if (adx > ady * 2) {
      return dx > 0 ? RIGHT : LEFT;
    } else if (ady > adx * 2) {
      return dy > 0 ? BOTTOM : TOP;
    } else {
      if (dx > 0 && dy > 0) return BOTTOM_RIGHT;
      if (dx > 0 && dy < 0) return TOP_RIGHT;
      if (dx < 0 && dy > 0) return BOTTOM_LEFT;
      if (dx < 0 && dy < 0) return TOP_LEFT;
    }
  }

  // 官方 utils.js:374-390 roomNameToXY 逐字移植（玩家可见的宽松解析——
  // parseInt 认前缀数字、W/w 与 N/n 取负减一、其余方向字母当 E/S；与 kk-core
  // RoomName 的强类型构造校验是两层：embed 边界拒非法名，玩家面对齐官方行为）
  function roomNameToXY(name) {
    let xx = __parseInt(__reflectApply(__substr, name, [1]), 10);
    let verticalPos = 2;
    if (xx >= 100) verticalPos = 4;
    else if (xx >= 10) verticalPos = 3;
    let yy = __parseInt(__reflectApply(__substr, name, [verticalPos + 1]), 10);
    if (__reflectApply(__charAt, name, [0]) === 'W' || __reflectApply(__charAt, name, [0]) === 'w')
      xx = -xx - 1;
    if (
      __reflectApply(__charAt, name, [verticalPos]) === 'N' ||
      __reflectApply(__charAt, name, [verticalPos]) === 'n'
    )
      yy = -yy - 1;
    return [xx, yy];
  }

  // 官方 utils.js:358-372 getRoomNameFromXY 慢路径逐字移植（E/W + N/S 拼接）
  function getRoomNameFromXYSlow(xx, yy) {
    if (xx < 0) {
      xx = 'W' + (-xx - 1);
    } else {
      xx = 'E' + xx;
    }
    if (yy < 0) {
      yy = 'N' + (-yy - 1);
    } else {
      yy = 'S' + yy;
    }
    return '' + xx + yy;
  }

  // 官方 rooms.js:1263-1274：room 名缓存（进程级稀疏数组，id=(xx+128)<<8|(yy+128)）
  // + 槽 0 预置 'sim'（sim 房间坐标偏移后恰为 0,0）
  const roomNames = [];
  function getRoomNameFromXY(xx, yy) {
    const id = ((xx + kMaxWorldSize2) << 8) | (yy + kMaxWorldSize2);
    const roomName = roomNames[id];
    if (roomName === undefined) {
      return (roomNames[id] = getRoomNameFromXYSlow(xx, yy));
    }
    return roomName;
  }
  roomNames[0] = 'sim';

  // 构造器（官方 rooms.js:1276-1292 逐字）：'sim' 特判（房间坐标强制 -128,-128）→
  // +128 偏移 → 界校验（NaN 用自身不等判定；房间 0..256、坐标 0..49）→
  // 唯一 own 字段 __packedPos（非枚举、可写、非配置——玩家可赋值搬运
  // （UNDOC-PACKEDPOS-003），不可重配置/删除）。编码 xx<<24|yy<<16|x<<8|y
  class RoomPosition {
    constructor(xx, yy, roomName) {
      let xy = roomName === 'sim' ? [-kMaxWorldSize2, -kMaxWorldSize2] : roomNameToXY(roomName);
      xy[0] += kMaxWorldSize2;
      xy[1] += kMaxWorldSize2;
      if (
        xy[0] < 0 ||
        xy[0] > kMaxWorldSize ||
        xy[0] !== xy[0] ||
        xy[1] < 0 ||
        xy[1] > kMaxWorldSize ||
        xy[1] !== xy[1] ||
        xx < 0 ||
        xx > 49 ||
        xx !== xx ||
        yy < 0 ||
        yy > 49 ||
        yy !== yy
      ) {
        throw new __Error('Invalid arguments in RoomPosition constructor');
      }
      __defineProperty(this, '__packedPos', {
        enumerable: false,
        value: (xy[0] << 24) | (xy[1] << 16) | (xx << 8) | yy,
        writable: true,
      });
    }
  }

  // x/y/roomName 原型 accessor（官方 rooms.js:1295-1348 逐字：enumerable、
  // 非 configurable——defineProperties 缺省）。getter 经 packedPos 位运算解码；
  // setter 校验后改写对应字节（'Invalid coordinate' / 'Invalid roomName' 抛错）。
  // roomName getter 先查 roomNames 缓存（sim 槽 0 命中），miss 走世界坐标反解
  Object.defineProperties(RoomPosition.prototype, {
    x: {
      enumerable: true,
      get() {
        return (this.__packedPos >> 8) & 0xff;
      },
      set(val) {
        if (val < 0 || val > 49 || val !== val) {
          throw new __Error('Invalid coordinate');
        }
        this.__packedPos = (this.__packedPos & ~(0xff << 8)) | (val << 8);
      },
    },
    y: {
      enumerable: true,
      get() {
        return this.__packedPos & 0xff;
      },
      set(val) {
        if (val < 0 || val > 49 || val !== val) {
          throw new __Error('Invalid coordinate');
        }
        this.__packedPos = (this.__packedPos & ~0xff) | val;
      },
    },
    roomName: {
      enumerable: true,
      get() {
        const roomName = roomNames[this.__packedPos >>> 16];
        if (roomName === undefined) {
          return getRoomNameFromXY(
            (this.__packedPos >>> 24) - kMaxWorldSize2,
            ((this.__packedPos >>> 16) & 0xff) - kMaxWorldSize2,
          );
        }
        return roomName;
      },
      set(val) {
        let xy = val === 'sim' ? [-kMaxWorldSize2, -kMaxWorldSize2] : roomNameToXY(val);
        xy[0] += kMaxWorldSize2;
        xy[1] += kMaxWorldSize2;
        if (
          xy[0] < 0 ||
          xy[0] > kMaxWorldSize ||
          xy[0] !== xy[0] ||
          xy[1] < 0 ||
          xy[1] > kMaxWorldSize ||
          xy[1] !== xy[1]
        ) {
          throw new __Error('Invalid roomName');
        }
        this.__packedPos = (this.__packedPos & ~(0xffff << 16)) | (xy[0] << 24) | (xy[1] << 16);
      },
    },
  });

  // toJSON（官方 :1350-1356 逐字）：Object.assign 展开继承面（子类 own 枚举字段
  // 合并；本类 own 仅非枚举 __packedPos 不参与）——基础三键先字面量定格。
  // 用运行期全局 Object.assign（官方 realm 语义，玩家可换全局属自害面）
  RoomPosition.prototype.toJSON = function () {
    return Object.assign(
      {
        x: this.x,
        y: this.y,
        roomName: this.roomName,
      },
      this,
    );
  };

  // 官方 :1358：`[room ${roomName} pos ${x},${y}]`——bot 生态日志拼接的事实标准
  RoomPosition.prototype.toString = function () {
    return '[room ' + this.roomName + ' pos ' + this.x + ',' + this.y + ']';
  };

  // 官方 :1362-1376 逐字：三参 (x,y,range) 形态 roomName 取自身；两参
  // (target,range) 形态解包 .pos 且 roomName 必须相等（无缺省豁免——
  // 裸 {x,y} 对象目标恒 false，官方怪癖）
  RoomPosition.prototype.inRangeTo = function (firstArg, secondArg, thirdArg) {
    let x = firstArg,
      y = secondArg,
      range = thirdArg,
      roomName = this.roomName;
    if (_.isUndefined(thirdArg)) {
      let pos = firstArg;
      if (pos.pos) {
        pos = pos.pos;
      }
      x = pos.x;
      y = pos.y;
      roomName = pos.roomName;
      range = secondArg;
    }
    return (
      Math.abs(x - this.x) <= range && Math.abs(y - this.y) <= range && roomName == this.roomName
    );
  };

  // 官方 :1378-1381 逐字：fetchXY + 逐轴 abs<=1（roomName 缺省视为同房间）
  RoomPosition.prototype.isNearTo = function (firstArg, secondArg) {
    const [x, y, roomName] = fetchXYArguments(firstArg, secondArg);
    return (
      Math.abs(x - this.x) <= 1 &&
      Math.abs(y - this.y) <= 1 &&
      (!roomName || roomName == this.roomName)
    );
  };

  // 官方 :1383-1394 逐字：同房间（或数值形态）直接阈值判定；跨房间按世界
  // 坐标换算后再判定
  RoomPosition.prototype.getDirectionTo = function (firstArg, secondArg) {
    const [x, y, roomName] = fetchXYArguments(firstArg, secondArg);
    if (!roomName || roomName == this.roomName) {
      return getDirection(x - this.x, y - this.y);
    }
    const [thisRoomX, thisRoomY] = roomNameToXY(this.roomName);
    const [thatRoomX, thatRoomY] = roomNameToXY(roomName);
    return getDirection(
      thatRoomX * 50 + x - thisRoomX * 50 - this.x,
      thatRoomY * 50 + y - thisRoomY * 50 - this.y,
    );
  };

  // 官方 rooms.js:1396-1428：委托本房 findPath（默认 _useNewPathFinder，
  // 官方 if (roomName == this.roomName || register._useNewPathFinder) 恒真臂）。
  // _.isObject(secondArg) 时官方克隆 opts（findPath 内的 maxOps 写入不漏给
  // 调用方对象）；房间不可见抛官方文案。
  RoomPosition.prototype.findPathTo = function (firstArg, secondArg) {
    const [x, y, rawRoomName] = fetchXYArguments(firstArg, secondArg);
    let opts;
    if (_.isObject(secondArg)) {
      opts = _.clone(secondArg);
    }
    opts = opts || {};
    const roomName = rawRoomName || this.roomName;
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) {
      throw new __Error('Could not access room ' + this.roomName);
    }
    return room.findPath(this, new gw.RoomPosition(x, y, roomName), opts);
  };

  // 官方 rooms.js:1429 起（默认走 _findClosestByPath2，rooms.js:304-374）：
  // range 1 多目标搜索（maxRooms 1、非 ignoreRoads 时 plainCost 2/swampCost
  // 10），路径终点 isNearTo 反查最近目标；同格命中直接返回、无目标 null。
  // costCallback 与 Room.findPath 共用房间矩阵，回调收到独立副本。
  RoomPosition.prototype.findClosestByPath = function (type, opts) {
    opts = _.clone(opts || {});
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) {
      throw new __Error('Could not access room ' + this.roomName);
    }
    if (_.isUndefined(type)) {
      return null;
    }
    // :308-313 数字 type 走 room.find（带 filter）；数组 type 有 filter 才过滤
    let objects;
    if (_.isNumber(type)) {
      objects = room.find(type, { filter: opts.filter });
    } else if (opts.filter) {
      objects = _.filter(type, opts.filter);
    } else {
      objects = type;
    }
    if (!objects.length) {
      return null;
    }
    // :319-322 同格目标直接返回
    const objectOnSquare = _.find(objects, (obj) => this.isEqualTo(obj));
    if (objectOnSquare) {
      return objectOnSquare;
    }
    // :325-328 目标归一：.pos 或自身，range 恒 1
    const goals = _.map(objects, (i) => ({ range: 1, pos: i.pos ? i.pos : i }));
    gw.pathfindingDeprecated(opts, false);
    const searchOpts = {
      roomCallback: gw.pathfindingRoomCallback(opts, true),
      maxOps: opts.maxOps,
      maxRooms: 1,
    };
    if (!opts.ignoreRoads) {
      searchOpts.plainCost = 2;
      searchOpts.swampCost = 10;
    }
    const ret = PathFinder.search(this, goals, searchOpts);
    // :360-373 终点（或起点）反查 isNearTo 的最后一个目标
    let lastPos = this;
    if (ret.path.length) {
      lastPos = ret.path[ret.path.length - 1];
    }
    let result = null;
    objects.forEach((obj) => {
      if (lastPos.isNearTo(obj)) {
        result = obj;
      }
    });
    return result;
  };

  // 官方 :1518-1543：数字 type 走 room.find（opts 透传含 filter）、数组 type
  // 直接过滤；不可达房抛错；inRangeTo（Chebyshev）过滤
  RoomPosition.prototype.findInRange = function (type, range, opts) {
    // 通过闭包读取当前 Game，避免玩家替换全局 Game 后改变此查找。
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) throw new __Error('Could not access room ' + this.roomName);
    // 数字 type 查询 room.find；数组 type 有 filter 时才过滤；其他类型按空列表处理。
    // 来源：上游 engine/src/game/rooms.js 的 RoomPosition.findInRange。
    let objects = [];
    if (_.isNumber(type)) {
      objects = room.find(type, opts);
    }
    if (_.isArray(type)) {
      objects = opts && opts.filter ? _.filter(type, opts.filter) : type;
    }
    return objects.filter((i) => this.inRangeTo(i, range));
  };

  // 按距离查找最近目标；等距时保留数组中首个目标，无目标返回 null。
  // 来源：上游 engine/src/game/rooms.js 的 RoomPosition.findClosestByRange。
  RoomPosition.prototype.findClosestByRange = function (type, opts) {
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) {
      throw new Error('Could not access room ' + this.roomName);
    }
    const o = { filter: opts && opts.filter };
    let objects = [];
    if (typeof type === 'number') {
      objects = room.find(type, o);
    } else if (Array.isArray(type)) {
      objects = o.filter ? type.filter(o.filter) : type;
    }
    let closest = null;
    let minRange = Infinity;
    for (let i = 0; i < objects.length; i++) {
      const range = this.getRangeTo(objects[i]);
      if (range < minRange) {
        minRange = range;
        closest = objects[i];
      }
    }
    return closest;
  };

  // 官方 :1578-1585 逐字：目标带 __packedPos 走快路径（字节位比较，同 packed
  // 即相等——不同写法的同房间同坐标也命中）；否则 fetchXY 兜底（宽松 == 比较）
  RoomPosition.prototype.isEqualTo = function (firstArg, secondArg) {
    if (firstArg.__packedPos !== undefined) {
      return firstArg.__packedPos === this.__packedPos;
    }
    const [x, y, roomName] = fetchXYArguments(firstArg, secondArg);
    return x == this.x && y == this.y && (!roomName || roomName == this.roomName);
  };

  // 官方 :1586-1593 逐字：仅目标带 roomName 且不等时跨房间 Infinity
  RoomPosition.prototype.getRangeTo = function (firstArg, secondArg) {
    const [x, y, roomName] = fetchXYArguments(firstArg, secondArg);
    if (roomName && roomName != this.roomName) {
      return Infinity;
    }
    return Math.max(Math.abs(this.x - x), Math.abs(this.y - y));
  };

  // 委托可见房间的 lookAt；房间不可见时抛错。
  // 来源：上游 engine/src/game/rooms.js 的 RoomPosition.look。
  RoomPosition.prototype.look = function () {
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) {
      throw new __Error('Could not access room ' + this.roomName);
    }
    return room.lookAt(this);
  };

  // terrain 直接读静态地形，其他类型委托可见房间的 lookForAt；不可见房间抛错。
  // 来源：上游 engine/src/game/rooms.js 的 RoomPosition.lookFor。
  RoomPosition.prototype.lookFor = function (type) {
    if (type == LOOK_TERRAIN) {
      return [gw.terrainWordAt(this.roomName, this.x, this.y)];
    }
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) {
      throw new __Error('Could not access room ' + this.roomName);
    }
    return room.lookForAt(type, this);
  };

  // 转发到当前可见房间的 createFlag，不可见房间抛错。
  RoomPosition.prototype.createFlag = function (name, color, secondaryColor) {
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) {
      throw new Error('Could not access room ' + this.roomName);
    }
    return room.createFlag(this, name, color, secondaryColor);
  };

  // 转发到当前可见房间的 createConstructionSite，不可见房间抛错。
  RoomPosition.prototype.createConstructionSite = function (structureType, name) {
    const room = gw.currentGame().rooms[this.roomName];
    if (!room) {
      throw new Error('Could not access room ' + this.roomName);
    }
    return room.createConstructionSite(this.x, this.y, structureType, name);
  };

  // RoomPosition 全局绑定不可写，原型仍可扩展。
  Object.defineProperty(globalThis, 'RoomPosition', {
    enumerable: true,
    value: RoomPosition,
  });
  // 内部装配面走捕获引用（与拆分前单闭包绑定等价）：玩家覆写 globalThis.RoomPosition
  // 不影响 wrapObject/makeRoom/PathFinder 内部闭包的构造
  gw.RoomPosition = RoomPosition;
  // 向后加载的 game.js 提供房间名与世界坐标转换函数。
  gw.roomNameToXY = roomNameToXY;
  gw.getRoomNameFromXY = getRoomNameFromXY;
  // Room.lookAt/lookForAt 复用此坐标参数解析器。
  gw.fetchXYArguments = fetchXYArguments;
  // findPath/moveTo 等后续段经 gw 复用官方方向判定（加载序在本段之后）。
  gw.getDirection = getDirection;
})();
