/* KKPV 块束的沙箱侧可信解码器。
 * 输入是宿主经私有通道传入的每 isolate 私有拷贝（ArrayBuffer）；本段在
 * 加载时捕获全部内建（含 Error），玩家运行期改写原型或全局不影响解码，
 * 缓冲区本身不进入玩家可见面——记录只暴露解码后的值。
 * 块布局与 crates/kk-schema/src/kkpv.rs 的 v1 编码一致：160 字节头、
 * ID 升序目录、按类型分桶的列式字段、集中 arena 与单字节嵌套标签。
 * 束框架是 u32 块数 + 逐块 u32 长度目录 + 顺序拼接的块字节。
 * 记录字段惰性解码：首次读取经 getter 解码并固化为值属性，未读取的
 * 字段不产生解码成本；缺失位置位的字段不定义属性，与 JSON 参考路径
 * 的缺键形态一致。
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
    let magHi = ((0 - hi) >>> 0);
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
      if (cp < 0x80 || (b0 <= 0xdf && cp < 0x80) || (b0 >= 0xe0 && b0 <= 0xef && cp < 0x800)
        || (b0 >= 0xf0 && cp < 0x10000)) {
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
      case 0: return [null, at];
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
            value: r[0], writable: true, enumerable: true, configurable: true,
          });
          at = r[1];
        }
        return [out, at];
      }
      default: fail('未知嵌套标签 ' + tag);
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
      case 1: return readInt(dv, field.colAt + row * 8);
      case 2: return readUint(dv, field.colAt + row * 8);
      case 3: {
        const v = __reflectApply(__dvGetFloat64, dv, [field.colAt + row * 8, true]);
        if (v !== v || v === __Infinity || v === -__Infinity) fail('非有限浮点');
        return v;
      }
      case 4: {
        return idFromHalves(
          u32(dv, field.colAt + row * 8),
          u32(dv, field.colAt + row * 8 + 4),
        );
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

  // 惰性字段：getter 首读解码并固化为值属性（writable 与 JSON 记录一致，
  // setter 写入同样固化，不静默丢弃）；值属性化后不再走解码路径。
  function defineField(rec, dv, field, row, arenaBase, arenaLen) {
    __defineProperty(rec, field.key, {
      enumerable: true,
      configurable: true,
      get() {
        const v = readField(dv, field, row, arenaBase, arenaLen);
        __defineProperty(rec, field.key, {
          value: v, writable: true, enumerable: true, configurable: true,
        });
        return v;
      },
      set(v) {
        __defineProperty(rec, field.key, {
          value: v, writable: true, enumerable: true, configurable: true,
        });
      },
    });
  }

  // 解析单块并把记录按目录顺序追加进 records；expectedTick 与信封的
  // world.time 一致性在此交叉校验（块与信封来自同一次投影）。
  function decodeBlockInto(dv, records, start, len, expectedTick) {
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
    // 段边界：零长度段 offset 须为零；非零段落在 [头长, 块长) 且两两不重叠。
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
    if (objectCount * 16 !== directory[1]) fail('目录长度与对象数不符');
    const arenaBase = start + arena[0];
    const arenaLen = arena[1];

    // 类型桶：16 字节桶头 + 12 字节字段项 + 缺失/null 位图 + 定宽列，
    // 消费量须与桶段长度精确一致。
    let cur = start + buckets[0];
    const bucketsEnd = cur + buckets[1];
    const bucketList = [];
    for (let t = 0; t < typeCount; t++) {
      if (cur + 16 > bucketsEnd) fail('桶头越界');
      const rows = u32(dv, cur + 8);
      const fieldCount = u32(dv, cur + 12);
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
          key: null, kind,
          keyOff: u32(dv, cur), keyLen: u32(dv, cur + 4),
          missingAt: 0, nullAt: 0, colAt: 0,
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
      for (let f = 0; f < fieldCount; f++) {
        if (fields[f].keyOff + fields[f].keyLen > arenaLen) fail('字段键描述符越界');
        fields[f].key = utf8(dv, arenaBase + fields[f].keyOff, fields[f].keyLen);
        // "id" 由目录承担；字段键不得覆盖记录身份（与编码端拒绝一致）。
        if (fields[f].key === 'id') fail('字段键不得为 id');
      }
      bucketList[bucketList.length] = { rows, fields };
    }
    if (cur !== bucketsEnd) fail('桶段消费不符');

    // 目录：ID 升序；句柄与行号须落在已解析桶内。
    const dirAt = start + directory[0];
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
      const rec = {};
      __defineProperty(rec, 'id', {
        value: idFromHalves(lo, hi),
        writable: true, enumerable: true, configurable: true,
      });
      for (let f = 0; f < bucket.fields.length; f++) {
        const field = bucket.fields[f];
        if (!bitAt(dv, field.missingAt, row)) {
          defineField(rec, dv, field, row, arenaBase, arenaLen);
        }
      }
      records[records.length] = rec;
    }
  }

  /** 解码整束并返回记录数组；块的房间顺序与目录的 ID 顺序共同决定
   * 数组顺序，与 JSON 参考路径的对象遍历序一致。expectedTick 是信封
   * 的 world.time，逐块交叉校验。 */
  function decodeBundle(buffer, expectedTick) {
    const dv = new __DataView(buffer);
    const total = __reflectApply(__dvByteLength, dv, []);
    if (expectedTick === undefined || expectedTick === null) {
      fail('缺少信封 tick 一致性参数');
    }
    if (total < 4) fail('束短于框架头部');
    const count = u32(dv, 0);
    const lens = [];
    for (let i = 0; i < count; i++) lens[lens.length] = u32(dv, 4 + i * 4);
    let cur = 4 + count * 4;
    const records = [];
    for (let i = 0; i < count; i++) {
      decodeBlockInto(dv, records, cur, lens[i], expectedTick);
      cur += lens[i];
    }
    if (cur !== total) fail('束长度与块目录不符');
    return records;
  }

  globalThis.__kkp.gw.kkpv = { decodeBundle };
})();
