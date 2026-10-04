/* PathFinder.search 经宿主 op 直连原生搜索（kk-pathfinder 的 C++ A*）。
 * CostMatrix 与官方逐字一致；参数清洗、目标归一与坐标转换按官方
 * driver/lib/path-finder.js 逐字复刻（含 |0 强转、|| 缺省与错误文案）。
 * roomCallback 在原生首次触达房间时同步执行；矩阵与异常契约见回调设计。
 */
(() => {
  'use strict';

  const { ops, countStub, lodash: _ } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // CostMatrix._bits 是公开的 2500 字节 Uint8Array，索引为 x*50+y。
  // 序列化将同一缓冲区视为 Uint32Array，得到 625 元素数组；不是 2500 元素字节数组。
  // 反序列化直接创建对应原型的对象并还原字节。
  // 来源：上游 engine/src/game/path-finder.js 的 CostMatrix。
  class CostMatrix {
    constructor() { this._bits = new Uint8Array(2500); }
    set(xx, yy, val) {
      xx = xx | 0; yy = yy | 0;
      this._bits[xx * 50 + yy] = Math.min(Math.max(0, val), 255);
    }
    get(xx, yy) {
      xx = xx | 0; yy = yy | 0;
      return this._bits[xx * 50 + yy];
    }
    clone() {
      const newMatrix = new CostMatrix();
      newMatrix._bits = new Uint8Array(this._bits);
      return newMatrix;
    }
    serialize() {
      return Array.prototype.slice.apply(new Uint32Array(this._bits.buffer));
    }
    static deserialize(data) {
      const instance = Object.create(CostMatrix.prototype);
      instance._bits = new Uint8Array(new Uint32Array(data).buffer);
      return instance;
    }
  }

  // 官方 driver/lib/path-finder.js:9-21 parseRoomName 逐字（"E1N1" →
  // {xx:129, yy:126}；正则与 0..255 检查，失败抛 'Invalid room name'）。
  const kWorldSize = 255;
  function parseRoomName(roomName) {
    const room = /^([WE])([0-9]+)([NS])([0-9]+)$/.exec(roomName);
    if (!room) {
      throw new Error('Invalid room name');
    }
    const rx = (kWorldSize >> 1) + (room[1] === 'W' ? -Number(room[2]) : Number(room[2]) + 1);
    const ry = (kWorldSize >> 1) + (room[3] === 'N' ? -Number(room[4]) : Number(room[4]) + 1);
    if (!(rx >= 0 && rx <= kWorldSize && ry >= 0 && ry <= kWorldSize)) {
      throw new Error('Invalid room name');
    }
    return { xx: rx, yy: ry };
  }

  // 官方 driver/lib/path-finder.js:51-56 generateRoomName 逐字。
  function generateRoomName(xx, yy) {
    return (
      (xx <= kWorldSize >> 1 ? 'W' + ((kWorldSize >> 1) - xx) : 'E' + (xx - (kWorldSize >> 1) - 1)) +
      (yy <= kWorldSize >> 1 ? 'N' + ((kWorldSize >> 1) - yy) : 'S' + (yy - (kWorldSize >> 1) - 1))
    );
  }

  // 官方 driver/lib/path-finder.js:58-70 toWorldPosition 逐字（|0 强转与
  // 0..49 检查——NaN|0 归 0 过检、越界抛 'Invalid room position' 均为官方行为）。
  function toWorldPosition(rp) {
    const xx = rp.x | 0, yy = rp.y | 0;
    if (!(xx >= 0 && xx < 50 && yy >= 0 && yy < 50)) {
      throw new Error('Invalid room position');
    }
    const offset = parseRoomName(rp.roomName);
    return { xx: xx + offset.xx * 50, yy: yy + offset.yy * 50 };
  }

  // 官方 driver/lib/path-finder.js:73-80 fromWorldPosition 逐字：世界坐标
  // 映射回 RoomPosition 实例（bot 生态直接在其上调用方法，含自打原型补丁）。
  function fromWorldPosition(wp) {
    return new gw.RoomPosition(
      wp[0] % 50,
      wp[1] % 50,
      generateRoomName(Math.floor(wp[0] / 50), Math.floor(wp[1] / 50)),
    );
  }

  const PathFinder = {
    CostMatrix,
    use() { countStub('PathFinder.use'); },
    search(origin, goal, options) {
      // 官方 engine/src/game/path-finder.js:60-62：空/无目标早退——两键形态
      //（无 cost/incomplete 键），不触达 origin 与选项。
      if (!goal || Array.isArray(goal) && !goal.length) {
        return { path: [], ops: 0 };
      }
      // 官方 driver/lib/path-finder.js:88-96 选项清洗逐字（含 (v|0)||缺省、
      // maxCost 0xffffffff 缺省与 4294967295|0 → -1 → 1 的官方怪癖）。
      options = options || {};
      const plainCost = Math.min(254, Math.max(1, (options.plainCost | 0) || 1));
      const swampCost = Math.min(254, Math.max(1, (options.swampCost | 0) || 5));
      const heuristicWeight = Math.min(9, Math.max(1, options.heuristicWeight || 1.2));
      const maxOps = Math.max(1, (options.maxOps | 0) || 2000);
      const maxCost = Math.max(1, (options.maxCost | 0) || 0xffffffff);
      const maxRooms = Math.min(64, Math.max(1, (options.maxRooms | 0) || 16));
      const flee = !!options.flee;
      // 官方 :98-112 目标归一：RoomPosition 形态（x/y/roomName 三键齐全）
      // range 0；{pos, range} 形态 range 夹 0。getter 依官方在玩家对象上
      // 执行（毒化 getter 抛错按官方异常面传播）。
      const goals = _.map(Array.isArray(goal) ? goal : [goal], function (g) {
        if (g.x !== undefined && g.y !== undefined && g.roomName !== undefined) {
          return { range: 0, pos: toWorldPosition(g) };
        }
        return { range: Math.max(0, g.range | 0), pos: toWorldPosition(g.pos) };
      });
      // 官方 :114-131：非函数忽略；truthy 返回值读取 _bits（含 getter）。
      let roomCallback = options.roomCallback;
      if (typeof roomCallback === 'function') {
        const callback = roomCallback;
        roomCallback = function (roomName) {
          const result = callback(roomName);
          if (result === false) return false;
          if (result) return result._bits;
          return undefined;
        };
      } else {
        roomCallback = undefined;
      }
      // 原生搜索在宿主侧同步执行：世界公共地形已随本 tick 注入（含未注册
      // 房的全墙边界），缺地形（墙环外）等原生错误以官方消息抛回。
      const originWorld = toWorldPosition(origin);
      const ret = ops.op_kk_pathfinder_search({
        origin: [originWorld.xx, originWorld.yy],
        goals: _.map(goals, (g) => [g.pos.xx, g.pos.yy, g.range]),
        plain_cost: plainCost,
        swamp_cost: swampCost,
        heuristic_weight: heuristicWeight,
        max_ops: maxOps,
        max_cost: maxCost,
        max_rooms: maxRooms,
        flee,
      }, roomCallback);
      // 官方 :133-141：原生 undefined（已在目标 range 内）与 -1（起点房不可
      // 入）两个空结果由 op 折叠为四键形状；路径为行走序世界坐标（不含
      // 起点，pf.cc 回溯在 origin 停止），此处映射后不再反转。
      return {
        path: _.map(ret.path, fromWorldPosition),
        ops: ret.ops,
        cost: ret.cost,
        incomplete: ret.incomplete,
      };
    },
  };
  // PathFinder 全局绑定不可写，但其对象和方法没有因此冻结。
  Object.defineProperty(globalThis, 'PathFinder', {
    enumerable: true, value: PathFinder,
  });
})();
