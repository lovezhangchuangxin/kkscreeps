/* kk-runner 桩环境 prelude · 段 2：CommonJS 加载器。
 *
 * 官方形态：引擎自制 requireFn（engine/src/game/game.js），模块缓存跨 tick、
 * 循环 require 拿半成品 exports（**Node 语义**；官方为抛 "Circular reference to
 * module" 错——两者不一致，行为跟谁已在 compatibility.md 灰区表登记，M2 拍板）、
 * require.cache 预置 lodash。
 * 相对路径按真实规则解析（spike 垫片的扁平解析升级），二进制模块返回 Buffer。
 */
(() => {
  'use strict';

  const { ops, lodash: _, bufferCtor: Buffer } = globalThis.__kkp;

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
    // 官方消息形态（game.js:587 requireFn：`Unknown module '...'`；Node 形态是
    // Cannot find module——bot/用例可能按官方文本匹配错误）
    throw new Error("Unknown module '" + spec + "'");
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
  globalThis.__kkp.cjs = { require, moduleCache, lodash: _ };
})();
