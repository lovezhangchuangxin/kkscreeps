/* kk-runner M0 桩环境 prelude。
 *
 * 执行顺序（由 sandbox.rs 保证）：lodash 资产脚本 → 常量平铺脚本 → 本文件。
 * 本文件末尾遮蔽 globalThis.Deno（op 引用已在闭包中捕获）。
 *
 * 本文件是 M0 的桩环境（stub Game），服务于 bot 环境兼容性验收与 screeps-ok 适配器；
 * M2 起被真正的玩家 API 层（共享内存直读 + 惰性加载）取代。
 */
(() => {
  'use strict';

  const ops = globalThis.Deno && globalThis.Deno.core ? globalThis.Deno.core.ops : null;
  if (!ops || !ops.op_kk_module_kind) {
    throw new Error('kk-runner prelude: op 不可达（extension 未注册？）');
  }

  // ---------------------------------------------------------------- 缺失 API 探测
  // M0 环境验收的核心仪表：记录"玩家代码访问了但桩环境没有"的路径（含次数）。
  // get trap 覆盖属性访问/解构（GetV），has trap 覆盖 `in` 检查——`hasOwnProperty`/
  // Object.keys 枚举不触发 trap，属仪表的已知边界（枚举无法反推玩家想找什么）。

  const missing = new Map();
  function reportMissing(path) {
    missing.set(path, (missing.get(path) || 0) + 1);
  }
  const PROBE_IGNORE = new Set(['then', 'constructor', '__proto__', 'prototype', 'inspect', 'toJSON', 'nodeType', 'length', 'name']);
  function probe(target, basePath) {
    return new Proxy(target, {
      get(t, prop) {
        if (typeof prop === 'string' && !(prop in t) && !PROBE_IGNORE.has(prop)) {
          reportMissing(basePath + '.' + prop);
        }
        return t[prop];
      },
      has(t, prop) {
        // 含 ./[ 的"属性名"是 lodash 等库的路径探测串（如 isKey 对 'owner.username'
        // 整串做 in 检查），不是真实属性访问，不报仪表
        if (typeof prop === 'string' && !prop.includes('.') && !prop.includes('[')
          && !(prop in t) && !PROBE_IGNORE.has(prop)) {
          reportMissing(basePath + '.' + prop);
        }
        return prop in t;
      },
    });
  }

  // 桩方法调用计数：存在但不做事的官方方法（返回 OK），统计实际被用到的部分。
  const stubCalls = new Map();
  function countStub(name) {
    stubCalls.set(name, (stubCalls.get(name) || 0) + 1);
  }

  // ---------------------------------------------------------------- console
  // 收集型 console：经 op 推给宿主（替代 deno_core 自带的直写 stdout 版本）。
  // 套探测仪表：console.dir/trace 等官方缺口以 'console.*' 登记，不静默 undefined。

  function inspectValue(v, depth, seen) {
    if (v === null) return 'null';
    if (v === undefined) return 'undefined';
    const t = typeof v;
    if (t === 'string') return depth === 0 ? v : JSON.stringify(v);
    if (t === 'number' || t === 'boolean' || t === 'bigint') return String(v);
    if (t === 'function') return `[Function: ${v.name || 'anonymous'}]`;
    if (t === 'symbol') return v.toString();
    if (v instanceof Error) return `${v.name}: ${v.message}`;
    if (v instanceof DataView) return `DataView(${v.byteLength})`; // DataView 无 slice
    if (ArrayBuffer.isView(v)) return `${v.constructor.name}(${v.length}) [${Array.from(v.slice(0, 8)).join(', ')}${v.length > 8 ? ', …' : ''}]`;
    if (seen.has(v)) return '[Circular]';
    if (depth >= 3) return Array.isArray(v) ? '[Array]' : '{Object}';
    seen.add(v);
    try {
      if (Array.isArray(v)) {
        return `[ ${v.slice(0, 8).map((x) => inspectValue(x, depth + 1, seen)).join(', ')}${v.length > 8 ? ', …' : ''} ]`;
      }
      const keys = Object.keys(v).slice(0, 8);
      return `{ ${keys.map((k) => `${k}: ${inspectValue(v[k], depth + 1, seen)}`).join(', ')}${Object.keys(v).length > 8 ? ', …' : ''} }`;
    } finally {
      seen.delete(v);
    }
  }

  function fmtArgs(args) {
    return Array.prototype.map.call(args, (a) => inspectValue(a, 0, new Set())).join(' ');
  }
  const mkLevel = (level) =>
    function (...args) {
      ops.op_kk_console(level, fmtArgs(args));
    };
  globalThis.console = probe({
    log: mkLevel('log'),
    info: mkLevel('info'),
    warn: mkLevel('warn'),
    error: mkLevel('error'),
    debug: mkLevel('debug'),
  }, 'console');

  // ---------------------------------------------------------------- lodash
  // lodash 资产脚本已执行并把结果放在 globalThis.__kkLodash（CJS 导出或 UMD 根挂载）。
  const _ = globalThis.__kkLodash;
  delete globalThis.__kkLodash;
  if (typeof _ !== 'function' || typeof _.map !== 'function') {
    throw new Error('kk-runner prelude: lodash 预置失败');
  }
  globalThis._ = _;

  // ---------------------------------------------------------------- Buffer 垫片
  // 官方沙箱可用 Buffer（bot 生态依赖，overmind 的 wasm 模块经 Buffer 使用）。
  // M0 只实现常见子集；缺失方法由缺失 API 探测兜底报告。

  function utf8Encode(str) {
    const out = [];
    for (let i = 0; i < str.length; i++) {
      let c = str.codePointAt(i);
      if (c > 0xffff) i++;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return out;
  }
  function utf8Decode(bytes) {
    // 对齐官方 Buffer：非法/截断的 UTF-8 序列产出 U+FFFD 替换字符，而非抛 RangeError
    let s = '';
    let i = 0;
    const n = bytes.length;
    while (i < n) {
      const b = bytes[i];
      if (b < 0x80) { s += String.fromCharCode(b); i += 1; continue; }
      let size = 0;
      let cp = 0;
      if (b >= 0xc2 && b <= 0xdf) { size = 2; cp = b & 0x1f; } // 0xc0/0xc1 为 overlong，非法
      else if (b >= 0xe0 && b <= 0xef) { size = 3; cp = b & 0x0f; }
      else if (b >= 0xf0 && b <= 0xf4) { size = 4; cp = b & 0x07; }
      if (size === 0 || i + size > n) { s += '\ufffd'; i += 1; continue; } // 非法 leading / 截断
      let ok = true;
      for (let k = 1; k < size; k++) {
        const c = bytes[i + k];
        if ((c & 0xc0) !== 0x80) { ok = false; break; } // 非法 continuation
        cp = (cp << 6) | (c & 0x3f);
      }
      if (!ok || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff)) {
        s += '\ufffd';
        i += 1;
        continue;
      }
      s += String.fromCodePoint(cp);
      i += size;
    }
    return s;
  }

  const HEX_DIGITS = '0123456789abcdefABCDEF';
  function hexVal(ch) {
    const v = HEX_DIGITS.indexOf(ch);
    // 0-f 共 22 个合法字符，>9 时换算大写
    return v < 0 ? -1 : v % 16;
  }
  const BASE64_CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  function base64Val(ch) { return BASE64_CHARS.indexOf(ch); }

  class Buffer extends Uint8Array {
    static from(input, encoding) {
      if (typeof input === 'string') {
        const enc = encoding || 'utf8';
        let bytes;
        if (enc === 'utf8' || enc === 'utf-8') bytes = utf8Encode(input);
        else if (enc === 'hex') {
          if (input.length % 2 !== 0) throw new TypeError('Buffer.from(hex): 奇数长度');
          bytes = [];
          for (let i = 0; i < input.length; i += 2) {
            const hi = hexVal(input[i]), lo = hexVal(input[i + 1]);
            if (hi < 0 || lo < 0) throw new TypeError('Buffer.from(hex): 非法字符');
            bytes.push((hi << 4) | lo);
          }
        } else if (enc === 'base64') {
          const clean = input.replace(/\s/g, '');
          bytes = [];
          let buf = 0;
          let bits = 0;
          for (const ch of clean) {
            if (ch === '=') break;
            const v = base64Val(ch);
            if (v < 0) throw new TypeError('Buffer.from(base64): 非法字符');
            buf = (buf << 6) | v;
            bits += 6;
            if (bits >= 8) { bits -= 8; bytes.push((buf >> bits) & 0xff); }
          }
        } else {
          // 未支持的编码显式报错，不静默按 utf8 处理（wasm 工具链会拿到错误数据）
          throw new TypeError('Buffer.from: 不支持的 encoding: ' + enc);
        }
        const b = new Buffer(bytes.length);
        b.set(bytes);
        return b;
      }
      if (Array.isArray(input)) {
        const b = new Buffer(input.length);
        b.set(input);
        return b;
      }
      if (input instanceof Uint8Array) {
        const b = new Buffer(input.length);
        b.set(input);
        return b;
      }
      if (typeof input === 'object' && input !== null) {
        const keys = Object.keys(input);
        const b = new Buffer(keys.length);
        keys.forEach((k, idx) => { b[idx] = input[k]; });
        return b;
      }
      throw new TypeError('Buffer.from: 不支持的入参形态');
    }
    static alloc(size, fill, encoding) {
      const b = new Buffer(size);
      if (fill !== undefined) {
        if (typeof fill === 'string') {
          // 官方语义：字符串 fill 按其（按 encoding 编码的）字节循环填充，
          // 如 alloc(8,'x') → 8 个 120；alloc(2,'65') → [54,53]（字符 '6'/'5' 的字节）
          const f = Buffer.from(fill, encoding);
          if (f.length > 0) {
            for (let i = 0; i < size; i++) b[i] = f[i % f.length];
          }
        } else if (fill instanceof Uint8Array) {
          if (fill.length > 0) {
            for (let i = 0; i < size; i++) b[i] = fill[i % fill.length];
          }
        } else {
          b.fill(fill);
        }
      }
      return b;
    }
    static allocUnsafe(size) { return new Buffer(size); }
    static isBuffer(x) { return x instanceof Buffer; }
    static byteLength(s) {
      if (typeof s === 'string') return utf8Encode(s).length;
      if (s instanceof Uint8Array) return s.length;
      throw new TypeError('Buffer.byteLength: 不支持的入参');
    }
    static concat(list) {
      const total = list.reduce((n, b) => n + b.length, 0);
      const out = new Buffer(total);
      let off = 0;
      for (const b of list) { out.set(b, off); off += b.length; }
      return out;
    }
    toString(encoding) {
      // 官方默认 utf8；'hex'/'base64' 常被 wasm 工具链使用
      if (encoding === 'hex') {
        let s = '';
        for (const b of this) s += b.toString(16).padStart(2, '0');
        return s;
      }
      if (encoding === 'base64') {
        const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
        let s = '';
        for (let i = 0; i < this.length; i += 3) {
          const b0 = this[i], b1 = this[i + 1], b2 = this[i + 2];
          s += chars[b0 >> 2];
          s += chars[((b0 & 3) << 4) | ((b1 ?? 0) >> 4)];
          s += b1 === undefined ? '=' : chars[((b1 & 15) << 2) | ((b2 ?? 0) >> 6)];
          s += b2 === undefined ? '=' : chars[b2 & 63];
        }
        return s;
      }
      return utf8Decode(this);
    }
  }
  // Buffer 静态面套探测仪表：官方缺失的静态成员（如 poolAlloc）以 'Buffer.*' 登记
  globalThis.Buffer = probe(Buffer, 'Buffer');

  // ---------------------------------------------------------------- CommonJS 加载器
  // 官方形态：引擎自制 requireFn（engine/src/game/game.js），模块缓存跨 tick、
  // 循环 require 拿半成品 exports（**Node 语义**；官方为抛 "Circular reference to
  // module" 错——两者不一致，行为跟谁已在 compatibility.md 灰区表登记，M2 拍板）、
  // require.cache 预置 lodash。
  // 相对路径按真实规则解析（spike 垫片的扁平解析升级），二进制模块返回 Buffer。

  const moduleCache = Object.create(null);
  let currentModule = null;

  function dirname(name) {
    const i = name.lastIndexOf('/');
    return i < 0 ? '' : name.slice(0, i);
  }
  function normalizeRelative(spec, from) {
    const parts = (dirname(from) ? dirname(from) + '/' : '') + spec;
    const out = [];
    for (const p of parts.split('/')) {
      if (p === '' || p === '.') continue;
      if (p === '..') { out.pop(); continue; }
      out.push(p);
    }
    return out.join('/');
  }
  function resolveModuleName(spec, from) {
    let base = spec;
    if (spec.startsWith('./') || spec.startsWith('../')) {
      base = normalizeRelative(spec, from || 'main');
    }
    const candidates = [base];
    if (base.endsWith('.js')) candidates.push(base.slice(0, -3));
    else candidates.push(base + '.js');
    const uniq = [...new Set(candidates)];
    for (const c of uniq) {
      if (c === 'lodash') return c;
      if (ops.op_kk_module_kind(c) !== 'missing') return c;
    }
    throw new Error("Cannot find module '" + spec + "'");
  }

  function requireImpl(spec) {
    if (spec === 'lodash') return _;
    const name = resolveModuleName(spec, currentModule);
    const cached = moduleCache[name];
    if (cached) return cached.exports; // 循环 require 期间拿到半成品 exports —— Node 语义（官方为抛 Circular reference 错，见段首注释）
    const kind = ops.op_kk_module_kind(name);
    if (kind === 'binary') {
      // 官方语义：二进制（.wasm）模块 require 返回 Buffer
      const bytes = ops.op_kk_module_load_binary(name);
      const mod = { exports: Buffer.from(bytes) };
      moduleCache[name] = mod;
      return mod.exports;
    }
    const source = ops.op_kk_module_load_js(name);
    const module = { exports: {}, loaded: false };
    moduleCache[name] = module; // 执行前占坑 → 循环依赖语义
    const prev = currentModule;
    currentModule = name;
    try {
      const wrapper = new Function(
        'module', 'exports', 'require', 'global',
        source + '\n//# sourceURL=kk_module/' + name,
      );
      // this === module.exports（Node 语义；官方形态的差异留待 screeps-ok 契约用例对照）
      wrapper.call(module.exports, module, module.exports, requireImpl, globalThis);
      module.loaded = true;
    } catch (e) {
      // 执行失败删缓存再重抛（Node 原生与官方 game.js:578-581 同款）：
      // 否则瞬时初始化错误会永久复用半成品 exports，官方每 tick 重跑可自愈
      delete moduleCache[name];
      throw e;
    } finally {
      currentModule = prev;
    }
    return module.exports;
  }
  const require = requireImpl;
  require.cache = moduleCache;
  moduleCache.lodash = { exports: _ };
  globalThis.require = require;

  // ---------------------------------------------------------------- RawMemory / Memory
  // 官方语义：RawMemory 是 2MB 上限的原生存档字符串；Memory 是懒解析 getter，
  // 首次访问 JSON.parse(RawMemory.get())，tick 末写回。

  const RAW_MEMORY_LIMIT = 2 * 1024 * 1024;
  let rawMemoryData = '';
  let parsedMemory;
  const RawMemory = {
    get() { return rawMemoryData; },
    set(value) {
      const s = String(value);
      if (s.length > RAW_MEMORY_LIMIT) {
        throw new Error('RawMemory 超出 2MB 上限');
      }
      rawMemoryData = s;
    },
    segments: Object.create(null),
    setActiveSegments() { countStub('RawMemory.setActiveSegments'); return 0; },
    getActiveSegments() { countStub('RawMemory.getActiveSegments'); return []; },
    getSegment() { countStub('RawMemory.getSegment'); return ''; },
    setSegment() { countStub('RawMemory.setSegment'); return 0; },
    interShardSegment: null,
    setInterShardMemory() { countStub('RawMemory.setInterShardMemory'); return 0; },
    getInterShardMemory() { countStub('RawMemory.getInterShardMemory'); return ''; },
  };
  globalThis.RawMemory = RawMemory;

  function ensureMemory() {
    if (parsedMemory === undefined) {
      try {
        parsedMemory = rawMemoryData ? JSON.parse(rawMemoryData) : {};
      } catch (e) {
        console.error('Memory 解析失败，重置为空对象: ' + e.message);
        parsedMemory = {};
      }
      if (parsedMemory === null || typeof parsedMemory !== 'object') parsedMemory = {};
    }
    return parsedMemory;
  }
  function writeBackMemory() {
    if (parsedMemory !== undefined) {
      let s;
      try {
        s = JSON.stringify(parsedMemory);
      } catch (e) {
        // 玩家侧错误（Memory.big = 1n / 循环引用等不可序列化）：打 console、跳过写回
        // （保留上一 tick 存档），归类为玩家错误而非基础设施 Err——对齐 runExpr 的
        // kind:'serialization' 防护
        console.error('Memory 写回失败（不可序列化），保留上一 tick 存档: ' + (e && e.message));
        return;
      }
      if (s.length <= RAW_MEMORY_LIMIT) rawMemoryData = s;
    }
  }
  // Memory 作为懒 getter（官方语义：访问才解析）
  Object.defineProperty(globalThis, 'Memory', {
    configurable: true,
    get() { return ensureMemory(); },
  });

  // ---------------------------------------------------------------- RoomPosition
  // 官方全局类；纯计算方法给真实现，副作用方法为桩。

  class RoomPosition {
    constructor(x, y, roomName) {
      this.x = x;
      this.y = y;
      this.roomName = roomName;
    }
    get room() { return Game.rooms[this.roomName] || undefined; }
    isEqualTo(x, y) {
      const p = typeof x === 'object' ? x : { x, y };
      return this.x === p.x && this.y === p.y;
    }
    isNearTo(x, y) { return this.getRangeTo(x, y) <= 1; }
    isInRangeTo(x, y, range) {
      // 形态按原始第一参区分（官方 rooms.js:1362-1376）：三参 (x,y,range) 时 r 取
      // 第三参；两参 (pos, range) 时 r 取第二参。不能先包装成 p 再判（p 恒为对象）
      const p = typeof x === 'object' ? x : { x, y };
      const r = typeof x === 'object' ? y : range;
      return this.getRangeTo(p) <= r;
    }
    getRangeTo(x, y) {
      const p = typeof x === 'object' ? x : { x, y };
      // 官方 rooms.js:1588：仅目标带 roomName 且不等时才跨房间；数值形态（x,y 双参）
      // 视为同房间坐标
      if (p.roomName && this.roomName !== p.roomName) return Infinity; // 跨房间（M0 桩：不实现）
      return Math.max(Math.abs(this.x - p.x), Math.abs(this.y - p.y));
    }
    getDirectionTo(x, y) {
      const p = typeof x === 'object' ? x : { x, y };
      const dx = p.x - this.x, dy = p.y - this.y;
      if (dx === 0 && dy === 0) return 0;
      const dirs = [[0, -1, TOP], [1, -1, TOP_RIGHT], [1, 0, RIGHT], [1, 1, BOTTOM_RIGHT], [0, 1, BOTTOM], [-1, 1, BOTTOM_LEFT], [-1, 0, LEFT], [-1, -1, TOP_LEFT]];
      let best = dirs[0], bestDot = -Infinity;
      const len = Math.hypot(dx, dy) || 1;
      for (const d of dirs) {
        const dot = (d[0] * dx + d[1] * dy) / len / Math.hypot(d[0], d[1]);
        if (dot > bestDot) { bestDot = dot; best = d; }
      }
      return best[2];
    }
    findClosestByRange() { countStub('RoomPosition.findClosestByRange'); return null; }
    findClosestByPath() { countStub('RoomPosition.findClosestByPath'); return null; }
    findPathTo() { countStub('RoomPosition.findPathTo'); return []; }
    moveTo() { countStub('RoomPosition.moveTo'); return 0; }
    createConstructionSite() { countStub('RoomPosition.createConstructionSite'); return 0; }
    createFlag() { countStub('RoomPosition.createFlag'); return 'stub-flag'; }
    lookFor() { countStub('RoomPosition.lookFor'); return []; }
    inRangeTo() { return this.isInRangeTo.apply(this, arguments); }
  }
  globalThis.RoomPosition = RoomPosition;

  // 常量在 prelude 之前已平铺为裸全局（TOP/OK/FIND_* 等，官方形态）。

  // ---------------------------------------------------------------- PathFinder 桩
  // M0：直线步进 + incomplete:false 的假结果，保证 bot 流程可跑；
  // 真实现（FFI 官方 pf.cc，逐点一致）在 M2。

  class CostMatrix {
    constructor() { this._bits = new Uint8Array(2500); }
    set(x, y, v) { this._bits[y * 50 + x] = v; return this; }
    get(x, y) { return this._bits[y * 50 + x]; }
    copy() { const c = new CostMatrix(); c._bits.set(this._bits); return c; }
    serialize() { countStub('CostMatrix.serialize'); return { bits: Array.from(this._bits) }; }
    static deserialize() { countStub('CostMatrix.deserialize'); return new CostMatrix(); }
  }
  function straightPath(from, to) {
    const steps = [];
    let x = from.x, y = from.y;
    let guard = 0;
    while ((x !== to.x || y !== to.y) && guard++ < 100) {
      const dx = Math.sign(to.x - x), dy = Math.sign(to.y - y);
      steps.push({ dx, dy });
      x += dx; y += dy;
    }
    return steps;
  }
  const PathFinder = {
    CostMatrix,
    use() { countStub('PathFinder.use'); },
    search(origin, goal, opts) {
      countStub('PathFinder.search');
      const goals = Array.isArray(goal) ? goal : [goal];
      const to = goals[0] && goals[0].pos ? goals[0].pos : goals[0];
      const path = straightPath(
        origin.pos ? origin.pos : origin,
        { x: to.x, y: to.y },
      );
      return { path, ops: 0, cost: path.length, incomplete: false };
    },
  };
  globalThis.PathFinder = PathFinder;

  // ---------------------------------------------------------------- RoomVisual 桩

  class RoomVisual {
    constructor(roomName) { this.roomName = roomName; }
  }
  // 绘制方法全部 no-op：用 Proxy 原型惰性生成（class 的 prototype 不可写，故借普通函数桥接）
  const visualBridge = function VisualBridge() {};
  visualBridge.prototype = new Proxy({}, {
    get(t, prop) {
      if (prop in t) return t[prop];
      if (typeof prop === 'string') return function () { countStub('RoomVisual.' + prop); };
      return undefined;
    },
  });
  Object.setPrototypeOf(RoomVisual.prototype, visualBridge.prototype);
  globalThis.RoomVisual = RoomVisual;

  // ---------------------------------------------------------------- 类全局（官方全集）
  // 官方运行时把 41 个类挂成裸全局（engine/src/game 各文件 defineProperty(globals,...)），
  // 其中 Spawn、Energy 是别名（官方 structures.js:1293 `globals.Spawn=StructureSpawn`、
  // resources.js:46 `globals.Energy=Resource`）——这里在类注册后直接赋同一引用，
  // spawn 对象 instanceof Spawn 与官方一致。tooangel 等 bot 直接引用 Room 等。
  // M0：桩类——可 new、可静态引用（未知静态成员进缺失 API 探测）。
  // Game 上的对象经原型链接挂在类原型链上（见 wrapObject），instanceof 已成立；
  // 实例属性面的差异由 M2 玩家 API 层对齐。

  const GAME_CLASS_NAMES = [
    'ConstructionSite', 'Creep', 'Deposit', 'Flag', 'Mineral', 'Nuke',
    'OwnedStructure', 'PowerCreep', 'Resource', 'Ruin', 'Room', 'RoomObject',
    'Source', 'Store', 'Structure', 'StructureContainer', 'StructureController',
    'StructureExtension', 'StructureExtractor', 'StructureFactory', 'StructureInvaderCore',
    'StructureKeeperLair', 'StructureLab', 'StructureLink', 'StructureNuker',
    'StructureObserver', 'StructurePortal', 'StructurePowerBank', 'StructurePowerSpawn',
    'StructureRampart', 'StructureRoad', 'StructureSpawn', 'StructureStorage',
    'StructureTerminal', 'StructureTower', 'StructureWall', 'Tombstone',
  ];
  function makeStubClass(name) {
    const ctor = function () {
      countStub('new ' + name);
    };
    Object.defineProperty(ctor, 'name', { value: name });
    return new Proxy(ctor, {
      get(t, prop) {
        if (typeof prop === 'string' && !(prop in t) && !PROBE_IGNORE.has(prop)) {
          reportMissing(name + '.' + prop);
        }
        return t[prop];
      },
    });
  }
  for (const name of GAME_CLASS_NAMES) {
    if (globalThis[name] === undefined) globalThis[name] = makeStubClass(name);
  }
  // 官方类层级（RoomObject 为根；bot 生态跨层级打补丁，如 RoomObject.prototype.ref）：
  // Structure* 大多 extends OwnedStructure，wall/road/container/portal 直接 extends Structure
  const CLASS_PARENTS = {
    Creep: 'RoomObject', PowerCreep: 'RoomObject', Structure: 'RoomObject',
    OwnedStructure: 'Structure', Source: 'RoomObject', Mineral: 'RoomObject',
    Deposit: 'RoomObject', Resource: 'RoomObject', Tombstone: 'RoomObject',
    Ruin: 'RoomObject', Nuke: 'RoomObject', ConstructionSite: 'RoomObject',
    StructureContainer: 'Structure', StructureRoad: 'Structure',
    StructureWall: 'Structure', StructurePortal: 'Structure',
  };
  for (const [cls, parent] of Object.entries(CLASS_PARENTS)) {
    const c = globalThis[cls], p = globalThis[parent];
    if (c && p && c.prototype && p.prototype
      && Object.getPrototypeOf(c.prototype) === Object.prototype) {
      Object.setPrototypeOf(c.prototype, p.prototype);
    }
  }
  for (const name of GAME_CLASS_NAMES) {
    if (name.startsWith('Structure') && name !== 'Structure' && name !== 'OwnedStructure'
      && !CLASS_PARENTS[name]) {
      const c = globalThis[name], p = globalThis.OwnedStructure;
      if (c && p && Object.getPrototypeOf(c.prototype) === Object.prototype) {
        Object.setPrototypeOf(c.prototype, p.prototype);
      }
    }
  }
  // 官方别名：同一引用（structures.js:1293 / resources.js:46），非独立 stub 类
  globalThis.Spawn = globalThis.StructureSpawn;
  globalThis.Energy = globalThis.Resource;

  // ---------------------------------------------------------------- 世界投影（stub Game）

  let world = { time: 0, cpu: { limit: 20, tickLimit: 500, bucket: 10000 }, rooms: {}, objects: [] };
  let objectsById = new Map();

  function creepMemoryFor(name) {
    const m = ensureMemory();
    if (!m.creeps) m.creeps = {};
    if (!m.creeps[name]) m.creeps[name] = {};
    return m.creeps[name];
  }

  function mkStubMethod(label) {
    return function () {
      countStub(label);
      return 0; // OK
    };
  }
  const CREEP_METHODS = ['move', 'moveByPath', 'moveTo', 'harvest', 'build', 'repair', 'dismantle', 'upgradeController', 'attack', 'rangedAttack', 'rangedMassAttack', 'heal', 'rangedHeal', 'claimController', 'reserveController', 'attackController', 'transfer', 'withdraw', 'drop', 'pickup', 'say', 'suicide', 'spawn', 'generateSafeMode', 'pull', ' recycleCreep', 'renewCreep', 'cancelOrder', 'moveHead'];
  const SPAWN_METHODS = ['spawnCreep', 'renewCreep', 'recycleCreep', 'destroy', 'setActive', 'notifyWhenAttacked'];
  const STRUCTURE_METHODS = ['destroy', 'setActive', 'notifyWhenAttacked'];
  const CONTROLLER_METHODS = ['activateSafeMode'];
  const SOURCE_METHODS = [];

  function makeStore(storeSpec) {
    const store = Object.assign({}, storeSpec);
    store.getUsedCapacity = (resource) => storeSpec[resource || 'energy'] || 0;
    store.getFreeCapacity = () => 0;
    store.getCapacity = () => null;
    return new Proxy(store, {
      get(t, prop) {
        if (prop in t) return t[prop];
        if (typeof prop === 'string') reportMissing('store.' + prop);
        return undefined;
      },
    });
  }

  function wrapObject(o) {
    // pos 套探测仪表：RoomPosition 实例面的官方缺口（如 findInRange）以
    // 'RoomPosition.*' 登记，不静默 undefined
    const pos = probe(new RoomPosition(o.x, o.y, o.room), 'RoomPosition');
    const base = {
      id: o.id,
      pos,
      room: undefined, // 由 buildGame 回填（Game.rooms[name]）
      hits: o.hits ?? 0,
      hitsMax: o.hitsMax ?? 0,
    };
    let obj;
    switch (o.type) {
      case 'creep': {
        obj = Object.assign(base, {
          name: o.name,
          my: true,
          owner: { username: o.owner || 'bot' },
          body: (o.body || []).map((p) => (typeof p === 'string' ? { type: p, hits: 100 } : p)),
          fatigue: o.fatigue || 0,
          saying: '',
          spawning: false,
          store: makeStore(o.store || { energy: 0 }),
          _memoryName: o.name,
        });
        Object.defineProperty(obj, 'memory', { configurable: true, get: () => creepMemoryFor(o.name) });
        for (const m of CREEP_METHODS) obj[m.trim()] = mkStubMethod('creep.' + m.trim());
        break;
      }
      case 'spawn': {
        obj = Object.assign(base, {
          name: o.name,
          my: true,
          owner: { username: o.owner || 'bot' },
          structureType: o.structureType || 'spawn',
          store: makeStore(o.store || { energy: 0 }),
          energy: (o.store && o.store.energy) || 0,
          energyCapacity: o.energyCapacity || 300,
          spawning: null,
          isStructure: true,
        });
        obj.isActive = () => { countStub('structure.isActive'); return true; };
        for (const m of SPAWN_METHODS) obj[m] = mkStubMethod('spawn.' + m);
        break;
      }
      case 'structure': {
        obj = Object.assign(base, {
          structureType: o.structureType,
          my: true,
          owner: o.owner ? { username: o.owner } : undefined,
          isStructure: true,
          store: makeStore(o.store || {}),
        });
        if (o.structureType === 'controller') {
          obj.level = o.level || 0;
          obj.progress = o.progress || 0;
          obj.progressTotal = o.progressTotal || 0;
          obj.reservation = undefined;
          obj.safeMode = 0;
          obj.safeModeAvailable = 0;
          obj.safeModeCooldown = 0;
          obj.activationDate = undefined;
          for (const m of CONTROLLER_METHODS) obj[m] = mkStubMethod('controller.' + m);
        } else if (o.structureType === 'source') {
          obj.energy = o.energy ?? 3000;
          obj.energyCapacity = o.energyCapacity ?? 3000;
          obj.ticksToRegeneration = o.ticksToRegeneration ?? 0;
        } else if (o.structureType === 'mineral') {
          obj.mineralType = o.mineralType || 'XUH2O';
          obj.mineralAmount = o.mineralAmount ?? 0;
          obj.ticksToRegeneration = o.ticksToRegeneration ?? 0;
          obj.density = o.density ?? 3;
        } else {
          obj.energy = (o.store && o.store.energy) || 0;
          obj.energyCapacity = o.energyCapacity || 0;
          obj.isActive = () => { countStub('structure.isActive'); return true; };
          for (const m of STRUCTURE_METHODS) obj[m] = mkStubMethod('structure.' + m);
        }
        break;
      }
      case 'constructionSite': {
        obj = Object.assign(base, {
          structureType: o.structureType,
          progress: o.progress || 0,
          progressTotal: o.progressTotal || 100,
          my: true,
        });
        obj.remove = mkStubMethod('constructionSite.remove');
        break;
      }
      case 'flag': {
        obj = {
          name: o.name,
          color: o.color || 10,
          secondaryColor: o.secondaryColor || 10,
          pos,
          memory: flagMemoryFor(o.name),
        };
        obj.remove = mkStubMethod('flag.remove');
        obj.setPosition = mkStubMethod('flag.setPosition');
        obj.setColor = mkStubMethod('flag.setColor');
        break;
      }
      case 'resource': {
        obj = Object.assign(base, {
          resourceType: o.resourceType || 'energy',
          amount: o.amount || 0,
        });
        break;
      }
      default: {
        obj = base;
      }
    }
    const cls = classGlobalFor(o);
    if (cls && globalThis[cls] && globalThis[cls].prototype) {
      // 官方语义：Game 对象是类的实例；bot 生态普遍给官方原型打补丁
      //（tooangel 的 prototype_*.js 全系），原型链接通后补丁可见、instanceof 成立
      Object.setPrototypeOf(obj, globalThis[cls].prototype);
    }
    const name = o.name ? ':' + o.name : '';
    return probe(obj, (cls || o.type) + name);
  }

  function classGlobalFor(o) {
    switch (o.type) {
      case 'creep': return 'Creep';
      case 'spawn': return 'StructureSpawn';
      case 'flag': return 'Flag';
      case 'constructionSite': return 'ConstructionSite';
      case 'resource': return 'Resource';
      case 'tombstone': return 'Tombstone';
      case 'ruin': return 'Ruin';
      case 'deposit': return 'Deposit';
      case 'powerCreep': return 'PowerCreep';
      case 'nuke': return 'Nuke';
      case 'structure': {
        const m = { controller: 'StructureController', source: 'Source', mineral: 'Mineral',
          storage: 'StructureStorage', terminal: 'StructureTerminal', tower: 'StructureTower',
          extension: 'StructureExtension', link: 'StructureLink', lab: 'StructureLab',
          rampart: 'StructureRampart', road: 'StructureRoad', wall: 'StructureWall',
          container: 'StructureContainer', extractor: 'StructureExtractor',
          factory: 'StructureFactory', nuker: 'StructureNuker', observer: 'StructureObserver',
          powerBank: 'StructurePowerBank', powerSpawn: 'StructurePowerSpawn',
          portal: 'StructurePortal', invaderCore: 'StructureInvaderCore',
          keeperLair: 'StructureKeeperLair' };
        return m[o.structureType] || 'Structure';
      }
      default: return null;
    }
  }

  function flagMemoryFor(name) {
    const m = ensureMemory();
    if (!m.flags) m.flags = {};
    if (!m.flags[name]) m.flags[name] = {};
    return m.flags[name];
  }

  function buildGame() {
    objectsById = new Map();
    const creeps = {};
    const spawns = {};
    const structures = {};
    const constructionSites = {};
    const flags = {};
    const resources = {};
    for (const o of world.objects || []) {
      const w = wrapObject(o);
      w.room = undefined;
      objectsById.set(o.id, w);
      if (o.type === 'creep') creeps[o.name] = w;
      else if (o.type === 'spawn') spawns[o.name] = w;
      else if (o.type === 'flag') flags[o.name] = w;
      else if (o.type === 'constructionSite') constructionSites[o.id] = w;
      else if (o.type === 'resource') resources[o.id] = w;
      else if (o.type === 'structure') {
        // M0 桩：全部结构入 structures。官方语义（game.js:298-339）只收自家 owned 结构
        //（含 spawn、自家 controller）；source/mineral/road/wall/container 等不入——
        // M2 对齐，已登记 compatibility.md 灰区表
        structures[o.id] = w;
      }
    }
    const rooms = {};
    for (const [name, r] of Object.entries(world.rooms || {})) {
      const room = {
        name,
        controller: null,
        energyAvailable: 0,
        energyCapacityAvailable: 300,
        storage: undefined,
        terminal: undefined,
        visual: new RoomVisual(name),
        mode: 'world',
      };
      // controller 挂进房间
      for (const o of world.objects || []) {
        if (o.room === name && o.type === 'structure' && o.structureType === 'controller') {
          room.controller = objectsById.get(o.id) || null;
        }
      }
      room.find = function (type) {
        countStub('room.find:' + type);
        const out = [];
        for (const o of world.objects || []) {
          if (o.room !== name) continue;
          const w = objectsById.get(o.id);
          if (!w) continue;
          const isStruct = o.type === 'structure' || o.type === 'spawn';
          if (type === 101 /* FIND_CREEPS */ && o.type === 'creep') out.push(w);
          else if (type === 102 /* FIND_MY_CREEPS */ && o.type === 'creep') out.push(w);
          else if ((type === 104 /* FIND_SOURCES_ACTIVE */ || type === 105 /* FIND_SOURCES */) && o.type === 'structure' && o.structureType === 'source') out.push(w);
          else if (type === 106 /* FIND_DROPPED_RESOURCES */ && o.type === 'resource') out.push(w);
          else if (type === 107 /* FIND_STRUCTURES */ && isStruct) out.push(w);
          else if (type === 108 /* FIND_MY_STRUCTURES */ && isStruct) out.push(w);
          else if (type === 110 /* FIND_FLAGS */ && o.type === 'flag') out.push(w);
          else if (type === 111 /* FIND_CONSTRUCTION_SITES */ && o.type === 'constructionSite') out.push(w);
          else if (type === 112 /* FIND_MY_SPAWNS */ && o.type === 'spawn') out.push(w);
          else if (type === 116 /* FIND_MINERALS */ && o.type === 'structure' && o.structureType === 'mineral') out.push(w);
        }
        return out;
      };
      room.lookAt = function () { countStub('room.lookAt'); return []; };
      room.lookForAt = function () { countStub('room.lookForAt'); return []; };
      room.createConstructionSite = function () { countStub('room.createConstructionSite'); return 0; };
      room.createFlag = function () { countStub('room.createFlag'); return 'stub-flag'; };
      room.getPositionAt = (x, y) => new RoomPosition(x, y, name);
      room.serializePath = () => '';
      room.deserializePath = () => [];
      Object.defineProperty(room, 'memory', {
        configurable: true,
        get() {
          const m = ensureMemory();
          if (!m.rooms) m.rooms = {};
          if (!m.rooms[name]) m.rooms[name] = {};
          return m.rooms[name];
        },
      });
      if (globalThis.Room && globalThis.Room.prototype) {
        Object.setPrototypeOf(room, globalThis.Room.prototype);
      }
      rooms[name] = probe(room, 'Room[' + name + ']');
    }
    // 回填对象上的 room 引用
    for (const [id, w] of objectsById) {
      const orig = (world.objects || []).find((x) => x.id === id);
      if (orig && rooms[orig.room]) w.room = rooms[orig.room];
    }

    const cpu = world.cpu || { limit: 20, tickLimit: 500, bucket: 10000 };
    const game = {
      time: world.time,
      shard: { name: 'shard0', type: 'normal', ptr: false },
      cpu: {
        limit: cpu.limit,
        tickLimit: cpu.tickLimit,
        bucket: cpu.bucket,
        shardLimits: { shard0: cpu.limit },
        getUsed: () => ops.op_kk_get_used(),
        getHeapStatistics: () => { countStub('Game.cpu.getHeapStatistics'); return { total_heap_size: 0, used_heap_size: 0, heap_size_limit: 0, externally_allocated_size: 0, physical_total: 0, malloced_memory: 0, peak_malloced_memory: 0, does_zap_garbage: 0, number_of_native_contexts: 1, number_of_detached_contexts: 0 }; },
        halt: () => { countStub('Game.cpu.halt'); },
        setShardLimits: () => 0,
        generatePixel: () => { countStub('Game.cpu.generatePixel'); return -9; },
        unlock: () => -4,
      },
      gcl: { level: 1, progress: 0, progressTotal: 1000000 },
      gpl: { level: 0, progress: 0, progressTotal: 1000 },
      creeps,
      spawns,
      structures,
      constructionSites,
      flags,
      resources,
      rooms,
      market: {
        credits: 0,
        getAllOrders: () => { countStub('Game.market.getAllOrders'); return []; },
        orders: {},
        incomingTransactions: [],
        outgoingTransactions: [],
        calcTransactionCost: () => 1,
        createOrder: () => -4,
        cancelOrder: () => -4,
        extendOrder: () => -4,
        deal: () => -4,
        getHistory: () => [],
      },
      getObjectById(id) {
        return objectsById.get(id) || null;
      },
      notify() { countStub('Game.notify'); },
      map: {
        getWorldSize: () => 1,
        getRoomLinearDistance: () => 0,
        getRoomStatus: () => ({ status: 'normal', timestamp: null }),
        describeExits: () => ({}),
        findExit: () => -1,
        findRoute: () => [],
        getTerrainAt: () => 'p',
      },
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
    Object.defineProperty(globalThis, 'Game', {
      configurable: true,
      get: () => gameInstance,
    });
  }
  refreshGame();

  globalThis.__kk = {
    /** 每 tick 注入世界快照（宿主以 JSON 字面量内嵌，此处已是对象）并重建 Game。 */
    beginTick(worldJson) {
      world = typeof worldJson === 'string' ? JSON.parse(worldJson) : worldJson;
      parsedMemory = undefined; // 官方语义：Memory 每 tick 从 RawMemory 懒解析
      refreshGame();
    },
    /** 执行玩家入口：require('main').loop()。返回结果 JSON 字符串（宿主 exec_json 约定）。 */
    runLoop() {
      let out;
      try {
        const main = require('main');
        if (main && typeof main.loop === 'function') {
          main.loop();
          out = { ok: true };
        } else {
          out = { ok: false, kind: 'runtime', error: "main 模块未导出 loop()" };
        }
      } catch (e) {
        // 官方行为：未捕获的玩家错误打进 console（bot-check 据此采集每 tick 错误流）
        console.error(String((e && e.stack) || e));
        out = { ok: false, kind: 'runtime', error: String((e && e.stack) || e) };
      }
      try {
        return JSON.stringify(out);
      } catch (e) {
        return JSON.stringify({ ok: true, note: 'loop 完成（结果不可序列化）' });
      }
    },
    /** 求值一段玩家表达式代码，返回末表达式值（screeps-ok runPlayer 通道）。
     *  返回结果 JSON 字符串（宿主 exec_json 约定）。 */
    runExpr(code) {
      let out;
      let v;
      try {
        // 间接 eval（(0,eval)）在全局作用域求值——切断对 prelude 闭包（含捕获的
        // Deno.core.ops 引用等引擎内部状态）的访问；官方控制台命令同样在全局作用域
        // 求值（game.js:535-547），不得有超常特权（screeps-ok 契约）
        // eslint-disable-next-line no-eval
        v = (0, eval)(code);
      } catch (e) {
        // 区分语法错误与运行期错误：能否通过 Function 预解析
        let isSyntax = false;
        try { new Function(code); } catch (pe) { isSyntax = pe instanceof SyntaxError; }
        out = { ok: false, kind: isSyntax ? 'syntax' : 'runtime', error: String((e && e.message) || e) };
        return JSON.stringify(out);
      }
      if (v === undefined) {
        out = { ok: true, value: null };
      } else {
        try {
          out = { ok: true, value: JSON.parse(JSON.stringify(v)) };
        } catch (e) {
          out = { ok: false, kind: 'serialization', error: '返回值不可 JSON 序列化: ' + e.message };
        }
      }
      return JSON.stringify(out);
    },
    /** tick 末：Memory 写回 RawMemory。 */
    endTick() {
      writeBackMemory();
    },
    /** 清空 require 缓存（玩家代码更新/换模块表时）。lodash 预置保留。 */
    resetModules() {
      for (const k of Object.keys(moduleCache)) delete moduleCache[k];
      moduleCache.lodash = { exports: _ };
    },
    drainMissing() {
      const out = {};
      for (const [k, c] of missing) out[k] = c;
      missing.clear();
      return JSON.stringify(out);
    },
    drainStubCalls() {
      const out = {};
      for (const [k, c] of stubCalls) out[k] = c;
      stubCalls.clear();
      return JSON.stringify(out);
    },
  };
  // __kk 不可枚举（玩家代码不应该把它当 API 面）且整体冻结——防止玩家覆盖
  // drainMissing 造假验收仪表、把 beginTick/runLoop 换成抛错函数劫持宿主通道。
  // 已知残余：冻结防不住玩家直接调用 __kk.drainMissing() 抢先清空仪表，根治需
  // 宿主侧闭包/独立 op——M2 玩家 API 层重写，已登记 compatibility.md 灰区表。
  Object.freeze(globalThis.__kk);
  Object.defineProperty(globalThis, '__kk', { enumerable: false, writable: false, configurable: false });

  // ---------------------------------------------------------------- 遮蔽 deno_core 内部面
  // deno_core 的内部函数集合默认对玩家可见——ADR-002 纪律。
  // - Deno.core（约 100 个 op，含可一行 abort 宿主的 op_panic）；
  // - globalThis.__bootstrap：deno_core 0.412 的 bootstrap 命名空间，挂有与
  //   Deno.core 同一个 core 对象及 internals（00_infra.js:549 / 01_core.js:1301），
  //   只删 Deno 会漏掉它。
  // op 引用已捕获进闭包，遮蔽不影响宿主通道；遮蔽后的全局面复查
  //（getOwnPropertyNames 白名单比对）见 sandbox 测试——deno_core 升级若新增
  // 残留全局，测试会显形。
  delete globalThis.Deno;
  delete globalThis.__bootstrap;
})();
