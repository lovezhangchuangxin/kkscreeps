/* kk-runner prelude · 30_game 段 2：RoomPosition（对照官方 engine/src/game/rooms.js:1276-1662）。
 *
 * 官方全局类；纯计算方法给真实现，副作用方法为桩。
 * 依赖：gw.terrainWordAt（lookFor 的 terrain 分支，30_game/room.js 注册）。
 */
(() => {
  'use strict';

  const { countStub } = globalThis.__kkp;
  // gw 引用在加载期捕获进闭包：90_host 末段删除 __kkp 后，运行期（玩家 tick）
  // 仍经此引用读写共享状态——延迟查找 globalThis.__kkp 会失效
  const gw = globalThis.__kkp.gw;

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
        return [gw.terrainWordAt(this.roomName, this.x, this.y)];
      }
      countStub('RoomPosition.lookFor'); return [];
    }
    inRangeTo() { return this.isInRangeTo.apply(this, arguments); }
  }
  globalThis.RoomPosition = RoomPosition;
  // 内部装配面走捕获引用（与拆分前单闭包绑定等价）：玩家覆写 globalThis.RoomPosition
  // 不影响 wrapObject/makeRoom/PathFinder 桩内部的构造
  gw.RoomPosition = RoomPosition;
})();
