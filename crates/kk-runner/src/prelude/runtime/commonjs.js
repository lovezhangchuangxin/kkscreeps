/* 按上传模块名加载 CommonJS 模块；仅去掉 ./ 前缀，不解析 ../ 或补 .js。
 * 缓存直接保存 exports，二进制模块保存 Buffer；初始化失败删除缓存，循环加载抛错。
 * 模块表替换或入口缓存不符合要求时整体重建 require 和缓存。
 * 来源：上游 engine/src/game/game.js 的 requireFn 与入口缓存检查。
 */
(() => {
  'use strict';

  const { ops, lodash: _, bufferCtor: Buffer } = globalThis.__kkp;
  // 捕获 Function 构造器，模块编译不使用玩家后来替换的全局 Function。
  const __Function = Function;

  // 官方 user-vm.js:59：`global` = 上下文全局对象自身（官方跨 tick 存储惯例，
  // tooangel 等 bot 直接引用；创建期一次注入，玩家删改自担——同官方）
  globalThis.global = globalThis;

  let codeStamp = 0; // 官方 userCodeTimestamp 等价物（重绑时 bump）
  let latestStamp = 0;
  // 当前查看者由 game.js 的 beginTick 更新；此段加载时世界状态尚未装配。
  let currentUser;

  function makeRequire() {
    const cache = {}; // 官方 game.js:510 `{lodash: _}` 同形——值为本体
    cache.lodash = _;
    const stamp = ++codeStamp;
    latestStamp = stamp;
    function requireFn(spec) {
      // 官方 :553：非字符串在此自然抛 TypeError（.replace 不是函数）
      const name = spec.replace(/^\.\//, '');
      if (name in cache) {
        const cached = cache[name];
        if (cached === -1) {
          throw new Error("Circular reference to module '" + name + "'");
        }
        return cached;
      }
      const kind = ops.op_kk_module_kind(name);
      if (kind === 'missing') {
        // 官方消息形态（game.js:587）
        throw new Error("Unknown module '" + name + "'");
      }
      if (kind === 'binary') {
        // 官方 :560-562：二进制模块缓存值为 Buffer 本体
        const bytes = ops.op_kk_module_load_binary(name);
        cache[name] = Buffer.from(bytes);
        return cache[name];
      }
      const source = ops.op_kk_module_load_js(name);
      cache[name] = -1; // 循环占坑（官方 :565）
      // 官方 moduleObject（game.js:569-575）：exports/user/timestamp/name/code
      // 均存在——user 官方为 user._id，此处为 viewer 名（字符串同域）
      const moduleObject = {
        exports: {},
        user: currentUser,
        timestamp: stamp,
        name,
        code: source,
      };
      try {
        const wrapper = __Function(
          'module', 'exports',
          source + '\n//# sourceURL=kk_module/' + name,
        );
        // 以 undefined 作为 this 调用模块：非严格模块体会看到全局对象，严格模块体保持 undefined。
        // 来源：上游 driver/lib/runtime/runtime-driver.js 的模块包装器。
        wrapper.apply(void 0, [moduleObject, moduleObject.exports]);
      } catch (e) {
        delete cache[name];
        throw e;
      }
      cache[name] = moduleObject.exports;
      return moduleObject.exports;
    }
    requireFn.cache = cache;
    requireFn.timestamp = stamp; // 官方 game.js:512（玩家可读）
    return requireFn;
  }

  function isObjectLike(v) {
    return v !== null && (typeof v === 'object' || typeof v === 'function');
  }

  /** 官方 game.js:504-512 每 tick 重绑四条件判定（beginTick 调用）。
   *  命中即整体重绑：新 require 函数对象 + 新缓存对象（官方同形）。 */
  function ensureRequire() {
    const req = globalThis.require;
    if (typeof req !== 'function' || req.timestamp !== latestStamp
      || !isObjectLike(req.cache) || !isObjectLike(req.cache.main)
      || typeof req.cache.main.loop !== 'function') {
      globalThis.require = makeRequire();
    }
  }
  /** 模块表替换（set_modules→宿主私有句柄 resetModules）：官方 timestamp 变更路径——
   *  bump stamp 并立即重绑重建缓存。 */
  function resetForNewModules() {
    globalThis.require = makeRequire();
  }

  globalThis.require = makeRequire();
  globalThis.__kkp.cjs = {
    ensureRequire,
    resetForNewModules,
    setCurrentUser(u) { currentUser = u; },
    lodash: _,
  };
})();
