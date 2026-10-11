/* KKPV 块束的沙箱侧可信解码器。
 * 私有束（对象记录）与公开束（地形）都经宿主传入 ArrayBuffer；公开束
 * 是跨玩家共享的同一份内存，本段在加载时捕获全部内建（含 Error 与
 * Uint8Array），玩家运行期改写原型或全局不影响解码，缓冲区本身不进入
 * 玩家可见面——私有束只暴露解码后的值，公开束只暴露地形字节的视图，
 * 视图由可信闭包按索引读取（getRawBuffer 经构造器拷出，不返还视图）。
 * 块布局与 crates/kk-schema/src/kkpv.rs 的 v1 编码一致：160 字节头、
 * ID 升序目录、按类型分桶的列式字段、集中 arena 与单字节嵌套标签。
 * FLAG_DIRTY 块在 arena 后携带逐桶行脏位段与基准拍号（行级脏位，
 * 见 kkpv.rs 模块注释）；基准与上一拍成功解码衔接时，干净行（与基准
 * 拍逐格等值）只重指惰性上下文、跳过全字段就地更新。
 * 束框架是 u32 块数 + 逐块 u32 长度目录 + 顺序拼接的块字节。
 * 记录字段解码：定宽列（bool/int/uint/float/ref）急切读取为普通数据
 * 属性；变长列（str/array/object）经共享惰性访问器首读解码并固化为值
 * 属性（访问器按桶字段共享，解码上下文经 Symbol 键挂对象，均不逐对象
 * 分配闭包）。缺失位置位的字段不定义属性，与 JSON 参考路径的缺键
 * 形态一致。
 * 校验分工：此处做逐块结构与逐读取校验（段边界/互斥、目录长度精确、
 * 桶消费精确、载荷截断、UTF-8 严格性、块头 tick 与信封一致）。行占用
 * 互斥、字段目录键序、位图互斥与尾位等全量扫描不变式在生产链路上由
 * "生产者即编码器"保证（宿主→沙箱不经 Rust 解码端）；Rust 解码端的
 * 全量校验服务于测试与块库发布路径。
 */
