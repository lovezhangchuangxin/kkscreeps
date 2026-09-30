/* kk-runner prelude · 30_game 段 8：PathFinder 桩（对照官方
 * engine/src/game/path-finder.js + driver/lib/path-finder.js）。
 *
 * M0：直线步进 + incomplete:false 的假结果，保证 bot 流程可跑；
 * 真实现（FFI 官方 pf.cc，逐点一致）的玩家面接线随 M2 API 批（C1/C2 成果）。
 * 依赖：RoomPosition（全局，room-position 段已挂）。
 */
(() => {
  'use strict';

  const { countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 官方 CostMatrix（path-finder.js:18-47）逐字：_bits 是公开 own 面
  //（Uint8Array(2500)，索引 **x*50+y**——UNDOC-COSTMATRIX 面）；set 钳
  // 0..255（|0 强转）；clone 复制位图；serialize 把 2500 字节 buffer 按
  // Uint32 视图切成 **625 元素普通数组**（官方 `Array.prototype.slice.apply(
  // new Uint32Array(this._bits.buffer))` 的打包怪形——四格一组小端序）；
  // deserialize 经 Uint32Array(data).buffer 还原（不走构造器——Object.create
  // 直接挂 _bits）。M2B 扩面 P6 真化（原桩索引反置+{bits} 形态全非官方）。
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
  // 官方 path-finder.js:78 同为非可写 defineProperty（清偿复审 P3-N1）
  Object.defineProperty(globalThis, 'PathFinder', {
    enumerable: true, value: PathFinder,
  });
})();
