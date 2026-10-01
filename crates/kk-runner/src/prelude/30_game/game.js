/* kk-runner prelude · 30_game 段 9：Game 装配与 tick 生命周期（对照官方
 * engine/src/game/game.js + driver/lib/runtime/runtime.js 的收集器）。
 *
 * RawMemory/Memory（官方 game.js:470-495 懒解析语义）、intents 收集器
 * （官方 runtime.js:60-100 同形闭包私有）、buildGame（Game 装配序，官方
 * game.js:273-345 collections）、refreshGame 与 __kkp.memoryWorld 导出
 * （beginTick/writeBackMemory/intentsPayload——90_host 宿主通道消费）。
 * 依赖：gw 全容器（_registry）、makeRoom/Terrain/terrainWordAt（room 段）、
 * roomNameToXY/getRoomNameFromXY（room-position 段——map 族房名算术）、
 * wrapObject（_registry 段）、各类型 assembler（已注册）。
 */
(() => {
  'use strict';

  const { ops, countStub } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;

  // 预置 JSON 函数（官方 systemFunctions.JSON 同款）：Memory 写回/世界解码等
  // 宿主关键序列化不经玩家可覆写的全局（外部复审 P1 修复面；自审 P2 补 parse）
  const __jsonStringify = JSON.stringify;
  const __jsonParse = JSON.parse;
  // 预置 Object（官方 systemFunctions.Object 同位，runtime.js 头部捕获）：
  // 宿主路径的对象构造不得经可交换的 globalThis.Object（对抗评审 P1 实证：
  // 交换 Object 使 nullEnv 产出真原型对象，null-proto 防线整体绕过）
  const __Object = Object;
  const __Array = Array;
  // 预置构造器/方法捕获（A3c 装配批，同 __Object 原则）：buildGame/beginTick
  // 每 tick 在玩家 realm 内执行，上一 tick 的全局/原型交换会让自家装配崩或
  // 视野失真（官方引擎 bundle 私有 lodash + 闭包捕获内建——我方等价物）
  const __Map = Map;
  const __Set = Set;
  const __mapGet = Map.prototype.get;
  const __mapSet = Map.prototype.set;
  const __setAdd = Set.prototype.add;
  const __setHas = Set.prototype.has;
  // Reflect.apply 加载期捕获（B0 WP4 .call 家族清偿：.call 动态分派走可
  // 交换的 Function.prototype.call——玩家可换，A6 对抗评审 P2 登记的家族债）
  const __reflectApply = Reflect.apply;
  const __objectEntries = Object.entries;
  const __objectKeys = Object.keys;
  const mathPow = Math.pow;
  const mathFloor = Math.floor;
  // M2B 扩面 P3：map/notify/cpu 族常量加载期捕获（官方闭包 C 同款）+
  // halt 宿主通道（00_shared 暴露，ops.op_kk_cpu_halt——官方 _halt 是 ivm
  // 引用注入 user-vm.js:68-70，同位等价物）
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

  // null 原型信封（官方 runtime.js:214 `outMessage = systemFunctions.Object.create(null)`
  // 同款）：宿主消息对象不得携带 Object.prototype——继承的 toJSON 会让预置
  // stringify 在序列化时被原型污染改写载荷（自审 P1：一条 defineProperty 可
  // 完整重放"伪造 intentsList 造工地"原探针；官方靠 null-proto + ExternalCopy
  // 出口双重免疫，我方出口是 stringify，null-proto 是等价结构位之一）
  function nullEnv(props) {
    return __Object.assign(__Object.create(null), props);
  }
  gw.nullEnv = nullEnv;
  gw.__Object = __Object;

  // 地形紧凑载荷解码器（M2B 前置批 P3）：注入协议的 terrain 值为
  // {rev, payload(base64 2500B)}——宿主侧按纪元缓存（地形首玩家 tick 后
  // 不可变，稳态零解码）。解码器为自包含实现（无 atob 依赖：V8 裸环境
  // 不保证宿主扩展）；反查表加载期构建一次。与 Rust 侧 world_json.rs
  // base64_encode 对表（Rust 测试 test_b64_decode 孪生锁定）。
  const B64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const b64Rev = new Uint8Array(128).fill(255);
  for (let i = 0; i < 64; i++) b64Rev[B64_ALPHABET.charCodeAt(i)] = i;
  function b64ToBytes(s) {
    if (s.length % 4 !== 0) throw new Error('bad base64 length: ' + s.length);
    // 非法字符 fail-fast（评审清偿）：静默产垃圾字节会让宿主 bug 以错误
    // 地形扩散——抛错即响亮失败（真路径 Rust 恒发合法串，此为防御纵深）
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
  // 地形解码缓存（跨 tick）：rev 命中复用 Uint8Array。沙箱与分片同寿命
  // （create_shard 重建沙箱 ⇒ 缓存自然归空），无跨分片陈旧面
  gw.terrainCache = {};

  // 出口净化序列化（对抗评审 P1/P2 的根本闸）：序列化窗口内摘除两个根原型的
  // toJSON（含数组——stringify 对数组自身也查 toJSON，pushByName 数组通道同
  // 面潜伏）——官方出口是 ExternalCopy 结构拷贝，任何玩家 toJSON 都不参与
  // 宿主消息（runtime.js:296），摘除=官方等价。save/restore 保持玩家 tick 内
  // 的原型状态不变（玩家的 toJSON 只在自己的序列化场景可见——如 Memory 写回
  // 官方就调用，systemFunctions.JSON.stringify 是原生语义）。净化期间的异常
  // （freeze 原型使 delete 抛错等）一律上抛由出口 catch 归 ok:false
  function sanitizedStringify(v) {
    const OP = __Object.prototype;
    const AP = __Array.prototype;
    const so = OP.toJSON;
    const sa = AP.toJSON;
    // 摘除容错（对抗评审 P1）：configurable:false 的污染使 delete 抛 TypeError
    // ——降级赋 undefined（不可调用即不参与序列化）；freeze 使两者皆抛时上抛，
    // 由出口 catch 的字面量信封兜底（绝不得逃逸为硬中断）
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
  // 官方 runtime.js:102 rawMemory 是 _start 函数体局部变量、make.js:160 每 tick
  // 重调——对象每 tick 新建，`_parsed` 生命周期严格单 tick（毒 parse 不跨 tick
  // 残留；评审 P1 纠偏：此前持久对象导致写回失败后下一 tick 持续报错，非官方
  // 同形）。字符串本体 rawMemoryData 跨 tick 持久（官方 data.userMemory.data）
  let RawMemory;
  // M2B 扩面 P6：segments 全链（官方 runtime.js:122-200 API + runtime.js:250-269
  // tick 尾回收）。**后端在协调器 PlayerState**（全量 100 槽存储+请求面）——
  // prelude 只持有本 tick 注入的**活动窗口**（≤10 槽，宿主 setMemoryState 通道
  // 注入，官方 data.memorySegments 同位）与 foreignSegment（官方
  // data.foreignMemorySegment）。四请求闭包变量跨 tick 持久——官方闭包其实
  // **每 tick 重置**（make 侧每 tick 重跑 _start，runtime.js 顶部 let 全部归
  // undefined），持久化全靠 db（users.activeSegments 等）+「本 tick 调用过才
  // emit」双要素；我方闭包持久 + 引擎 PlayerState 兜底 = 官方 db 兜底的等价
  // 落点（评审 2 P2-2 勘正：勿按「官方闭包持久」类推新语义——emit 门形状
  // 才是权威：active 无条件带出（[] 清空）、public 经 join(',') 空串被
  // truthy-check 弃（[] no-op），两者形状不同）；tick 尾经 writeBackMemory
  // 的 segmentsOut 信封带出
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
      // 官方 runtime.js:129-148（M2B 扩面 P6 真化——原 countStub 桩面摘除；
      // 官方 RawMemory 无 getActiveSegments/getSegment/setSegment/
      // setInterShardMemory/getInterShardMemory 面——bot 实测零消费后撤）
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
    // 官方 runtime.js:196-199：foreignSegment 属性只在 data.foreignMemorySegment
    // 存在时出现（null 原型对象 {username,id,data} 的浅拷贝），且在**建对象
    // 之后普通赋值**挂载——writable/enumerable/configurable 三 true（props
    // 描述符挂载三 false 与官方描述符面相异，外部复审 P2 清偿）；未请求/
    // 未授权/未知名 → 整个属性 undefined
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

  /** tick 尾 segments 信封（官方 runtime.js:250-292 + outMessage 四请求字段）：
   * 窗口写回（校验 ≤10 槽/0-99/字符串/≤100KB——违例按官方 throw 同义走
   * {ok:false} 整 tick 失败）+ 四请求闭包值**全量带出**（官方每 tick emit
   * 同形；publicSegments 空列表丢弃——官方 truthy-check 弃空串 quirk，
   * suite FOREIGN-008 注释实证）。 */
  let segmentsOutStash;
  function buildSegmentsOut() {
    // 官方 runtime.js:250-269 惰性序（外部复审 P2-3 清偿）：先 Object.keys
    // （不触发 getter）+ 数量门，再逐 key **三次读**（typeof/长度/终值）——
    // getter 调用次数、入库值（末次读）与异常优先级（数量超限先于任何
    // getter 执行）与官方逐位同形。Object.entries 快照会先读全部值（getter
    // 提前执行+首值入库+11 个抛错 getter 时 getter 异常先于数量门）——已弃。
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
    // active 无条件带出（null=本 tick 未调；[] 是合法的清空请求——官方
    // runtime.js:287 无守卫，[] truthy 经 make 侧 $set 清库；评审 1/2 P1 清偿）
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
    // 官方 runtime.js 执行序（外部复审 P1 清偿）：先 :246-248 序列化 Memory
    // （stringify 跑玩家 toJSON 回调——回调内的 segments 写与四请求**对本
    // tick 生效**），再 :250-269 回收校验 segments，请求最后随信封带出。
    // `_parsed` 门只支配 Memory 保存——官方段回收与请求带出**不受该门支配**
    // （玩家删 _parsed 仍保存段与请求；我方原 `!parsed` 早退连请求一起丢，
    // 同族缺陷一并清偿）。任一步失败整轮回退（官方 throw 于 outMessage
    // 填充中途、段/请求/Memory 俱失——同形）。
    // 返回 {ok} | {ok:false,error}：screeps-ok 契约（adapter-spec:229）把 tick 末
    // 序列化失败归为 runPlayer 的 runtime 错误；玩家侧写回错误（不可序列化 /
    // Memory.toJSON 返回非字符串等）一律走这里，绝不抛出——抛出会被宿主误判
    // 为硬中断并按整份 tickLimit 计费。
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
  // cjs 钩子必须加载期捕获（__kkp 在 90_host 末段删除——运行期延迟查找是
  // A3a 坑① 的固定形态）
  const { ensureRequire, setCurrentUser } = globalThis.__kkp.cjs;

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
          // 官方 spawn 先经 structureTypes 表进 structures、再进 spawns
          //（game.js:298-303 structures 收一切本人 OwnedStructure，spawn 是其
          // 子类 structures.js:854）——两集合同时收录（自审 P3 修复：
          // 原只进 spawns 漏 structures）
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
    // 可见房（官方 game.js:218-220 只注册 runtimeData.rooms）：查看者**非工地
    /// 非旗**自有对象所在房（对齐官方归集跳过表 data.js:113-116——对抗评审
    // P2：自家工地经 world 破例导出进 objects 后，此处若不排除会把工地所在
    // 房带回 Game.rooms，"工地不授视野"在房间存在性层被击穿）；工地全集经
    // Game.constructionSites 收录（官方 data.js:103 + game.js:366-374），两语义正交
    // 可见房用 Rust 显式传递的集合（GPT 复审 P1-1 清偿：官方 data.js:103-116
    // 服务端归集 runtimeData.rooms，JS 不从对象字段重算——原 o.owner 推断
    // 会漏投影键为 user 的墓碑/废墟，仅剩它们时整房从 Game.rooms 消失）。
    // 键缺失（手写测试世界不经 world_json 生成）回退对象推断——推断分支
    // 同时认 owner/user 两键（tombstone/ruin 投影是 user）
    const visibleRooms = new __Set();
    const visibleList = gw.world.visibleRooms;
    if (visibleList !== undefined && visibleList !== null) {
      for (let i = 0; i < visibleList.length; i++) {
        __reflectApply(__setAdd, visibleRooms, [visibleList[i]]);
      }
    } else {
      for (let i = 0; i < worldObjs.length; i++) {
        const o = worldObjs[i];
        // 归集跳过表（data.js:113-116）：flag/工地不授房视野（M2B P2 清偿：
        // 旗有 owner 后此回退分支须显式跳过——主路径 Rust 侧过滤同款）
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
    // 回填对象上的 room 引用：rawById 直查替代数组 .find（O(N²)→O(N)，
    // 官方 byRoom 索引同构；.find 经可交换原型，装配路径不得经——A3c 装配批）
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
        // 官方 game.js:153-155：宿主注入（ivm getHeapStatisticsSync——
        // snake_case 键面，overmind/tooangel 均按此消费）。我方=begin_tick
        // 时点在宿主控制线程采样的真 V8 统计（deno_core v8_isolate），
        // 非调用时实时（登记：快照时点差异）；每次调用返回浅拷贝（官方
        // 每次新建对象）。externally_allocated_size=ivm 外部分配追踪面，
        // 我方无对应通道恒 0（登记）
        getHeapStatistics: () => ({ ...cpu.heap }),
        // 官方 user-vm.js:68-70：_halt 置 didHaltByUserRequest + isolate
        // dispose（硬停不可捕获）；我方 terminate 同为硬停（登记：沙箱
        // 长寿命不销毁，计费按实际用量）
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
        return __reflectApply(__mapGet, gw.objectsById, [id]) || null;
      },
      // 官方 game.js:174-180：全局 notify intent（maxLen 20——每 tick 上限，
      // runtime.js:78-86 push 通道；满返 ERR_FULL）。落库面=玩家通知记录
      //（官方 users.notifications；我方 PlayerState.notifications），游戏内
      // 不可观测（官方同——邮件通知）
      notify(message, groupInterval) {
        if (gw.intents.push('notify', { message, groupInterval }, 20)) {
          return C_OK;
        }
        return C_ERR_FULL;
      },
      map: (() => {
        // M2B 扩面 P6：MapVisual 惰性单例（官方 map.js:16 `let _visual`——
        // 数据在 gw.addVisual 的 store（每 tick 重置），对象身份仅当 tick 稳定）
        let _mapVisual;
        // ---------------------------------------------------------------- M2B 扩面 P3：map 族真化
        // 官方 map.js:11-268 逐方法对照。数据源=投影 rooms 键集（世界全房
        // 表——不可见房也携 terrain-only 条目；官方 mapGrid gridData 同位）。
        // 差异登记（compatibility.md）：①出口推导——官方逐房 t/b/l/r 标记
        //（可单边封闭/closed 房），我方在场房恒四向全开（单机全图无封闭房
        // 概念；off-world 返 null 对齐 gridItem 缺失）；②novice/respawn/closed
        // 状态无数据源（getRoomStatus 恒走 normal/closed 两臂，roomStatus
        // 能力面随后续批）；③getWorldSize 维持 1（登记面）。玩家调用时点的
        // Math/Number 裸用=官方同形（官方 map.js 同在玩家 realm 裸用）
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
          // 官方 path-utils.js:5-122 Heap/OpenClosed 逐字移植（评审 1 P2-1
          // 清偿：自写二叉堆+惰性删除在并列 fcost 的弹出序与官方分岔——
          // 实测 8/11 tie 案例返回房间序列不同（成本/步数同），玩家经
          // findRoute 返回值可直接观测；Heap 冒泡 `<=` 即换/pop `>=` 下沉/
          // update 线性扫 decrease-key 三处序敏感语义逐字保留）
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
            if (continuous) {
              const worldSize = 1;
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
          // 官方 map.js:275-374（M2B 扩面 P6）：visual 惰性 getter——MapVisual
          // 全套（世界坐标：RoomPosition 强制 + 各点携房名 n/n1/n2；style
          // 缺省 {}；绘制经 addVisual("map",...)——map 桶 1000KB 上限）
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
          // 越界或无地形数据 → undefined。M0 世界仅在 createShard/setTerrain 提供
          // 地形时可用（bit 语义同官方 TERRAIN_MASK_*）
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

  // ---------------------------------------------------------------- intents 收集器
  // 官方同形（driver/lib/runtime/runtime.js:60-100）：闭包私有对象，玩家拿不到
  // 引用——伪造/清空/抢跑均不可能（防篡改是约束②的结构性实现）。同名覆盖
  // 只计一次费（:68）、say/pull 免费（:61）、cancelOrder 退费不查免费表（:95，
  // 官方 quirk 原样保留）；list 按对象 id → 动作名组织，room/global 级走
  // 数组通道（M2A-A2 只实装对象级与 room 级，global 随 market 批次）。
  // null 原型 + 自有键（官方 runtime.js:60 同款）：原型污染不可使计费免费化
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
      pushByName(id, name, data) {
        list[id] = list[id] || __Object.create(null);
        __Object.setPrototypeOf(data, null);
        list[id][name] = list[id][name] || [];
        list[id][name].push(data);
        // 官方 room/global 级按元素数计费（make.js:100-106；免费表 say/pull
        // 是对象级语义）——Rust 侧 take_endtick_intents 同口径兜底，此处计入
        // 玩家可见的 getUsed（intents.cpu 显示面）
        cpu += 0.2;
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
    /** 宿主私有通道：注入 RawMemory 初值（官方每 tick 从存档恢复，make.js:160
     *  一带）。写闭包 rawMemoryData——不经 globalThis.RawMemory 查找（玩家
     *  换掉全局绑定后旧路径会执行伪造 set，A5-core 对抗评审 P3b），下一次
     *  beginTick 重建 RawMemory 时即生效（_parsed 单 tick 生命周期，无旁路）。 */
    setRawMemoryHost(data) {
      rawMemoryData = data;
    },
    /** 宿主私有通道：rawMemoryData 当前值（endTick 信封的 rawMemory 候选——
     *  B0-5 统一提交点：协调器在世界结算成功后写入玩家持久状态）。 */
    rawMemoryDataHost() {
      return rawMemoryData;
    },
    /** 宿主私有通道（M2B 扩面 P6）：注入本 tick 的 segments 活动窗口（官方
     *  data.memorySegments——协调器从 PlayerState 按 active_request 解算；
     *  beginTick 重建 RawMemory 时拷入）。 */
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
     *  兼容 string/对象两种形态）并重建 Game。 */
    beginTick(worldJson, cpuMeta) {
      // 世界解码走预置 parse（官方 runtime.js:207 对宿主数据用
      // systemFunctions.JSON.parse 同款）：全局 parse 玩家可覆写，可伪造
      // 整份世界含 user → per-viewer 判定翻转（自审 P2）
      gw.world = typeof worldJson === 'string' ? __jsonParse(worldJson) : worldJson;
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
      // 原始投影索引（官方 runtimeData.roomObjects 等价物）：原型字段 getter/
      // 方法经 gw.rawData(id) 回查（A3b）——先于 refreshGame（壳装配期即有回查面）
      gw.rawById = new __Map();
      const tickObjs = gw.world.objects || [];
      for (let i = 0; i < tickObjs.length; i++) {
        const o = tickObjs[i];
        if (o && o.id !== undefined) __reflectApply(__mapSet, gw.rawById, [o.id, o]);
      }
      resetIntents(); // 官方每 tick 新建收集器（runtime.js _start 每 tick 执行）
      // A4-iii per-tick 闭包重置（官方 game 层每 tick 新 make 闭包的等价物）：
      // activateSafeMode 的 lastActivateSafeMode（B1 拆分后在 structures_spawn.js——同 tick
      // 后开者覆盖先开者、跨 tick 不残留）与 claimController 的
      // controllersClaimedInTick（creeps.js:845——GCL 门的同 tick 已 claim 计数）
      gw.lastActivateSafeMode = null;
      gw.controllersClaimedInTick = 0;
      // M2B 扩面 P2：创建族 per-tick 计数（官方 game 层闭包每 tick 新建的
      // 等价物——rooms.js 的 createdFlagNames/createdConstructionSites/
      // createdSpawnNames，:1015/:1037-1043/:1078-1087 消费）
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
      // M2B 扩面 P6：visual 存储每 tick 重置（官方后端每 tick getVisual drain
      // 同语义——绘制只活在当 tick）
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
      // per-tick 重置钩（官方 game/structures.js make 每 tick 重置
      // createdCreepNames:29 的等价物——spawn 族的同 tick 同名拒名单）
      // 索引迭代（A4-i 评审 2 P2-1：beginTick 宿主路径的 for...of 经
      // Array.prototype[Symbol.iterator] 运行期解析——家族修法，同 A4-ii.1）
      if (gw.perTickReset) {
        const fns = gw.perTickReset;
        for (let i = 0; i < fns.length; i++) fns[i]();
      }
      refreshGame();
    },
    writeBackMemory,
    intentsPayload,
  };
})();