(() => {
  'use strict';

  // 全部内建在加载期捕获；此后玩家对原型与全局的任何改写都不影响解码。
  const __Object = Object;
  const __defineProperty = __Object.defineProperty;
  const __getOwnPropertyDescriptor = __Object.getOwnPropertyDescriptor;
  const __reflectApply = Reflect.apply;
  const __DataView = DataView;
  const __dvGetUint8 = __DataView.prototype.getUint8;
  const __dvGetUint16 = __DataView.prototype.getUint16;
  const __dvGetUint32 = __DataView.prototype.getUint32;
  const __dvGetFloat64 = __DataView.prototype.getFloat64;
  const __dvByteLength = __getOwnPropertyDescriptor(__DataView.prototype, 'byteLength').get;
  const __fromCharCode = String.fromCharCode;
  const __Infinity = Infinity;
  const __Error = Error;
  const __Uint8Array = Uint8Array;
  const __Set = Set;
  const __mapGet = Map.prototype.get;
  const __mapSet = Map.prototype.set;
  const __mapDelete = Map.prototype.delete;
  const __mapForEach = Map.prototype.forEach;
  const __weakGet = WeakMap.prototype.get;
  const __weakSet = WeakMap.prototype.set;

  const HEADER_LEN = 160;
  // 2^32 在 double 中精确；i64 取负经补码量值合成（见 readInt）。
  const TWO_32 = 4294967296;
  // 嵌套深度上限与 Rust 侧 NESTING_HARD_CAP 同值，防敌意载荷栈溢出。
  const NESTING_CAP = 1024;
  const HEX = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'];

  function fail(msg) {
    throw new __Error('KKPV: ' + msg);
  }

  function u8(dv, at) {
    return __reflectApply(__dvGetUint8, dv, [at]);
  }
  function u16(dv, at) {
    return __reflectApply(__dvGetUint16, dv, [at, true]);
  }
  function u32(dv, at) {
    return __reflectApply(__dvGetUint32, dv, [at, true]);
  }

  // u64 的十六进制对象 ID：与 kk-world ObjId::as_str 的 kk{:016x} 同形。
  function hex8(n) {
    n = n >>> 0;
    let s = '';
    for (let i = 7; i >= 0; i--) s += HEX[(n >>> (i * 4)) & 0xf];
    return s;
  }
  function idFromHalves(lo, hi) {
    return 'kk' + hex8(hi) + hex8(lo);
  }

  // 无符号：hi*2^32+lo 是对真值的单次正确舍入，与十进制解析的结果恒等。
  function readUint(dv, at) {
    return u32(dv, at + 4) * TWO_32 + u32(dv, at);
  }

  // 有符号：负数不走"合成无符号再减 2^64"——[2^63,2^64) 区间 double 有
  // 一次舍入，先减后差会引入最多半 ulp（约 1024）的系统性误差。改为按
  // 32 位半字做补码取负得到量值，量值按无符号单次正确舍入，再精确取负。
  function readInt(dv, at) {
    const lo = u32(dv, at);
    const hi = u32(dv, at + 4);
    if (hi <= 0x7fffffff) return hi * TWO_32 + lo;
    let magLo = (0 - lo) >>> 0;
    let magHi = (0 - hi) >>> 0;
    if (lo !== 0) magHi = (magHi - 1) >>> 0;
    return -(magHi * TWO_32 + magLo);
  }

  // 严格 UTF-8 → JS 字符串：拒绝续字节错位、超长编码、代理区与越界码点。
  function utf8(dv, at, len) {
    const units = [];
    let i = 0;
    while (i < len) {
      const b0 = u8(dv, at + i);
      i += 1;
      if (b0 < 0x80) {
        units[units.length] = b0;
        continue;
      }
      let need;
      let cp;
      if (b0 >= 0xc2 && b0 <= 0xdf) {
        need = 1;
        cp = b0 & 0x1f;
      } else if (b0 >= 0xe0 && b0 <= 0xef) {
        need = 2;
        cp = b0 & 0x0f;
      } else if (b0 >= 0xf0 && b0 <= 0xf4) {
        need = 3;
        cp = b0 & 0x07;
      } else {
        fail('非法 UTF-8 引导字节');
      }
      if (i + need > len) fail('UTF-8 序列截断');
      for (let j = 0; j < need; j++) {
        const b = u8(dv, at + i + j);
        if ((b & 0xc0) !== 0x80) fail('非法 UTF-8 续字节');
        cp = (cp << 6) | (b & 0x3f);
      }
      i += need;
      if (cp >= 0xd800 && cp <= 0xdfff) fail('UTF-8 代理区码点');
      if (cp > 0x10ffff) fail('UTF-8 码点越界');
      if (
        cp < 0x80 ||
        (b0 <= 0xdf && cp < 0x80) ||
        (b0 >= 0xe0 && b0 <= 0xef && cp < 0x800) ||
        (b0 >= 0xf0 && cp < 0x10000)
      ) {
        fail('超长 UTF-8 编码');
      }
      if (cp <= 0xffff) {
        units[units.length] = cp;
      } else {
        const v = cp - 0x10000;
        units[units.length] = ((v / 0x400) | 0) + 0xd800;
        units[units.length] = (v % 0x400) + 0xdc00;
      }
    }
    // 分批 fromCharCode：单次 apply 有参数上限，批间用字符串拼接。
    let out = '';
    for (let s = 0; s < units.length; s += 4096) {
      const end = s + 4096 < units.length ? s + 4096 : units.length;
      const chunk = [];
      for (let k = s; k < end; k++) chunk[chunk.length] = units[k];
      out += __reflectApply(__fromCharCode, null, chunk);
    }
    return out;
  }

  // 嵌套载荷（标签 0-8）；返回 [值, 下一偏移]。每个标签的载荷宽度都做
  // end 边界检查——截断不得静默读入 arena 相邻字节。
  function nested(dv, at, end, depth) {
    if (depth > NESTING_CAP) fail('嵌套深度超限');
    if (at >= end) fail('嵌套载荷截断');
    const tag = u8(dv, at);
    at += 1;
    switch (tag) {
      case 0:
        return [null, at];
      case 1: {
        if (at >= end) fail('布尔载荷截断');
        const b = u8(dv, at);
        if (b > 1) fail('非法 bool 字节');
        return [b === 1, at + 1];
      }
      case 2: {
        if (at + 8 > end) fail('整数载荷截断');
        return [readInt(dv, at), at + 8];
      }
      case 3: {
        if (at + 8 > end) fail('整数载荷截断');
        return [readUint(dv, at), at + 8];
      }
      case 4: {
        if (at + 8 > end) fail('浮点载荷截断');
        const v = __reflectApply(__dvGetFloat64, dv, [at, true]);
        if (v !== v || v === __Infinity || v === -__Infinity) fail('非有限浮点');
        return [v, at + 8];
      }
      case 5: {
        if (at + 8 > end) fail('引用载荷截断');
        return [idFromHalves(u32(dv, at), u32(dv, at + 4)), at + 8];
      }
      case 6: {
        if (at + 4 > end) fail('字符串长度截断');
        const len = u32(dv, at);
        at += 4;
        if (at + len > end) fail('字符串载荷截断');
        return [utf8(dv, at, len), at + len];
      }
      case 7: {
        if (at + 4 > end) fail('数组计数截断');
        const n = u32(dv, at);
        at += 4;
        const arr = [];
        for (let i = 0; i < n; i++) {
          const r = nested(dv, at, end, depth + 1);
          arr[arr.length] = r[0];
          at = r[1];
        }
        return [arr, at];
      }
      case 8: {
        if (at + 4 > end) fail('记录计数截断');
        const n = u32(dv, at);
        at += 4;
        const out = {};
        for (let i = 0; i < n; i++) {
          if (at + 4 > end) fail('记录键长度截断');
          const klen = u32(dv, at);
          at += 4;
          if (at + klen > end) fail('记录键载荷截断');
          const key = utf8(dv, at, klen);
          at += klen;
          const r = nested(dv, at, end, depth + 1);
          // 嵌套键经 defineProperty 定义自有属性：与 JSON.parse 的
          // DefineOwnProperty 语义一致（"__proto__" 等键不改写原型），
          // 键序保持载荷顺序。
          __defineProperty(out, key, {
            value: r[0],
            writable: true,
            enumerable: true,
            configurable: true,
          });
          at = r[1];
        }
        return [out, at];
      }
      default:
        fail('未知嵌套标签 ' + tag);
    }
  }

  function bitAt(dv, bitmapAt, row) {
    const b = u8(dv, bitmapAt + (row >>> 3));
    return (b & (1 << (row & 7))) !== 0;
  }

  // 读取一行一列的值；调用前已排除缺失位。变长载荷要求恰好消费。
  function readField(dv, field, row, arenaBase, arenaLen) {
    if (bitAt(dv, field.nullAt, row)) return null;
    switch (field.kind) {
      case 0: {
        const b = u8(dv, field.colAt + row);
        if (b > 1) fail('非法 bool 字节');
        return b === 1;
      }
      case 1:
        return readInt(dv, field.colAt + row * 8);
      case 2:
        return readUint(dv, field.colAt + row * 8);
      case 3: {
        const v = __reflectApply(__dvGetFloat64, dv, [field.colAt + row * 8, true]);
        if (v !== v || v === __Infinity || v === -__Infinity) fail('非有限浮点');
        return v;
      }
      case 4: {
        return idFromHalves(u32(dv, field.colAt + row * 8), u32(dv, field.colAt + row * 8 + 4));
      }
      default: {
        const off = u32(dv, field.colAt + row * 8);
        const len = u32(dv, field.colAt + row * 8 + 4);
        if (off + len > arenaLen) fail('arena 描述符越界');
        const r = nested(dv, arenaBase + off, arenaBase + off + len, 0);
        if (r[1] !== arenaBase + off + len) fail('变长载荷非恰好消费');
        return r[0];
      }
    }
  }

  // 惰性字段访问器：每（块, 桶, 字段）一份共享 getter/setter，闭包只
  // 捕获列描述；对象身份经 Symbol 键的解码上下文传递（不可枚举，不进
  // Object.keys/JSON.stringify）。getter 首读解码并固化为值属性（writable
  // 与 JSON 记录一致，setter 写入同样固化，不静默丢弃）。
  const __Symbol = Symbol;
  const DECODE_CTX = __Symbol('kk.decodeCtx');
  // 清扫戳（本拍 expectedTick）：与旧 seen Set 等价但免 3210 次串哈希——
  // 符号键不进 JSON/枚举面。
  const SEEN_TICK = __Symbol('kk.seenTick');
  // 每 resident Map 的上一拍记录序：records 全数组 + 每块累计末界 + 目录
  // 键并行双列（u32 double 数组），供下一拍块内两指针归并对位——复用旧
  // 记录时不物化 id 字符串（省每拍 ~3210 次 idFromHalves 分配/哈希/Map
  // 探测与 seen Set 存活，GC 实测主税之一，见研究补记 L4）。块数变动的
  // 错位轮走慢路径自愈（重建记录，正确性无害），下一拍恢复全量对位。
  const mergeState = new WeakMap();

  // 变更戳（本拍 expectedTick）：走全量更新或新建/重建的记录打戳，干净行
  // 不打——装配主环（game.js）按戳跳过干净行的全部包装/分桶/分类工作。
  // 回退轮（跳拍、重试、块数变动、签名不符）自然全量打戳，装配随之自动
  // 退化为全量路径，无需独立门控。符号键不进枚举与 JSON 面。
  const CHANGED_TICK = __Symbol('kk.changedTick');
  // 变长字段的共享惰性访问器按**字段索引**工作：字段元数据（含列偏移）
  // 随块布局逐 tick 变化，因此放在 DECODE_CTX 里按索引取，而不是在闭包
  // 里捕获——访问器于是对常驻记录终身有效，就地更新只需换 ctx。
  function makeLazyAccessors(index) {
    return {
      enumerable: true,
      configurable: true,
      get() {
        const ctx = this[DECODE_CTX];
        const field = ctx.fields[index];
        const v = readField(ctx.dv, field, ctx.row, ctx.arenaBase, ctx.arenaLen);
        __defineProperty(this, field.key, {
          value: v,
          writable: true,
          enumerable: true,
          configurable: true,
        });
        return v;
      },
      set(v) {
        const field = this[DECODE_CTX].fields[index];
        __defineProperty(this, field.key, {
          value: v,
          writable: true,
          enumerable: true,
          configurable: true,
        });
      },
    };
  }

  // 校验块头与四段边界，返回后续解码需要的头部信息。expectedTick 与
  // 信封的 world.time 一致性在此交叉校验（块与信封来自同一次投影）。
  // 段规则：零长度段 offset 须为零；非零段落在 [头长, 块长) 且两两不重叠。
  function checkBlockHeader(dv, start, len, expectedTick) {
    if (len < HEADER_LEN) fail('块短于头部');
    if (u32(dv, start) !== 0x56504b4b) fail('魔数不符');
    if (u16(dv, start + 4) !== 1) fail('版本不支持');
    const flags = u16(dv, start + 6);
    if ((flags & ~7) !== 0) fail('未知 flags 位');
    if ((flags & 4) !== 0 && (flags & 2) === 0) {
      fail('字段脏位段不得脱离行脏位段单独携带');
    }
    if (u32(dv, start + 8) !== len) fail('byte_len 不符');
    if (u32(dv, start + 12) !== HEADER_LEN) fail('header_len 不符');
    // 保留区 144..160 承载行脏位段对（FLAG_DIRTY 置位时为块内第五段，
    // 逐桶位图按桶秩序拼接）与基准拍号；未置位时整区必须为零。
    const dirty = [u32(dv, start + 144), u32(dv, start + 148)];
    const baseTick = readUint(dv, start + 152);
    if ((flags & 2) === 0) {
      if (dirty[0] !== 0 || dirty[1] !== 0 || baseTick !== 0) fail('保留字节非零');
    } else if (baseTick > expectedTick) {
      fail('脏位基准拍号晚于本块拍号');
    }
    if (((flags & 2) !== 0) !== dirty[1] > 0) fail('脏位段与 FLAG_DIRTY 不一致');
    if (readUint(dv, start + 16) !== expectedTick) fail('块头 tick 与信封不一致');
    const objectCount = u32(dv, start + 104);
    const typeCount = u32(dv, start + 108);
    const terrain = [u32(dv, start + 112), u32(dv, start + 116)];
    const directory = [u32(dv, start + 120), u32(dv, start + 124)];
    const buckets = [u32(dv, start + 128), u32(dv, start + 132)];
    const arena = [u32(dv, start + 136), u32(dv, start + 140)];
    const segs = [terrain, directory, buckets, arena, dirty];
    for (let s = 0; s < segs.length; s++) {
      const o = segs[s][0];
      const l = segs[s][1];
      if (l === 0) {
        if (o !== 0) fail('零长度段 offset 非零');
      } else if (o < HEADER_LEN || o + l > len) {
        fail('段越界');
      }
    }
    for (let a = 0; a < segs.length; a++) {
      for (let b = a + 1; b < segs.length; b++) {
        const x = segs[a];
        const y = segs[b];
        if (x[1] > 0 && y[1] > 0 && x[0] < y[0] + y[1] && y[0] < x[0] + x[1]) fail('段重叠');
      }
    }
    return {
      flags,
      objectCount,
      typeCount,
      terrain,
      directory,
      buckets,
      arena,
      arenaBase: start + arena[0],
      arenaLen: arena[1],
      dirty,
      baseTick,
    };
  }

  // 常驻记录的布局签名键：记下创建时桶的字段序与列类型；后续 tick 的
  // 桶签名一致才允许就地更新（访问器的字段索引语义不变），否则重建。
  const RESIDENT_SIG = __Symbol('kkpvResidentSig');

  // 解析单块并把记录按目录顺序追加进 records；私有束只接受私有块
  // （公开块出现在私有束说明通道装配有误）。resident 非 null 时启用
  // 跨 tick 记录常驻：id 命中且布局签名一致就就地更新（ctx 重指新块、
  // 急读属性重写、变长属性重装共享访问器），目录外的 id 由调用方清扫。
  // lastTick 是该常驻表上一拍成功解码的拍号；块携带脏位段且基准拍号
  // 与之衔接时，干净行（与基准拍逐格等值）只重指 ctx、跳过全字段更新
  // ——错位（跳拍、重试、基准过期）自动回退全量路径。
  function decodeBlockInto(
    dv,
    records,
    his,
    los,
    start,
    len,
    expectedTick,
    resident,
    prev,
    lastTick,
  ) {
    const h = checkBlockHeader(dv, start, len, expectedTick);
    if ((h.flags & 1) !== 1) fail('私有束要求私有块');
    if (h.terrain[1] !== 0) fail('私有对象块不带地形段');
    const objectCount = h.objectCount;
    const arenaBase = h.arenaBase;
    const arenaLen = h.arenaLen;
    if (objectCount * 16 !== h.directory[1]) fail('目录长度与对象数不符');
    const cleanEnabled = (h.flags & 2) !== 0 && resident !== null && h.baseTick === lastTick;

    // 类型桶：16 字节桶头 + 12 字节字段项 + 缺失/null 位图 + 定宽列，
    // 消费量须与桶段长度精确一致。
    let cur = start + h.buckets[0];
    const bucketsEnd = cur + h.buckets[1];
    const bucketList = [];
    // 脏位段按桶秩序拼接，桶走查同步推进每桶位图偏移。
    let dirtyCursor = start + h.dirty[0];
    for (let t = 0; t < h.typeCount; t++) {
      if (cur + 16 > bucketsEnd) fail('桶头越界');
      const rows = u32(dv, cur + 8);
      const fieldCount = u32(dv, cur + 12);
      // 与 Rust 解码端同源拒绝：类型桶至少承载一行，零行桶无合法产出方。
      if (rows === 0) fail('桶行数为零');
      cur += 16;
      const fields = [];
      for (let f = 0; f < fieldCount; f++) {
        if (cur + 12 > bucketsEnd) fail('字段项越界');
        const kind = u8(dv, cur + 8);
        if (kind > 7) fail('未知列类型');
        if (u8(dv, cur + 9) !== 0 || u8(dv, cur + 10) !== 0 || u8(dv, cur + 11) !== 0) {
          fail('字段项保留字节非零');
        }
        fields[fields.length] = {
          key: null,
          kind,
          keyOff: u32(dv, cur),
          keyLen: u32(dv, cur + 4),
          missingAt: 0,
          nullAt: 0,
          colAt: 0,
        };
        cur += 12;
      }
      // 位图长度不用 (rows+7)>>>3：rows 接近 2^32 时加法先回绕。
      const bmLen = (rows >>> 3) + ((rows & 7) !== 0 ? 1 : 0);
      const dirtyAt = dirtyCursor;
      dirtyCursor += bmLen;
      for (let f = 0; f < fieldCount; f++) {
        fields[f].missingAt = cur;
        cur += bmLen;
      }
      for (let f = 0; f < fieldCount; f++) {
        fields[f].nullAt = cur;
        cur += bmLen;
      }
      for (let f = 0; f < fieldCount; f++) {
        fields[f].colAt = cur;
        cur += (fields[f].kind === 0 ? 1 : 8) * rows;
      }
      if (cur > bucketsEnd) fail('桶列越界');
      let lazyAccessors = null;
      for (let f = 0; f < fieldCount; f++) {
        if (fields[f].keyOff + fields[f].keyLen > arenaLen) fail('字段键描述符越界');
        fields[f].key = utf8(dv, arenaBase + fields[f].keyOff, fields[f].keyLen);
        // "id" 由目录承担；字段键不得覆盖记录身份（与编码端拒绝一致）。
        if (fields[f].key === 'id') fail('字段键不得为 id');
        // 变长字段的共享惰性访问器：每桶每字段一份（按索引工作，终身
        // 有效），不逐对象分配。
        if (fields[f].kind >= 5) {
          if (lazyAccessors === null) lazyAccessors = new Array(fieldCount).fill(null);
          lazyAccessors[f] = makeLazyAccessors(f);
        }
      }
      // 桶布局签名（字段键 + 列类型的有序串，每桶一次）：变长访问器按
      // 字段索引工作，签名一致才允许常驻记录就地更新。
      let sig = null;
      if (resident !== null) {
        const parts = [];
        for (let f = 0; f < fieldCount; f++) {
          parts.push(fields[f].key, fields[f].kind);
        }
        sig = parts.join('\u0000');
      }
      bucketList[bucketList.length] = { rows, fields, lazyAccessors, sig, dirtyAt };
    }
    if (cur !== bucketsEnd) fail('桶段消费不符');
    // 字段脏位小节（FLAG_FIELD_DIRTY）：行位图小节之后按桶序为每桶
    // 每个 kind≥5 字段各一行位图；记录每桶偏移数组供 updateResident
    // 按字段序对位（变长字段枚举序与编码端同源）。总长须精确消费。
    if ((h.flags & 4) !== 0) {
      let fieldCursor = dirtyCursor;
      for (let t = 0; t < bucketList.length; t++) {
        const bucket = bucketList[t];
        const bmLen = (bucket.rows >>> 3) + ((bucket.rows & 7) !== 0 ? 1 : 0);
        const offs = [];
        for (let f = 0; f < bucket.fields.length; f++) {
          if (bucket.fields[f].kind < 5) continue;
          offs[offs.length] = fieldCursor;
          fieldCursor += bmLen;
        }
        bucket.fieldDirtyAt = offs;
      }
      if (fieldCursor !== start + h.dirty[0] + h.dirty[1]) {
        fail('字段脏位段长度与桶变长字段数不符');
      }
    } else if (h.dirty[1] > 0 && dirtyCursor !== start + h.dirty[0] + h.dirty[1]) {
      // 无字段小节时行位图小节即整段（旧形态块），长度精确校验。
      fail('脏位段长度与桶行数不符');
    }

    // 目录：ID 升序；句柄与行号须落在已解析桶内。
    const dirAt = start + h.directory[0];
    let prevHi = -1;
    let prevLo = -1;
    // 块内归并（prev 非 null 时启用）：两指针推进，小于当键的旧记录即本拍
    // 消亡（出 resident）；键相等即复用旧记录对象——完全不物化 id 字符串。
    // 未命中或签名不符落回慢路径（idFromHalves + Map 查询），块错位自愈。
    let pi = prev === null ? 0 : prev.ps;
    const pe = prev === null ? 0 : prev.pe;
    const prevArr = prev === null ? null : prev.arr;
    const prevHiCol = prev === null ? null : prev.his;
    const prevLoCol = prev === null ? null : prev.los;
    for (let i = 0; i < objectCount; i++) {
      const at = dirAt + i * 16;
      const lo = u32(dv, at);
      const hi = u32(dv, at + 4);
      if (hi < prevHi || (hi === prevHi && lo <= prevLo)) fail('目录未按 ID 升序');
      prevHi = hi;
      prevLo = lo;
      const handle = u32(dv, at + 8);
      const row = u32(dv, at + 12);
      if (handle >= bucketList.length) fail('未知类型句柄');
      const bucket = bucketList[handle];
      if (row >= bucket.rows) fail('行号越界');
      let existing = null;
      if (resident !== null && prevArr !== null) {
        while (pi < pe) {
          if (prevHiCol[pi] < hi || (prevHiCol[pi] === hi && prevLoCol[pi] < lo)) {
            // 块内消失的旧记录：本拍已在其他块复用（SEEN_TICK 已推进——
            // 跨房移动换块）则是活行，不得出 resident 也不算消亡；未见
            // 过才是真消亡。消亡收集以 SEEN 判定为准，末尾清扫兜底。
            if (prevArr[pi][SEEN_TICK] !== expectedTick) {
              deadScratch[deadScratch.length] = prevArr[pi].id;
              __reflectApply(__mapDelete, resident, [prevArr[pi].id]);
            }
            pi++;
          } else {
            break;
          }
        }
        if (pi < pe && prevHiCol[pi] === hi && prevLoCol[pi] === lo) {
          existing = prevArr[pi];
          pi++;
        }
      }
      // 常驻命中：布局签名一致（访问器的字段索引语义不变）就就地更新，
      // 免去每拍重建记录对象；布局变化（字段增删/列类型变/换桶）重建。
      // 干净行（脏位清零且基准拍衔接）只重指 ctx 跳过全字段更新——
      // 编码端逐格比对保证值等价，重指 ctx 同时避免旧束缓冲被钉住。
      if (existing !== null) {
        if (existing[RESIDENT_SIG] === bucket.sig) {
          existing[SEEN_TICK] = expectedTick;
          if (cleanEnabled && !bitAt(dv, bucket.dirtyAt, row)) {
            if (bucket.lazyAccessors !== null) {
              rePointCtx(existing, dv, row, arenaBase, arenaLen, bucket.fields);
            }
          } else {
            updateResident(existing, dv, bucket, row, arenaBase, arenaLen, cleanEnabled);
          }
          records[records.length] = existing;
          his[his.length] = hi;
          los[los.length] = lo;
          continue;
        }
        __reflectApply(__mapDelete, resident, [existing.id]);
        existing = null;
      }
      const id = idFromHalves(lo, hi);
      if (resident !== null) {
        const found = __reflectApply(__mapGet, resident, [id]);
        if (found !== undefined) {
          if (found[RESIDENT_SIG] === bucket.sig) {
            found[SEEN_TICK] = expectedTick;
            if (cleanEnabled && !bitAt(dv, bucket.dirtyAt, row)) {
              if (bucket.lazyAccessors !== null) {
                rePointCtx(found, dv, row, arenaBase, arenaLen, bucket.fields);
              }
            } else {
              updateResident(found, dv, bucket, row, arenaBase, arenaLen, cleanEnabled);
            }
            records[records.length] = found;
            his[his.length] = hi;
            los[los.length] = lo;
            continue;
          }
          __reflectApply(__mapDelete, resident, [id]);
        }
      }
      const rec = {};
      rec.id = id;
      // 新建记录（含签名不符重建）：本拍新数据，打变更戳。
      rec[CHANGED_TICK] = updateStamp;
      // 定宽列（bool/int/uint/float/ref）一次 DataView 读即值：急切装
      // 普通数据属性比逐对象逐字段装访问器便宜得多，也免去其闭包分配。
      // 变长列（str/array/object）保持首读惰性：共享访问器 + Symbol 上下文。
      const hasVariable = bucket.lazyAccessors !== null;
      if (hasVariable) {
        rec[DECODE_CTX] = { dv, row, arenaBase, arenaLen, fields: bucket.fields };
      }
      const fields = bucket.fields;
      if (resident !== null) {
        rec[RESIDENT_SIG] = bucket.sig;
        rec[SEEN_TICK] = expectedTick;
      }
      for (let f = 0; f < fields.length; f++) {
        const field = fields[f];
        if (bitAt(dv, field.missingAt, row)) continue;
        if (field.kind < 5) {
          rec[field.key] = readField(dv, field, row, arenaBase, arenaLen);
        } else {
          __defineProperty(rec, field.key, bucket.lazyAccessors[f]);
        }
      }
      if (resident !== null) __reflectApply(__mapSet, resident, [id, rec]);
      records[records.length] = rec;
      his[his.length] = hi;
      los[los.length] = lo;
    }
    // 块尾残留的旧记录（键大于本块末键）：块内消失，同上以 SEEN 区分
    // 跨块活行与真消亡。
    if (resident !== null && prevArr !== null) {
      while (pi < pe) {
        if (prevArr[pi][SEEN_TICK] !== expectedTick) {
          deadScratch[deadScratch.length] = prevArr[pi].id;
          __reflectApply(__mapDelete, resident, [prevArr[pi].id]);
        }
        pi++;
      }
    }
  }

  // 惰性解码上下文重指：干净行跳过全字段更新时仍把 ctx 指向本拍束，
  // 未物化的变长字段首读走新缓冲（值等价由编码端逐格比对保证），旧束
  // backing store 不再被常驻记录引用。
  function rePointCtx(rec, dv, row, arenaBase, arenaLen, fields) {
    const ctx = rec[DECODE_CTX];
    ctx.dv = dv;
    ctx.row = row;
    ctx.arenaBase = arenaBase;
    ctx.arenaLen = arenaLen;
    ctx.fields = fields;
  }

  // 常驻记录的就地更新：ctx 换指新块（复用对象免分配），急读属性按新值
  // 重写、缺失字段删除残留，变长字段重装共享访问器（顶掉已物化的旧值）。
  // 调用前已由布局签名保证字段集、顺序与列类型不变。就地更新即数据变更
  // ——打变更戳供装配主环差分（干净行不打）。
  function updateResident(rec, dv, bucket, row, arenaBase, arenaLen, trusted) {
    DIAG.dirty++;
    rec[CHANGED_TICK] = updateStamp;
    if (bucket.lazyAccessors !== null) {
      rePointCtx(rec, dv, row, arenaBase, arenaLen, bucket.fields);
    }
    const fields = bucket.fields;
    // 变长字段按字段脏位条件跳过（基准衔接时才可信任——错位轮编码端
    // 的"与基准等值"不代表解码端常驻状态）：位清零表示该格与基准拍
    // 逐格等值，已物化值保持、未物化经重指的 ctx 惰性首读等值字节，
    // 两者都不需要重装访问器；位图缺省（无字段小节）按全量重装。
    const fieldDirtyAt = trusted ? bucket.fieldDirtyAt : null;
    let varIdx = 0;
    for (let f = 0; f < fields.length; f++) {
      const field = fields[f];
      const isVar = field.kind >= 5;
      if (bitAt(dv, field.missingAt, row)) {
        // 缺失也占一个字段位图槽（枚举序与编码端同源按字段序）。
        delete rec[field.key];
      } else if (!isVar) {
        rec[field.key] = readField(dv, field, row, arenaBase, arenaLen);
      } else if (
        fieldDirtyAt === null ||
        fieldDirtyAt === undefined ||
        bitAt(dv, fieldDirtyAt[varIdx], row)
      ) {
        DIAG.remat++;
        __defineProperty(rec, field.key, bucket.lazyAccessors[f]);
      }
      if (isVar) varIdx++;
    }
  }

  /** 束框架走查：校验 u32 块数 + 长度目录 + 精确总量，对每块调用
   * visit(start, len)。逐块先做束内边界预检，越界在解码前失败。返回块数。 */
  function forEachBlock(dv, total, visit) {
    if (total < 4) fail('束短于框架头部');
    const count = u32(dv, 0);
    if (count > (total - 4) / 4) fail('束短于长度目录');
    let cur = 4 + count * 4;
    for (let i = 0; i < count; i++) {
      const len = u32(dv, 4 + i * 4);
      if (len > total - cur) fail('块越界');
      visit(cur, len);
      cur += len;
    }
    if (cur !== total) fail('束长度与块目录不符');
    return count;
  }

  /** 解码私有对象束并返回记录数组；块的房间顺序与目录的 ID 顺序共同
   * 决定数组顺序，与 JSON 参考路径的对象遍历序一致。expectedTick 是
   * 信封的 world.time，逐块交叉校验。 */
  function decodeBundle(buffer, expectedTick) {
    const dv = new __DataView(buffer);
    const total = __reflectApply(__dvByteLength, dv, []);
    if (expectedTick === undefined || expectedTick === null) {
      fail('缺少信封 tick 一致性参数');
    }
    const records = [];
    const his = [];
    const los = [];
    forEachBlock(dv, total, (start, len) => {
      decodeBlockInto(dv, records, his, los, start, len, expectedTick, null, null, undefined);
    });
    return records;
  }

  /** 常驻版解码：resident 是跨 tick 存活的 id → 记录 Map（调用方持有，
   * 沙箱生命周期内复用）。id 命中且布局签名一致就地更新，未命中新建，
   * 本 tick 目录之外的 id 清扫出表。记录对象跨 tick 保持同一性——与
   * Memory 常驻同属已接受的观察面（依赖每拍新对象的代码在官方即坏）。
   * 块序与上一拍一致时逐块两指针归并（免 id 串物化与 seen Set）；
   * 清扫改走 SEEN_TICK 戳比对。块数变动即弃用归并一拍（慢路径自愈）。 */
  // run 相构成诊断的累积计数（KKDIAG）：rows=处理行数、dirty=走全量
  // 更新的行数（干净行跳过不计）、remat=重装的变长字段访问器数。
  const DIAG = { rows: 0, dirty: 0, remat: 0 };
  globalThis.__kkp.kkpvDiag = DIAG;

  // 本拍消亡 id 收集与变更戳当前值：decodeBundleResident 每次调用重置，
  // 结束后经 gw.kkpv.lastDecode 交给装配主环（rawById/常驻包装/分类的
  // 差分驱动）。decodeBundle（无常驻参考路径）不消费这些信息。
  let deadScratch = [];
  let updateStamp = -1;

  function decodeBundleResident(buffer, expectedTick, resident) {
    const dv = new __DataView(buffer);
    const total = __reflectApply(__dvByteLength, dv, []);
    if (expectedTick === undefined || expectedTick === null) {
      fail('缺少信封 tick 一致性参数');
    }
    deadScratch = [];
    updateStamp = expectedTick;
    const records = [];
    const his = [];
    const los = [];
    const state = __reflectApply(__weakGet, mergeState, [resident]);
    // 块数一致才启用归并对位（束块序=投影房间序，房间集稳定时逐拍一致）
    const prevEnds = state !== undefined ? state.ends : null;
    // 上一拍成功解码的拍号：块脏位基准与之一致时干净行才可跳过更新。
    const lastTick = state !== undefined ? state.lastTick : undefined;
    let blockIndex = 0;
    const ends = [];
    forEachBlock(dv, total, (start, len) => {
      let prev = null;
      if (prevEnds !== null && blockIndex < prevEnds.length) {
        prev = {
          arr: state.records,
          his: state.his,
          los: state.los,
          ps: blockIndex === 0 ? 0 : prevEnds[blockIndex - 1],
          pe: prevEnds[blockIndex],
        };
      }
      decodeBlockInto(dv, records, his, los, start, len, expectedTick, resident, prev, lastTick);
      ends[ends.length] = records.length;
      blockIndex++;
    });
    // 无论本轮块数是否变动都存回本轮记录序：变动轮的错位已由慢路径
    // 自愈，下一拍以本轮真实布局为基线恢复全量对位。
    __reflectApply(__weakSet, mergeState, [
      resident,
      { records, ends, his, los, lastTick: expectedTick },
    ]);
    __reflectApply(__mapForEach, resident, [
      (rec, id) => {
        if (rec[SEEN_TICK] !== expectedTick) {
          deadScratch[deadScratch.length] = id;
          __reflectApply(__mapDelete, resident, [id]);
        }
      },
    ]);
    DIAG.rows += records.length;
    // 交给装配差分：tick 供门控（JSON 通道/跳拍自然失配），deadIds 供
    // rawById/常驻包装/分类/分桶的消亡清理。对象每拍新建，旧引用读旧拍。
    gwRef.kkpv.lastDecode = { tick: expectedTick, deadIds: deadScratch };
    return records;
  }

  /** 解码公开地形束，返回 {房间名 → 地形字节视图}。视图直接覆盖共享
   * ArrayBuffer 的地形段（零拷贝）；视图不进入玩家可见面——game.js
   * 只把它交给 terrainByRoom 供可信闭包按索引读取。公开束只接受公开
   * 块（flags 无私有位）、无对象且带非空地形段；房名从 arena 严格
   * UTF-8 读出，重复房名拒绝。 */
  function decodePublicBundle(buffer, expectedTick) {
    const dv = new __DataView(buffer);
    const total = __reflectApply(__dvByteLength, dv, []);
    if (expectedTick === undefined || expectedTick === null) {
      fail('缺少信封 tick 一致性参数');
    }
    const out = __Object.create(null);
    forEachBlock(dv, total, (start, len) => {
      const h = checkBlockHeader(dv, start, len, expectedTick);
      if ((h.flags & 1) !== 0) fail('公开束要求公开块');
      if (h.objectCount !== 0 || h.typeCount !== 0) fail('公开地形块不带对象');
      if (h.directory[1] !== 0 || h.buckets[1] !== 0) fail('公开地形块无目录与桶段');
      if (h.terrain[1] === 0) fail('公开地形块缺地形段');
      // 房间地形契约是 50×50：短段会让越界格静默按 plain 读出，须拒绝。
      if (h.terrain[1] !== 2500) fail('地形段须为 2500 字节');
      const nameOff = u32(dv, start + 96);
      const nameLen = u32(dv, start + 100);
      if (nameLen === 0) fail('公开块缺房间名');
      if (nameOff + nameLen > h.arenaLen) fail('房间名描述符越界');
      const room = utf8(dv, h.arenaBase + nameOff, nameLen);
      if (out[room] !== undefined) fail('公开束房间名重复: ' + room);
      // 视图锚定在共享缓冲区的地形段上；构造器来自加载期捕获。
      const view = new __Uint8Array(buffer, start + h.terrain[0], h.terrain[1]);
      __defineProperty(out, room, {
        value: view,
        writable: true,
        enumerable: true,
        configurable: true,
      });
    });
    return out;
  }

  // gw 引用加载期捕获（__kkp 收尾删除后闭包仍可写 lastDecode）。
  const gwRef = globalThis.__kkp.gw;
  gwRef.kkpv = {
    decodeBundle,
    decodeBundleResident,
    decodePublicBundle,
    // 变更戳符号：装配主环以 rec[CHANGED] === 本拍 tick 判定变更行。
    CHANGED: CHANGED_TICK,
  };
})();
