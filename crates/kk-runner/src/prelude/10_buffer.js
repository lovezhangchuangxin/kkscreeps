/* kk-runner 桩环境 prelude · 段 1：Buffer 垫片。
 *
 * 官方沙箱可用 Buffer（bot 生态依赖，overmind 的 wasm 模块经 Buffer 使用）。
 * M0 只实现常见子集；缺失方法由缺失 API 探测兜底报告。
 * 原始类（未套仪表的 Buffer 构造器）经 __kkp 暴露给 20_cjs 的二进制模块链路
 * （与单文件时期闭包引用同一语义——玩家改写 globalThis.Buffer 不影响该链路）。
 */
(() => {
  'use strict';

  const { probe } = globalThis.__kkp;

  function utf8Encode(str) {
    const out = [];
    for (let i = 0; i < str.length; i++) {
      let c = str.codePointAt(i);
      if (c > 0xffff) i++;
      // 孤代理（合法对的 codePointAt 返回合并码点，落代理区间的必是孤代理）：
      // Node 输出 U+FFFD 的 UTF-8 字节，而非 CESU-8 形态
      else if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return out;
  }
  function utf8Decode(bytes) {
    // 对齐 Node/V8（well-formed UTF-8 解码 + maximal-subpart 替换策略）：
    // - 非法/截断序列产出 U+FFFD，替换粒度 = 已验证的合法前缀（截断的 3 字节序列
    //   整体一个 FFFD，而非每字节一个）；
    // - 第二字节按 leading 的受限范围校验（E0≥A0、ED≤9F、F0≥90、F4≤8F），
    //   overlong/代理区/超界由此天然排除。
    const REPL = '\ufffd';
    let s = '';
    let i = 0;
    const n = bytes.length;
    while (i < n) {
      const b = bytes[i];
      if (b < 0x80) { s += String.fromCharCode(b); i += 1; continue; }
      let size, lo2, hi2;
      if (b >= 0xc2 && b <= 0xdf) { size = 2; lo2 = 0x80; hi2 = 0xbf; }
      else if (b === 0xe0) { size = 3; lo2 = 0xa0; hi2 = 0xbf; }
      else if (b >= 0xe1 && b <= 0xec) { size = 3; lo2 = 0x80; hi2 = 0xbf; }
      else if (b === 0xed) { size = 3; lo2 = 0x80; hi2 = 0x9f; }
      else if (b === 0xf0) { size = 4; lo2 = 0x90; hi2 = 0xbf; }
      else if (b >= 0xf1 && b <= 0xf3) { size = 4; lo2 = 0x80; hi2 = 0xbf; }
      else if (b === 0xf4) { size = 4; lo2 = 0x80; hi2 = 0x8f; }
      else { s += REPL; i += 1; continue; } // 0xc0/0xc1（overlong）与 0xf5-0xff：非法 leading
      const b2 = bytes[i + 1];
      if (b2 === undefined || b2 < lo2 || b2 > hi2) {
        s += REPL; i += 1; continue; // 第二字节非法或截断：只吃 leading 一个字节
      }
      let cp = ((b & (size === 2 ? 0x1f : size === 3 ? 0x0f : 0x07)) << 6) | (b2 & 0x3f);
      let j = i + 2;
      while (j < n && j - i < size && (bytes[j] & 0xc0) === 0x80) {
        cp = (cp << 6) | (bytes[j] & 0x3f);
        j += 1;
      }
      if (j - i < size) { s += REPL; i = j; continue; } // 截断：已验证前缀整体一个 FFFD
      s += String.fromCodePoint(cp);
      i = j;
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
          // Node 宽松语义：奇数长度静默截断最后一个半字节；非法字符处停止解码
          bytes = [];
          const len = input.length - (input.length % 2);
          for (let i = 0; i < len; i += 2) {
            const hi = hexVal(input[i]), lo = hexVal(input[i + 1]);
            if (hi < 0 || lo < 0) break;
            bytes.push((hi << 4) | lo);
          }
        } else if (enc === 'base64') {
          // Node 宽松语义：跳过字母表外的字符继续解码
          const clean = input.replace(/\s/g, '');
          bytes = [];
          let buf = 0;
          let bits = 0;
          for (const ch of clean) {
            if (ch === '=') break;
            const v = base64Val(ch);
            if (v < 0) continue;
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
        // Node 语义：对象形态按 array-like 处理（取 length 与索引），否则报错
        if (typeof input.length !== 'number') {
          throw new TypeError('Buffer.from: 非数组对象必须带 length（array-like）');
        }
        // 负 length 钳 0（Node 实测 Buffer.from({length:-1}) 得空 Buffer）：
        // >>> 0 会把 -1 wrap 成 4294967295 —— TypedArray backing store 是 V8 堆外
        // 内存，near-heap 闸门管不住，一次调用即分配 4GB
        const len = input.length < 0 ? 0 : input.length >>> 0;
        const b = new Buffer(len);
        for (let k = 0; k < len; k++) {
          const v = input[k];
          b[k] = v === undefined ? 0 : v;
        }
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
    // Node Buffer 的 toJSON（官方沙箱的 binary 模块链路就是 Node Buffer）：
    // 缺失时 JSON.stringify(buf) 得 TypedArray 形态 {"0":104,"1":105}
    toJSON() {
      return { type: 'Buffer', data: Array.from(this) };
    }
  }
  // Buffer 静态面套探测仪表：官方缺失的静态成员（如 poolAlloc）以 'Buffer.*' 登记
  globalThis.Buffer = probe(Buffer, 'Buffer');
  globalThis.__kkp.bufferCtor = Buffer;
})();
