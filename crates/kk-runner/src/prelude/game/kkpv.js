/* KKPV 块束的沙箱侧可信解码器。
 * 私有束（对象记录）与公开束（地形）都经宿主传入 ArrayBuffer；公开束
 * 是跨玩家共享的同一份内存，本段在加载时捕获全部内建（含 Error 与
 * Uint8Array），玩家运行期改写原型或全局不影响解码，缓冲区本身不进入
 * 玩家可见面——私有束只暴露解码后的值，公开束只暴露地形字节的视图，
 * 视图由可信闭包按索引读取（getRawBuffer 经构造器拷出，不返还视图）。
 * 块布局与 crates/kk-schema/src/kkpv.rs 的 v1 编码一致：160 字节头、
 * ID 升序目录、按类型分桶的列式字段、集中 arena 与单字节嵌套标签。
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
  const __setAdd = Set.prototype.add;
  const __setHas = Set.prototype.has;

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
    if ((flags & ~1) !== 0) fail('未知 flags 位');
    if (u32(dv, start + 8) !== len) fail('byte_len 不符');
    if (u32(dv, start + 12) !== HEADER_LEN) fail('header_len 不符');
    for (let r = 144; r < 160; r++) {
      if (u8(dv, start + r) !== 0) fail('保留字节非零');
    }
    if (readUint(dv, start + 16) !== expectedTick) fail('块头 tick 与信封不一致');
    const objectCount = u32(dv, start + 104);
    const typeCount = u32(dv, start + 108);
    const terrain = [u32(dv, start + 112), u32(dv, start + 116)];
    const directory = [u32(dv, start + 120), u32(dv, start + 124)];
    const buckets = [u32(dv, start + 128), u32(dv, start + 132)];
    const arena = [u32(dv, start + 136), u32(dv, start + 140)];
    const segs = [terrain, directory, buckets, arena];
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
    };
  }

  // 常驻记录的布局签名键：记下创建时桶的字段序与列类型；后续 tick 的
  // 桶签名一致才允许就地更新（访问器的字段索引语义不变），否则重建。
  const RESIDENT_SIG = __Symbol('kkpvResidentSig');

  // 解析单块并把记录按目录顺序追加进 records；私有束只接受私有块
  // （公开块出现在私有束说明通道装配有误）。resident 非 null 时启用
  // 跨 tick 记录常驻：id 命中且布局签名一致就就地更新（ctx 重指新块、
  // 急读属性重写、变长属性重装共享访问器），目录外的 id 由调用方清扫。
  function decodeBlockInto(dv, records, start, len, expectedTick, resident, seen) {
    const h = checkBlockHeader(dv, start, len, expectedTick);
    if ((h.flags & 1) !== 1) fail('私有束要求私有块');
    if (h.terrain[1] !== 0) fail('私有对象块不带地形段');
    const objectCount = h.objectCount;
    const arenaBase = h.arenaBase;
    const arenaLen = h.arenaLen;
    if (objectCount * 16 !== h.directory[1]) fail('目录长度与对象数不符');

    // 类型桶：16 字节桶头 + 12 字节字段项 + 缺失/null 位图 + 定宽列，
    // 消费量须与桶段长度精确一致。
    let cur = start + h.buckets[0];
    const bucketsEnd = cur + h.buckets[1];
    const bucketList = [];
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
      bucketList[bucketList.length] = { rows, fields, lazyAccessors, sig };
    }
    if (cur !== bucketsEnd) fail('桶段消费不符');

    // 目录：ID 升序；句柄与行号须落在已解析桶内。
    const dirAt = start + h.directory[0];
    let prevHi = -1;
    let prevLo = -1;
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
      const id = idFromHalves(lo, hi);
      // 常驻命中：布局签名一致（访问器的字段索引语义不变）就就地更新，
      // 免去每拍重建记录对象；布局变化（字段增删/列类型变/换桶）重建。
      if (resident !== null) {
        __reflectApply(__setAdd, seen, [id]);
        const existing = __reflectApply(__mapGet, resident, [id]);
        if (existing !== undefined) {
          if (existing[RESIDENT_SIG] === bucket.sig) {
            updateResident(existing, dv, bucket, row, arenaBase, arenaLen);
            records[records.length] = existing;
            continue;
          }
          __reflectApply(__mapDelete, resident, [id]);
        }
      }
      const rec = {};
      rec.id = id;
      // 定宽列（bool/int/uint/float/ref）一次 DataView 读即值：急切装
      // 普通数据属性比逐对象逐字段装访问器便宜得多，也免去其闭包分配。
      // 变长列（str/array/object）保持首读惰性：共享访问器 + Symbol 上下文。
      const hasVariable = bucket.lazyAccessors !== null;
      if (hasVariable) {
        rec[DECODE_CTX] = { dv, row, arenaBase, arenaLen, fields: bucket.fields };
      }
      const fields = bucket.fields;
      if (resident !== null) rec[RESIDENT_SIG] = bucket.sig;
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
    }
  }

  // 常驻记录的就地更新：ctx 换指新块（复用对象免分配），急读属性按新值
  // 重写、缺失字段删除残留，变长字段重装共享访问器（顶掉已物化的旧值）。
  // 调用前已由布局签名保证字段集、顺序与列类型不变。
  function updateResident(rec, dv, bucket, row, arenaBase, arenaLen) {
    if (bucket.lazyAccessors !== null) {
      const ctx = rec[DECODE_CTX];
      ctx.dv = dv;
      ctx.row = row;
      ctx.arenaBase = arenaBase;
      ctx.arenaLen = arenaLen;
      ctx.fields = bucket.fields;
    }
    const fields = bucket.fields;
    for (let f = 0; f < fields.length; f++) {
      const field = fields[f];
      if (bitAt(dv, field.missingAt, row)) {
        delete rec[field.key];
      } else if (field.kind < 5) {
        rec[field.key] = readField(dv, field, row, arenaBase, arenaLen);
      } else {
        __defineProperty(rec, field.key, bucket.lazyAccessors[f]);
      }
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
    forEachBlock(dv, total, (start, len) => {
      decodeBlockInto(dv, records, start, len, expectedTick, null, null);
    });
    return records;
  }

  /** 常驻版解码：resident 是跨 tick 存活的 id → 记录 Map（调用方持有，
   * 沙箱生命周期内复用）。id 命中且布局签名一致就地更新，未命中新建，
   * 本 tick 目录之外的 id 清扫出表。记录对象跨 tick 保持同一性——与
   * Memory 常驻同属已接受的观察面（依赖每拍新对象的代码在官方即坏）。 */
  function decodeBundleResident(buffer, expectedTick, resident) {
    const dv = new __DataView(buffer);
    const total = __reflectApply(__dvByteLength, dv, []);
    if (expectedTick === undefined || expectedTick === null) {
      fail('缺少信封 tick 一致性参数');
    }
    const records = [];
    const seen = new __Set();
    forEachBlock(dv, total, (start, len) => {
      decodeBlockInto(dv, records, start, len, expectedTick, resident, seen);
    });
    __reflectApply(__mapForEach, resident, [
      (_, id) => {
        if (!__reflectApply(__setHas, seen, [id])) __reflectApply(__mapDelete, resident, [id]);
      },
    ]);
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

  globalThis.__kkp.gw.kkpv = { decodeBundle, decodeBundleResident, decodePublicBundle };
})();
