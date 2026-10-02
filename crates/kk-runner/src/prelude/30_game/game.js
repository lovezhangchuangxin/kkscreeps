/* 管理 RawMemory/Memory、指令收集器和每 tick 的 Game 装配。
 * beginTick 注入世界并重建对象索引、Game 和 Memory 懒解析入口；writeBackMemory
 * 先序列化 Memory，再校验 segment 写入与请求，返回等待 Rust 协调器提交的候选。
 * 宿主函数从 __kkp.memoryWorld 捕获这些入口；私有状态由闭包引用跨 tick 保留。
 */
(() => {
  'use strict';

  const { ops, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 世界解码与 Memory 写回使用捕获的 JSON 函数；玩家读取 Memory 时仍使用全局 JSON.parse。
  const __jsonStringify = JSON.stringify;
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
  const __objectKeys = Object.keys;
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
  // 2B 批 P2：市场族常量加载期捕获（官方 market.js 闭包 C 同款）
  const C_MARKET_FEE = globalThis.MARKET_FEE;
  const C_MARKET_MAX_ORDERS = globalThis.MARKET_MAX_ORDERS;
  const C_ORDER_BUY = globalThis.ORDER_BUY;
  const C_ORDER_SELL = globalThis.ORDER_SELL;
  const C_INTERSHARD_RESOURCES = globalThis.INTERSHARD_RESOURCES;
  const C_RESOURCES_ALL = globalThis.RESOURCES_ALL;
  const C_RESOURCE_ENERGY_NAME = globalThis.RESOURCE_ENERGY;
  const C_ERR_NOT_OWNER = globalThis.ERR_NOT_OWNER;
  const C_ERR_TIRED_MKT = globalThis.ERR_TIRED;
  const C_ERR_NOT_ENOUGH_RESOURCES = globalThis.ERR_NOT_ENOUGH_RESOURCES;

  // 宿主消息使用 null 原型，避免继承 Object.prototype.toJSON 改写顶层结果。
  // 嵌套数组等对象另由 sanitizedStringify 处理。
  function nullEnv(props) {
    return __Object.assign(__Object.create(null), props);
  }
  gw.nullEnv = nullEnv;
  gw.__Object = __Object;

  // 地形载荷为 {rev,payload}，payload 是 2500 字节的 base64。
  // 自行解码以避免依赖 atob；相同 rev 的结果在 terrainCache 中复用。
  // 对应 Rust 编码器：kk-engine 的 world_json.rs。
  const B64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const b64Rev = new Uint8Array(128).fill(255);
  for (let i = 0; i < 64; i++) b64Rev[B64_ALPHABET.charCodeAt(i)] = i;
  function b64ToBytes(s) {
    if (s.length % 4 !== 0) throw new Error('bad base64 length: ' + s.length);
    // 反查表中没有映射的字符抛错，避免把这类错误载荷静默解码为地形。
    const rev = (ch) => {
      const v = b64Rev[ch & 127];
      if (v === 255) throw new Error('bad base64 char: ' + String.fromCharCode(ch));
      return v;
    };
    let len = (s.length >> 2) * 3;
    if (s.charCodeAt(s.length - 1) === 61) len--;
    if (s.charCodeAt(s.length - 2) === 61) len--;
    const out = new Uint8Array(len);
    let o = 0;
    for (let i = 0; i < s.length; i += 4) {
      const a = rev(s.charCodeAt(i));
      const b = rev(s.charCodeAt(i + 1));
      const cCh = s.charCodeAt(i + 2);
      const dCh = s.charCodeAt(i + 3);
      const c = cCh === 61 ? 0 : rev(cCh);
      const d = dCh === 61 ? 0 : rev(dCh);
      out[o++] = (a << 2) | (b >> 4);
      if (o < len) out[o++] = (b << 4) | (c >> 2);
      if (o < len) out[o++] = ((c & 3) << 6) | d;
    }
    return out;
  }
  gw.b64ToBytes = b64ToBytes;
  // 同一沙箱按房间名和 rev 缓存地形字节；rev 不变时不重新解码载荷。
  gw.terrainCache = {};

  // 序列化期间临时移除 Object.prototype 与 Array.prototype 的 toJSON。
  // 此操作不移除对象自己的 toJSON，也不能替代结构拷贝；取值和恢复仍可能执行玩家代码。
  // 只保存属性值并尽力恢复，不保证原属性描述符不变。冻结属性导致失败时由宿主出口捕获。
  function sanitizedStringify(v) {
    const OP = __Object.prototype;
    const AP = __Array.prototype;
    const so = OP.toJSON;
    const sa = AP.toJSON;
    // 不可配置属性删除失败时尝试赋 undefined；两者都失败则抛错，由宿主出口返回固定 JSON。
    const lift = (proto) => {
      try { delete proto.toJSON; } catch (_) {
        try { proto.toJSON = undefined; } catch (_) { throw new Error('根原型已冻结'); }
      }
    };
    const restore = (proto, saved) => {
      if (saved === undefined) return;
      try { proto.toJSON = saved; } catch (_) { /* 玩家自见面，尽力恢复 */ }
    };
    try {
      lift(OP);
      lift(AP);
      return __jsonStringify(v);
    } finally {
      restore(OP, so);
      restore(AP, sa);
    }
  }
  gw.sanitizedStringify = sanitizedStringify;

  // ---------------------------------------------------------------- RawMemory / Memory
  // 官方语义（driver/lib/runtime/runtime.js:102-137 + engine/src/game/game.js:479-500）：
  // RawMemory 是 null 原型对象（get/set/segments 等均为不可配置 value 描述符），
  // 2MB 门只在 set；Memory 是每 tick 重装的懒 getter，首访用**可污染的全局
  // JSON.parse** 解析（官方 bundle 跑在玩家 realm，同形）、成功对象顶层
  // `__proto__ = null`、失败置 null 且不写回；写回读 `RawMemory._parsed`
  // （玩家可见可删——删掉即跳过保存，memhack 011）、用**预置** stringify
  // （官方 systemFunctions.JSON.stringify，runtime.js:246——parse 可污染与
  // 写回预置的不对称是官方原样）。

  const RAW_MEMORY_LIMIT = 2 * 1024 * 1024;
  let rawMemoryData = '';
  // 每 tick 新建 RawMemory，使 _parsed 只属于当前 tick；rawMemoryData 字符串跨 tick 保留。
  let RawMemory;
  // 完整 segment 内容由协调器 PlayerState 保存；这里仅持有本 tick 注入的活动窗口。
  // 请求变量跨 tick 保留，由 buildSegmentsOut 编码；active=[] 表示清空，public=[] 不带出请求。
  // 来源：上游 driver/lib/runtime/runtime.js 的请求输出及 make.js 的持久状态更新。
  let segmentsWindow = __Object.create(null);
  let foreignSegmentData;
  let activeSegmentsReq = null;
  let publicSegmentsReq = null;
  let defaultPublicSegmentReq;
  let foreignSegmentReq;
  /** 官方 setActiveSegments 等四 API 的 id 校验公共段（runtime.js:130-184：
   * `_.isArray` 门 + parseInt + NaN/越界 0..99 → throw——**throw 非 return**，
   * 玩家侧 runtime error 同 tick 可 catch）。 */
  function parseSegmentIds(ids) {
    if (!lodash.isArray(ids)) {
      throw new Error('"' + ids + '" is not an array');
    }
    const out = [];
    for (let i = 0; i < ids.length; i++) {
      const id = parseInt(ids[i]);
      if (isNaN(id) || id > 99 || id < 0) {
        throw new Error('"' + ids[i] + '" is not a valid segment ID');
      }
      out.push(id);
    }
    return out;
  }

  function makeRawMemory() {
    // 官方 runtime.js:192-199：每 tick 把 data.memorySegments 拷入新建
    // null 原型对象（注入窗口的内容快照——玩家写它、tick 尾回收）
    const segs = __Object.create(null);
    const entries = __objectEntries(segmentsWindow);
    for (let i = 0; i < entries.length; i++) segs[entries[i][0]] = entries[i][1];
    const props = {
      get: {
        value() { return rawMemoryData; },
      },
      set: {
        value(v) {
          // 官方 runtime.js:110 用私有 lodash 的 _.isString（String 包装对象也
          // 过门）；此处用捕获的 lodash 同域
          if (!lodash.isString(v)) {
            throw new Error('Raw memory value is not a string');
          }
          if (v.length > RAW_MEMORY_LIMIT) {
            throw new Error('Raw memory length exceeded 2 MB limit');
          }
          // 官方 runtime.js:116-118：带 _parsed 门——set 即失效本 tick 的解析
          // 缓存，否则 tick 末写回会用陈旧 parse 覆写玩家刚 set 的 raw
          if (this._parsed) delete this._parsed;
          rawMemoryData = v;
        },
      },
      segments: { value: segs },
      interShardSegment: { value: '', writable: true },
      // segment 请求通过下列四个方法记录，内容由宿主在下一 tick 注入。
      setActiveSegments: {
        value(ids) {
          const parsed = parseSegmentIds(ids);
          if (parsed.length > 10) {
            throw new Error('Only 10 memory segments can be active at the same time');
          }
          activeSegmentsReq = parsed;
        },
      },
      setPublicSegments: {
        value(ids) {
          publicSegmentsReq = parseSegmentIds(ids);
        },
      },
      setDefaultPublicSegment: {
        value(id) {
          if (id !== null) {
            id = parseInt(id);
            if (isNaN(id) || id > 99 || id < 0) {
              throw new Error('"' + id + '" is not a valid segment ID');
            }
          }
          defaultPublicSegmentReq = id;
        },
      },
      setActiveForeignSegment: {
        value(username, id) {
          if (username === null) {
            foreignSegmentReq = null;
            return;
          }
          if (id !== undefined) {
            id = parseInt(id);
            if (isNaN(id) || id > 99 || id < 0) {
              throw new Error('"' + id + '" is not a valid segment ID');
            }
          }
          foreignSegmentReq = { username, id };
        },
      },
    };
    // foreignSegment 只在宿主提供数据时出现，以普通赋值设置为可写、可枚举、可配置属性。
    // 来源：上游 driver/lib/runtime/runtime.js 的 foreignMemorySegment 装配。
    const rm = __Object.create(null, props);
    if (foreignSegmentData !== undefined) {
      const fs = __Object.create(null);
      const fEntries = __objectEntries(foreignSegmentData);
      for (let i = 0; i < fEntries.length; i++) fs[fEntries[i][0]] = fEntries[i][1];
      rm.foreignSegment = fs;
    }
    return rm;
  }
  RawMemory = makeRawMemory();
  globalThis.RawMemory = RawMemory;

  function safeSegErrorText(e) {
    try {
      return String((e && e.message) || e).slice(0, 200);
    } catch (_) {
      return 'unprintable segment error';
    }
  }

  function ensureMemory() {
    // 官方 game.js:479-497：无条件重解析（玩家预置 _parsed 会被覆盖，memhack
    // 005）；解析用全局 JSON.parse——官方 bundle 在玩家 realm 内同形（自害面，
    // 与世界注入的预置 parse 不同源，见段首注释）
    let parsed;
    try {
      parsed = JSON.parse(rawMemoryData || '{}');
      if (parsed !== null && typeof parsed === 'object') parsed.__proto__ = null;
    } catch (e) {
      parsed = null;
    }
    RawMemory._parsed = parsed;
    // 官方 game.js:492-495：首访后固化为值属性——官方**没写** writable（默认
    // false）：`Memory = {...}` 官方静默失败（严格模式抛错）；descriptor 与
    // 值（含 null/原始值原样保留）均官方同形
    __Object.defineProperty(globalThis, 'Memory', {
      configurable: true,
      enumerable: true,
      value: parsed,
    });
    return parsed;
  }
  gw.ensureMemory = ensureMemory;

  // per-object memory 的官方形态（creeps.js:95-119 / rooms.js:551 / flags.js:31 /
  // structures.js:865-885）：每次访问**重读** `Memory.<subKey>[name]`（"tick 内
  // 钉住"是 Memory 本身首访固化的涌现，不在对象上缓存）；undefined 与字符串
  // 'undefined'（遗留病态）都落默认 {}；非对象返回 undefined（getter）/抛错
  // （setter）；Memory 为 null 时 `m[subKey]` 抛 TypeError 原样传播（官方
  // sloppy 同样抛）。判定用捕获语义的 _.isObject 同域（object/function 且非
  // null）——官方读全局 `_` 可污染，此处为登记过的加固分歧
  function isObjectLike(v) {
    return v !== null && (typeof v === 'object' || typeof v === 'function');
  }
  gw.memoryFor = function memoryFor(subKey, name) {
    const m = globalThis.Memory; // 懒触发；null → 下一行抛 TypeError（官方同形）
    let sub = m[subKey];
    if (sub === undefined || sub === 'undefined') {
      if (isObjectLike(m)) m[subKey] = {}; // Memory 为原始值时官方 sloppy 赋值静默无效
      sub = m[subKey];
    }
    if (!isObjectLike(sub)) return undefined;
    return (sub[name] = sub[name] || {});
  };
  gw.setMemoryFor = function setMemoryFor(subKey, name, value, label) {
    const m = globalThis.Memory;
    let sub = m[subKey];
    if (sub === undefined || sub === 'undefined') {
      if (isObjectLike(m)) m[subKey] = {};
      sub = m[subKey];
    }
    if (!isObjectLike(sub)) throw new Error('Could not set ' + label + ' memory');
    sub[name] = value;
  };

  /** 宿主注入通道（setMemoryState）：本 tick 的活动窗口内容与 foreignSegment
   * （官方 data.memorySegments / data.foreignMemorySegment 同位——begin_tick
   * 之前注入，协调器从 PlayerState 解算）。 */
  function setSegmentsWindowHost(entriesJson) {
    segmentsWindow = __Object.create(null);
    const entries = __jsonParse(entriesJson);
    if (lodash.isArray(entries)) {
      for (let i = 0; i < entries.length; i++) {
        segmentsWindow['' + entries[i][0]] = entries[i][1];
      }
    }
  }
  function setForeignSegmentHost(fsJson) {
    const v = __jsonParse(fsJson);
    foreignSegmentData = v === null || v === undefined ? undefined : v;
  }

  /** 校验并生成 segment 写入与请求候选。
   * 最多 10 槽，id 为 0–99，内容须为字符串且不超过 100×1024 个 UTF-16 码元。
   * 请求保留最近记录的值；public 空列表不带出，active 空列表表示清空。 */
  let segmentsOutStash;
  function buildSegmentsOut() {
    // 先取键并检查数量，再逐键读取类型、长度和最终内容，总共三次读取值。
    // 不能用 Object.entries 提前读取全部值，否则会改变 getter 次数与错误优先级。
    // 来源：上游 driver/lib/runtime/runtime.js 的 memorySegments 写回。
    const segs = RawMemory.segments;
    const segmentKeys = __objectKeys(segs);
    const writes = [];
    if (segmentKeys.length > 0) {
      if (segmentKeys.length > 10) {
        throw new Error('Cannot save more than 10 memory segments on the same tick');
      }
      for (let i = 0; i < segmentKeys.length; i++) {
        const id = parseInt(segmentKeys[i]);
        if (isNaN(id) || id > 99 || id < 0) {
          throw new Error('"' + segmentKeys[i] + '" is not a valid memory segment ID');
        }
        if (typeof segs[segmentKeys[i]] !== 'string') {
          throw new Error('Memory segment #' + segmentKeys[i] + ' is not a string');
        }
        if (segs[segmentKeys[i]].length > 100 * 1024) {
          throw new Error('Memory segment #' + segmentKeys[i] + ' has exceeded 100 KB length limit');
        }
        writes.push([id, '' + segs[segmentKeys[i]]]);
      }
    }
    const out = __Object.create(null);
    out.writes = writes;
    // 已记录的 active 请求始终带出，空数组表示清空活动窗口；null 表示尚无记录。
    if (activeSegmentsReq !== null) out.active = activeSegmentsReq;
    // public 的 [] 是 no-op（官方 join(',') 得 '' 被 truthy-check 弃——与
    // active 的 [] 清空是两个不同的官方形状，勿合并）
    if (publicSegmentsReq && publicSegmentsReq.length) out.public = publicSegmentsReq;
    if (defaultPublicSegmentReq !== undefined) {
      out.defaultPublic = defaultPublicSegmentReq === null ? null : defaultPublicSegmentReq;
    }
    if (foreignSegmentReq !== undefined) {
      out.foreign = foreignSegmentReq === null ? null : {
        username: foreignSegmentReq.username,
        id: foreignSegmentReq.id === undefined ? null : foreignSegmentReq.id,
      };
    }
    return out;
  }

  function writeBackMemory() {
    // 先序列化 Memory，再读取 segments 与请求；Memory.toJSON 中的 segment 修改因此本 tick 生效。
    // 删除 _parsed 只跳过 Memory 保存，不能跳过 segment 写回。两阶段都成功后才更新 rawMemoryData。
    // 普通序列化错误返回 ok:false，避免被 Rust 当作硬中断而按完整预算计费。
    segmentsOutStash = null;
    // ① Memory 序列化（官方 :246-248：读 `RawMemory._parsed`——玩家可见可删，
    // 删掉即跳过保存；写回无 2MB 门（超限只拦 set）；序列化用预置 stringify）
    let memoryPending = null;
    const parsed = RawMemory._parsed;
    if (parsed) {
      try {
        const s = __jsonStringify(parsed);
        if (typeof s !== 'string') {
          // Memory.toJSON 返回非字符串时 stringify 得 undefined——玩家侧写回失败
          // （官方顶层 throw、无玩家 console 行：错误只在 run 结局信封可见）
          return nullEnv({ ok: false, error: 'Memory.toJSON 返回了非字符串' });
        }
        memoryPending = s;
      } catch (e) {
        // 玩家异常对象的 message 可是污染 getter（抛错逃逸会被宿主误判硬中断）
        let why;
        try { why = String((e && e.message) || e); } catch (_) { why = 'unprintable player error'; }
        return nullEnv({ ok: false, error: 'Memory 不可序列化: ' + why });
      }
    }
    // ② segments tick 尾回收（官方 :250-269——在 Memory 序列化**之后**；违例
    // throw 与官方同义：整 tick 写回失败，已算好的 Memory 候选一并丢弃）
    try {
      segmentsOutStash = buildSegmentsOut();
    } catch (e) {
      return nullEnv({ ok: false, error: 'Segment 写回失败: ' + safeSegErrorText(e) });
    }
    if (memoryPending !== null) rawMemoryData = memoryPending;
    return nullEnv({ ok: true });
  }
  // Memory 作为懒 getter（官方语义：访问才解析）。官方 game.js:470-473 两处
  // defineProperty 均 enumerable: true（初始 getter 与首访固化后的 value 属性）
  __Object.defineProperty(globalThis, 'Memory', {
    configurable: true,
    enumerable: true,
    get() { return ensureMemory(); },
  });

  // ---------------------------------------------------------------- Game 装配

  const { probe, lodash } = globalThis.__kkp;
  // 加载时捕获 CJS 钩子；运行时 __kkp 已被 Rust 删除，不能再从全局查找。
  const { ensureRequire, setCurrentUser } = globalThis.__kkp.cjs;

  // 市场数据来自当前查看者的世界数据，价格从毫 credit 转为 credit。
  // 订单、历史和交易结果在本 tick 首次查询时复制并缓存；订单动作记录 global 指令。
  function makeMarket() {
    const view = gw.world.market || {};
    const txView = view.transactions || {};
    let _incoming;
    let _outgoing;
    let _orders;
    // 官方 :205-233：transactionId=""+_id（投影 id 已为串）、sender/
    // recipient→{username} 或缺键（undefined 序列化丢键——两侧同形）
    function mapTransactions(list) {
      return (list || []).map((i) => {
        const out = {
          transactionId: '' + i.id,
          time: i.time,
          resourceType: i.resourceType,
          amount: i.amount,
          from: i.from,
          to: i.to,
        };
        if (i.description !== undefined) out.description = i.description;
        if (i.order !== undefined) out.order = i.order;
        if (i.sender !== undefined) out.sender = { username: i.sender };
        if (i.recipient !== undefined) out.recipient = { username: i.recipient };
        return out;
      });
    }
    // 订单簿原始投影（毫价——÷1000 在视图层，官方 _getOrders/orders getter
    ///deal API 的单位分工同位：deal 读 raw 毫价、视图读 credits）
    const ordersView = view.orders || {};
    const myOrdersRaw = view.my_orders || [];
    const historyView = view.history || { all: [] };
    // 官方 make 闭包缓存（market.js:10-11——每 tick 对象重建=每 tick 首读
    // 快照；P6 惰性缓存同款纪律）
    const cachedOrders = {};
    const cachedHistory = {};
    let ordersCreatedDuringTick = 0;
    // 官方 :13-27 _getOrders：falsy→'all'；非法资源名 {} 不缓存；深拷贝+
    // price÷1000
    function _getOrders(resourceType) {
      if (!resourceType) resourceType = 'all';
      if (!cachedOrders[resourceType]) {
        if (resourceType !== 'all' && !lodash.contains(C_RESOURCES_ALL, resourceType)
          && !lodash.contains(C_INTERSHARD_RESOURCES, resourceType)) {
          return {};
        }
        const raw = ordersView[resourceType] || {};
        const copy = __jsonParse(__jsonStringify(raw));
        for (const i in copy) {
          copy[i].price /= 1000;
        }
        cachedOrders[resourceType] = copy;
      }
      return cachedOrders[resourceType];
    }
    // 官方 deal API 的 terminal 门（:126 `_.find(userObjects, {type:'terminal',
    // room})`——自家 terminal；room.terminal 快捷面首个【同房多 terminal 官方
    // any() 任一、我方首个+my 过滤，混主夹具态登记】）
    function myTerminalIn(roomName) {
      const room = Game.rooms[roomName];
      return room && room.terminal && room.terminal.my ? gw.rawData(room.terminal.id) : null;
    }
    const market = {
      // 官方 :31-34：calcRoomsDistance(continuous) + calcTerminalEnergyCost
      //（utils.js:644-659）=ceil(amount×(1−e^(−range/30)))。**不环绕**：
      // 官方 MMO 经 getWorldSize 环绕取 min(size−d,d)；我方单机世界 plain
      // Chebyshev（与 Rust settle/market.rs calc_rooms_distance 同判登记，
      // liveWorldSize 能力键 false）。畸形房名：gw.roomNameToXY 是官方
      // utils.js:374-390 的宽松逐字移植——畸形名产 NaN 不抛错 → 距离/成本
      // NaN（number 返回，两侧同函数同形）
      calcTransactionCost(amount, roomName1, roomName2) {
        const xy1 = gw.roomNameToXY(roomName1);
        const xy2 = gw.roomNameToXY(roomName2);
        const dx = Math.abs(xy2[0] - xy1[0]);
        const dy = Math.abs(xy2[1] - xy1[1]);
        return Math.ceil(amount * (1 - Math.exp(-Math.max(dx, dy) / 30)));
      },
      // 官方 :36-39：_getOrders 快照 + lodash.filter（filter 缺省=全量）
      getAllOrders(filter) {
        const orders = _getOrders(filter && filter.resourceType);
        return lodash.filter(orders, filter);
      },
      // 官方 :41-53：非法资源名 {}；'all'→[]（driver 恒空初始化）；
      // 合法资源无数据 {} **对象非数组**（MARKET-QUERY-006 官方同红的根源）
      getHistory(resourceType) {
        if (!resourceType) resourceType = 'all';
        if (!cachedHistory[resourceType]) {
          if (resourceType !== 'all' && !lodash.contains(C_RESOURCES_ALL, resourceType)
            && !lodash.contains(C_INTERSHARD_RESOURCES, resourceType)) {
            return {};
          }
          cachedHistory[resourceType] = __jsonParse(__jsonStringify(historyView[resourceType] || {}));
        }
        return cachedHistory[resourceType];
      },
      // 官方 :55-66：先自己的（this.orders 已 ÷1000）→ 公共 active 池（÷1000）
      //→ null。this.orders 触发 orders getter 惰性缓存（官方同形）
      getOrderById(id) {
        if (this.orders[id]) {
          return __jsonParse(__jsonStringify(this.orders[id]));
        }
        const order = ordersView.all[id];
        if (order) {
          const result = __jsonParse(__jsonStringify(order));
          result.price /= 1000;
          return result;
        }
        return null;
      },
      // 官方 :68-98：对象/参数双形 → 资源门 → type 门 → parseFloat/parseInt →
      // 正数门 → fee>credits 门（credits 单位）→ terminal 门（intershard 免）
      //→ 数量门（size(orders)+本 tick 已建 ≥300）→ pushByName 无 maxLen
      createOrder(type, resourceType, price, totalAmount, roomName) {
        if (typeof type === 'object' && type !== null) {
          ({ type, resourceType, price, totalAmount, roomName } = type);
        }
        if (!lodash.contains(C_RESOURCES_ALL, resourceType)
          && !lodash.contains(C_INTERSHARD_RESOURCES, resourceType)) {
          return C_ERR_INVALID_ARGS;
        }
        if (type !== C_ORDER_BUY && type !== C_ORDER_SELL) {
          return C_ERR_INVALID_ARGS;
        }
        price = parseFloat(price);
        totalAmount = parseInt(totalAmount);
        if (!price || price <= 0 || !totalAmount) {
          return C_ERR_INVALID_ARGS;
        }
        if (price * totalAmount * C_MARKET_FEE > this.credits) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        if (!lodash.contains(C_INTERSHARD_RESOURCES, resourceType)
          && (!roomName || !myTerminalIn(roomName))) {
          return C_ERR_NOT_OWNER;
        }
        if (lodash.size(this.orders) + ordersCreatedDuringTick >= C_MARKET_MAX_ORDERS) {
          return C_ERR_FULL;
        }
        ordersCreatedDuringTick++;
        gw.intents.pushByName('global', 'createOrder', {
          type, resourceType, price, totalAmount, roomName,
        });
        return C_OK;
      },
      // 官方 :100-106：自己的订单门 → pushByName(50)（返回值忽略恒 OK）
      cancelOrder(orderId) {
        if (!this.orders[orderId]) {
          return C_ERR_INVALID_ARGS;
        }
        gw.intents.pushByName('global', 'cancelOrder', { orderId }, 50);
        return C_OK;
      },
      // 官方 :108-153：公共 active 池 **raw 毫价**；intershard 资源面门 /
      // 自家 terminal 门（transferCost=calcTransactionCost）+ 能量面门 +
      // 冷却门 + BUY 资源面门 + SELL money 门（毫×量 vs 毫）→ pushByName(10)
      // 满 ERR_FULL
      deal(orderId, amount, targetRoomName) {
        const order = ordersView.all[orderId];
        if (!order) {
          return C_ERR_INVALID_ARGS;
        }
        amount = parseInt(amount);
        if (!amount || amount < 0) {
          return C_ERR_INVALID_ARGS;
        }
        if (lodash.contains(C_INTERSHARD_RESOURCES, order.resourceType)) {
          if (order.type === C_ORDER_BUY
            && ((gw.world.market && gw.world.market.resources
              ? gw.world.market.resources[order.resourceType] : 0) || 0) < amount) {
            return C_ERR_NOT_ENOUGH_RESOURCES;
          }
        } else {
          if (!targetRoomName) {
            return C_ERR_INVALID_ARGS;
          }
          const t = myTerminalIn(targetRoomName);
          const transferCost = this.calcTransactionCost(amount, targetRoomName, order.roomName);
          if (!t) {
            return C_ERR_NOT_OWNER;
          }
          // 此检查把缺失 energy 当作 0；上游 engine/src/game/market.js 的 deal
          // 直接比较 store.energy，缺失时 undefined < cost 为 false，返回码可能不同。
          if (!t.store || (t.store[C_RESOURCE_ENERGY_NAME] || 0) < transferCost) {
            return C_ERR_NOT_ENOUGH_RESOURCES;
          }
          if (t.cooldownTime > gw.world.time) {
            return C_ERR_TIRED_MKT;
          }
          if (order.type === C_ORDER_BUY) {
            if ((order.resourceType !== C_RESOURCE_ENERGY_NAME
              && (!t.store[order.resourceType] || t.store[order.resourceType] < amount))
              || (order.resourceType === C_RESOURCE_ENERGY_NAME
                && t.store[C_RESOURCE_ENERGY_NAME] < amount + transferCost)) {
              return C_ERR_NOT_ENOUGH_RESOURCES;
            }
          }
        }
        if (order.type === C_ORDER_SELL && (view.money_milli || 0) < amount * order.price) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        if (!gw.intents.pushByName('global', 'deal', { orderId, targetRoomName, amount }, 10)) {
          return C_ERR_FULL;
        }
        return C_OK;
      },
      // 官方 :155-172：自己的订单 + parseFloat + 提价差额费（credits 单位，
      // order.price 为视图 credits 价）→ pushByName(50)
      changeOrderPrice(orderId, newPrice) {
        const order = this.orders[orderId];
        if (!order) {
          return C_ERR_INVALID_ARGS;
        }
        newPrice = parseFloat(newPrice);
        if (!newPrice || newPrice <= 0) {
          return C_ERR_INVALID_ARGS;
        }
        if (newPrice > order.price
          && (newPrice - order.price) * order.remainingAmount * C_MARKET_FEE > this.credits) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        gw.intents.pushByName('global', 'changeOrderPrice', { orderId, newPrice }, 50);
        return C_OK;
      },
      // 官方 :174-191：自己的订单 + parseInt + fee 门 → pushByName(50)
      extendOrder(orderId, addAmount) {
        const order = this.orders[orderId];
        if (!order) {
          return C_ERR_INVALID_ARGS;
        }
        addAmount = parseInt(addAmount);
        if (!addAmount || addAmount <= 0) {
          return C_ERR_INVALID_ARGS;
        }
        if (order.price * addAmount * C_MARKET_FEE > this.credits) {
          return C_ERR_NOT_ENOUGH_RESOURCES;
        }
        gw.intents.pushByName('global', 'extendOrder', { orderId, addAmount }, 50);
        return C_OK;
      },
    };
    __Object.defineProperties(market, {
      // 官方 :196-201：`(user.money||0)/1000`——毫 credits ÷1000（注入面给
      // 毫原值，除法在 getter=官方同位）
      credits: {
        enumerable: true,
        get() { return (view.money_milli || 0) / 1000; },
      },
      incomingTransactions: {
        enumerable: true,
        get() {
          if (!_incoming) _incoming = mapTransactions(txView.incoming);
          return _incoming;
        },
      },
      outgoingTransactions: {
        enumerable: true,
        get() {
          if (!_outgoing) _outgoing = mapTransactions(txView.outgoing);
          return _outgoing;
        },
      },
      // 官方 :235-249：myOrders map——id 串化（投影已串）、删 user、
      // price÷1000、indexBy('id')；惰性闭包缓存（官方 _orders）
      orders: {
        enumerable: true,
        get() {
          if (!_orders) {
            _orders = lodash.indexBy(myOrdersRaw.map((i) => {
              const o = __jsonParse(__jsonStringify(i));
              delete o.user;
              o.price /= 1000;
              return o;
            }), 'id');
          }
          return _orders;
        },
      },
    });
    return market;
  }

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
        // 旗帜与工地不授予房间视野，即使带有本人的 owner。
        // 来源：上游 driver/lib/runtime/data.js 的房间视野归集。
        const own = o.owner !== undefined ? o.owner : o.user;
        if (own === gw.viewerName && o.type !== 'constructionSite' && o.type !== 'flag') {
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
    setRawMemoryHost(data) {
      rawMemoryData = data;
    },
    /** 读取当前 RawMemory 数据作为待提交候选；真正保存到 PlayerState 由 Rust 协调器完成。 */
    rawMemoryDataHost() {
      return rawMemoryData;
    },
    /** 设置活动 segment 窗口，下一次 beginTick 将内容复制到新的 RawMemory.segments。 */
    setSegmentsWindowHost,
    /** 宿主私有通道（P6）：注入本 tick 的 foreignSegment（官方
     *  data.foreignMemorySegment——null 串表示无）。 */
    setForeignSegmentHost,
    /** 宿主私有通道（P6）：tick 尾 segments 信封（writeBackMemory 成功打包的
     *  写回+四请求快照；endTick 随 rawMemory 一并带出——失败轮为 null）。 */
    segmentsOutHost() {
      return segmentsOutStash;
    },
    /** 每 tick 注入世界快照（宿主经私有句柄以字符串参数传入——下方双分支
     * 兼容 string/对象两种形态）并重建 Game。 */
    beginTick(worldJson, cpuMeta) {
      beginTickImpl(worldJson, cpuMeta, null);
    },
    /** KKPV 通道注入：worldJson 是不含对象数组的世界信封，对象记录由
     * 可信解码器从每 isolate 私有拷贝的块束（ArrayBuffer）解出。装配后
     * 与 JSON 通道的等同面：gw.world.objects 的成员、数组顺序（块房
     * 序×ID 序）、字段值与缺失形态、Store/嵌套记录的键序（载荷保序）；
     * 两者共用同一段 beginTickImpl 后续步骤。 */
    beginTickKkpv(worldJson, bundle, cpuMeta) {
      beginTickImpl(worldJson, cpuMeta, bundle);
    },
    writeBackMemory,
    intentsPayload,
  };

  /** beginTick/beginTickKkpv 的共用实现；bundle 非 null 时对象数组来自
   * KKPV 块束（信封不含 objects 键），其余步骤两条通道完全一致。 */
  function beginTickImpl(worldJson, cpuMeta, bundle) {
      // 使用捕获的 JSON.parse 解码，避免玩家替换全局函数后改写世界与用户身份。
      gw.world = typeof worldJson === 'string' ? __jsonParse(worldJson) : worldJson;
      if (bundle !== null && bundle !== undefined) {
        // 块头 tick 与信封 time 在解码器内交叉校验（同一投影的同界快照）。
        gw.world.objects = gw.kkpv.decodeBundle(bundle, gw.world.time);
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
      RawMemory = makeRawMemory();
      globalThis.RawMemory = RawMemory;
      // 官方 game.js:504-512：require 每 tick 重绑四条件判定（缺失/timestamp
      // 变/main 非对象/main.loop 非函数 → 整体重绑）；moduleObject.user 注入
      ensureRequire();
      setCurrentUser(gw.viewerName);
      __Object.defineProperty(globalThis, 'Memory', {
        configurable: true,
        enumerable: true,
        get() { return ensureMemory(); },
      });
      gw.terrainByRoom = {};
      // 新 tick 开始时丢弃上一 tick 的绘图缓冲区。
      if (gw.visualReset) gw.visualReset();
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
          gw.terrainByRoom[roomName] = gw.terrainCache[roomName].bytes;
        } else if (__Array.isArray(t)) {
          // 旧数组形态（宿主手搓注入的夹具路径——bot_check 桩世界等；真
          // 路径已不产出），原样接收
          gw.terrainByRoom[roomName] = t;
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
