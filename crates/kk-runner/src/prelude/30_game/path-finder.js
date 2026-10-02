/* 提供 CostMatrix 与直线步进的 PathFinder.search。
 * CostMatrix 使用 x*50+y 索引，将 2500 字节通过 Uint32Array 打包成 625 个数值。
 * search 只采用首个目标，忽略房间、障碍和搜索选项，仍返回 incomplete:false；
 * 此结果不能用于判断真实路径可达性。尚未调用 kk-pathfinder 的原生寻路实现。
 */
(() => {
  'use strict';

  const { countStub } = globalThis.__kkp;
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
  function straightPath(from, to, range) {
    // 官方 path-finder.js：search 返回的 path 元素是 **RoomPosition 实例**（绝对
    // 坐标）——bot 生态（tooangel 等）直接在这些元素上调用方法（含其自打的
    // 原型补丁），返回 {dx,dy} 步进对象会让 bot 每 tick 崩溃
    const positions = [];
    let x = from.x, y = from.y;
    const roomName = from.roomName;
    let guard = 0;
    // 官方 pf.cc:574：回溯在 pos == origin 处停止——path **不含起点**（bot 生态
    // 依赖此语义，tooangel 甚至自行 splice 把起点插回头部）；同起终点 → 空路径。
    // goal {pos, range}：进入目标的 range（Chebyshev）邻域即停，range 缺省 0——
    // 官方路径止于 range 边界，bot 自行拼端点（tooangel buildPath 的 push(end)）
    // 才不会产生重复项
    const dist = () => Math.max(Math.abs(to.x - x), Math.abs(to.y - y));
    while (dist() > range && guard++ < 100) {
      x += Math.sign(to.x - x);
      y += Math.sign(to.y - y);
      positions.push(new gw.RoomPosition(x, y, roomName));
    }
    return positions;
  }
  const PathFinder = {
    CostMatrix,
    use() { countStub('PathFinder.use'); },
    search(origin, goal, opts) {
      countStub('PathFinder.search');
      const goals = Array.isArray(goal) ? goal : [goal];
      const g0 = goals[0];
      const to = g0 && g0.pos ? g0.pos : g0;
      // 官方 goal 形态：{pos, range} 或裸 RoomPosition（range 0）；负 range
      // 夹 0（官方 driver/lib/path-finder.js:106 Math.max(0, range|0)）
      const range = g0 && g0.pos && typeof g0.range === 'number'
        ? Math.max(0, g0.range) : 0;
      const path = straightPath(
        origin.pos ? origin.pos : origin,
        { x: to.x, y: to.y },
        range,
      );
      // 官方 ops 是搜索消耗计数（非空路径恒 ≥1，pf.cc:594）；直线桩以步数为近似
      return { path, ops: path.length, cost: path.length, incomplete: false };
    },
  };
  // PathFinder 全局绑定不可写，但其对象和方法没有因此冻结。
  Object.defineProperty(globalThis, 'PathFinder', {
    enumerable: true, value: PathFinder,
  });
})();
