/* RawMemory/Memory 懒解析、segment 注入与 tick 尾候选写回。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const __jsonStringify = JSON.stringify;
  const __jsonParse = JSON.parse;
  const __Object = Object;
  const __Array = Array;
  const __objectEntries = Object.entries;
  const __objectKeys = Object.keys;
  const __Set = Set;
  const __reflectApply = Reflect.apply;
  const { lodash } = globalThis.__kkp;

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
      try {
        delete proto.toJSON;
      } catch (_) {
        try {
          proto.toJSON = undefined;
        } catch (_) {
          throw new Error('根原型已冻结');
        }
      }
    };
    const restore = (proto, saved) => {
      if (saved === undefined) return;
      try {
        proto.toJSON = saved;
      } catch (_) {
        /* 玩家自见面，尽力恢复 */
      }
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
        value() {
          return rawMemoryData;
        },
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
          residentParsed = undefined;
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

  // Memory 常驻缓存：writeBack 成功 stringify 后记录 (解析对象, 字符串)
  // 成对快照；下一拍首访时注入字符串未变则直接复用对象，跳过官方每拍
  // 全量 JSON.parse（xxscreeps mods/meta/memory/memory.ts 的 previousJson
  // 同路）。复用使对象跨拍保持同一性——依赖每拍新对象的 bot 在官方即坏，
  // 接受面与 xxscreeps 一致（套件绿）。RawMemory.set 与宿主注入不同字符
  // 串都会失效本对。
  let residentParsed = undefined;
  let residentString = undefined;

  // Memory 洁度走查：一遍 O(n) 无分配深走，既做官方 JSON 往返的归一化
  // （数组 undefined 槽转 null、对象 undefined 键删除），又判定对象是否
  // 与 parse(stringify(obj)) 逐值一致——任何 stringify 丢弃/改写的值形态
  // 都判脏：函数/Symbol/bigint、NaN/±Infinity/-0、非普通原型（Date/Map/
  // 类实例——stringify 会把它们变成别的东西）、环（官方 stringify 抛错）、
  // 不可枚举自有键与 Symbol 键（stringify 不带走）、数组命名键与稀疏洞。
  // 干净才允许常驻复用；脏则作废常驻对，下一拍走完整 parse（官方同形）。
  // 已知接受的残余差异（xxscreeps crunch 同样存在、套件无覆盖）：访问器
  // 属性在走查与 stringify 中各读一次（官方只读一次）。
  const __numberIsFinite = Number.isFinite;
  const __objectIs = Object.is;
  const __objectGetPrototypeOf = Object.getPrototypeOf;
  const __objectGetOwnPropertyNames = Object.getOwnPropertyNames;
  const __objectGetOwnPropertySymbols = Object.getOwnPropertySymbols;
  const __setHas = Set.prototype.has;
  const __setAdd = Set.prototype.add;
  const OBJECT_PROTOTYPE = __Object.prototype;
  function memoryCleanWalk(payload, seen) {
    if (payload === null) return true;
    const t = typeof payload;
    if (t === 'boolean' || t === 'string') return true;
    if (t === 'number') {
      return __numberIsFinite(payload) && !__objectIs(payload, -0);
    }
    if (t !== 'object') return false;
    if (__reflectApply(__setHas, seen, [payload])) return false;
    __reflectApply(__setAdd, seen, [payload]);
    if (__Array.isArray(payload)) {
      const names = __objectGetOwnPropertyNames(payload);
      if (names.length !== payload.length) return false;
      for (let i = 0; i < payload.length; i++) {
        const slot = payload[i];
        if (slot === undefined) payload[i] = null;
        else if (!memoryCleanWalk(slot, seen)) return false;
      }
      return true;
    }
    if (__objectGetPrototypeOf(payload) !== OBJECT_PROTOTYPE &&
        __objectGetPrototypeOf(payload) !== null) {
      return false;
    }
    const keys = __objectKeys(payload);
    if (__objectGetOwnPropertyNames(payload).length !== keys.length) return false;
    if (__objectGetOwnPropertySymbols(payload).length !== 0) return false;
    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      const v = payload[k];
      if (v === undefined) delete payload[k];
      else if (!memoryCleanWalk(v, seen)) return false;
    }
    return true;
  }

  function ensureMemory() {
    // 官方 game.js:479-497：无条件重解析（玩家预置 _parsed 会被覆盖，memhack
    // 005）；解析用全局 JSON.parse——官方 bundle 在玩家 realm 内同形（自害面，
    // 与世界注入的预置 parse 不同源，见段首注释）。常驻命中时跳过解析，
    // 未命中走官方同形路径。
    let parsed;
    if (residentParsed !== undefined && rawMemoryData === residentString) {
      parsed = residentParsed;
    } else {
      try {
        parsed = JSON.parse(rawMemoryData || '{}');
        if (parsed !== null && typeof parsed === 'object') parsed.__proto__ = null;
      } catch (e) {
        parsed = null;
      }
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

  // InterShardMemory 本地段：官方后端专有 API（OSS 引擎无此模块），语义
  // 按套件契约——每玩家字符串、≤100×1024 个 UTF-16 码元、仅 setLocal 写入。
  // getRemote 需要多 shard 宿主，本片不安装（未定义即调用报 TypeError）。
  let interShardLocal = null;
  let interShardDirty = false;

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

  // InterShardMemory（官方后端 API）：非字符串与超限抛错（segment 保持
  // 旧值——套件契约"拒绝不留痕"）；同 tick round-trip 读闭包现行值。
  __Object.defineProperty(globalThis, 'InterShardMemory', {
    enumerable: true,
    configurable: true,
    value: {
      getLocal() {
        return interShardLocal;
      },
      setLocal(value) {
        if (typeof value !== 'string') {
          throw new Error('The value to InterShardMemory.setLocal must be a string');
        }
        if (value.length > 100 * 1024) {
          throw new Error('InterShardMemory segment has exceeded 100 KB length limit');
        }
        interShardLocal = value;
        interShardDirty = true;
      },
    },
  });

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
          throw new Error(
            'Memory segment #' + segmentKeys[i] + ' has exceeded 100 KB length limit',
          );
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
      out.foreign =
        foreignSegmentReq === null
          ? null
          : {
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
        const clean = memoryCleanWalk(parsed, new __Set());
        const s = __jsonStringify(parsed);
        if (typeof s !== 'string') {
          // Memory.toJSON 返回非字符串时 stringify 得 undefined——玩家侧写回失败
          // （官方顶层 throw、无玩家 console 行：错误只在 run 结局信封可见）；
          // 走查已做的归一化保留（对玩家而言只是无返回值的普通变异）
          residentParsed = undefined;
          return nullEnv({ ok: false, error: 'Memory.toJSON 返回了非字符串' });
        }
        memoryPending = s;
        if (clean) {
          residentParsed = parsed;
          residentString = s;
        } else {
          residentParsed = undefined;
        }
      } catch (e) {
        residentParsed = undefined;
        // 玩家异常对象的 message 可是污染 getter（抛错逃逸会被宿主误判硬中断）
        let why;
        try {
          why = String((e && e.message) || e);
        } catch (_) {
          why = 'unprintable player error';
        }
        return nullEnv({ ok: false, error: 'Memory 不可序列化: ' + why });
      }
    }
    // ② segments tick 尾回收（官方 :250-269——在 Memory 序列化**之后**；违例
    // throw 与官方同义：整 tick 写回失败，已算好的 Memory 候选一并丢弃）
    if (parsed) {
      // 常驻对已在上方成功路径或各失败路径定论
    } else {
      // 未访问或 _parsed 被删：保存跳过，但玩家可能仍持有常驻对象引用并
      // 已/将变异它——作废常驻对，下一拍重新 parse 注入串（官方丢弃语义）。
      residentParsed = undefined;
    }
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
    get() {
      return ensureMemory();
    },
  });

  // Rust 捕获这些入口后删除 __kkp；Game 只持有闭包函数，不在 tick 中重查临时全局。
  globalThis.__kkp.memory = {
    setRawMemoryHost(data) {
      if (data !== rawMemoryData) {
        // 宿主注入不同字符串（重试轮回放旧候选、恢复、管理编辑）——常驻
        // 对失效，下一拍走完整 parse；相同字符串（正常提交回注）保持复用。
        rawMemoryData = data;
        residentParsed = undefined;
      }
    },
    setIsmHost(value) {
      interShardLocal = value;
      interShardDirty = false;
    },
    ismLocalHost() {
      return interShardLocal;
    },
    ismDirtyHost() {
      return interShardDirty;
    },
    rawMemoryDataHost() {
      return rawMemoryData;
    },
    setSegmentsWindowHost,
    setForeignSegmentHost,
    segmentsOutHost() {
      return segmentsOutStash;
    },
    writeBackMemory,
    resetRawMemoryHost() {
      RawMemory = makeRawMemory();
      globalThis.RawMemory = RawMemory;
    },
    installMemoryHost() {
      __Object.defineProperty(globalThis, 'Memory', {
        configurable: true,
        enumerable: true,
        get() {
          return ensureMemory();
        },
      });
    },
  };
})();
