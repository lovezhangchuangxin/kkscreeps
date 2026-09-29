/* kk-runner prelude · 段 2：CommonJS 加载器（官方 requireFn 同形）。
 *
 * 官方形态（engine/src/game/game.js:551-594 + driver/lib/runtime/runtime-driver.js:65）：
 * - 名字归一仅剥 `./` 前缀（:553）——无 `../` 解析、无 `.js` 补全，模块名即上传名；
 * - require.cache 是普通对象字面量（Object.prototype 域），键=模块名，值=
 *   **exports 本体**（二进制为 Buffer——:562），非 {exports} 包装；lodash 预置本体；
 * - 循环 require：占坑 -1，命中抛 `Circular reference to module '...'`（:565/:590-592）
 *   ——A5 前我方为 Node 半成品导出语义，已按硬约束①对齐官方（灰区清偿）；
 * - 执行失败删缓存重抛（:578-581）——瞬时初始化错误下一 tick 重跑可自愈；
 * - 模块包装 `(function __module(module,exports){…})(__module,__module.exports)`
 *   （runtime-driver.js:65）：只传两参、this=undefined（非严格模块体落到
 *   globalThis）；require/global/`_` 从全局取（它们本就是全局）；
 * - 官方 game.js:504-512 每 tick 重绑判定：require 缺失 ∨ userCodeTimestamp 变 ∨
 *   cache.main 非对象 ∨ cache.main.loop 非函数 → 重绑 require 并**整体重建缓存
 *   对象**（{lodash:_}）。我方等价物：模块表替换（set_modules→__kk.resetModules）
 *   bump codeStamp；beginTick 经 ensureRequire() 跑同款四条件判定。整体重建
 *   （而非逐键 delete）同时免疫玩家冻结 require.cache（对抗评审 P2）。
 */
(() => {
  'use strict';

  const { ops, lodash: _, bufferCtor: Buffer } = globalThis.__kkp;
  // 模块加载是宿主基础设施：编译经捕获的 Function 构造器（官方走驱动侧
  // compileScriptSync，不经玩家可交换的 realm 全局 Function）
  const __Function = Function;

  // 官方 user-vm.js:59：`global` = 上下文全局对象自身（官方跨 tick 存储惯例，
  // tooangel 等 bot 直接引用；创建期一次注入，玩家删改自担——同官方）
  globalThis.global = globalThis;

  let codeStamp = 0; // 官方 userCodeTimestamp 等价物（重绑时 bump）
  let latestStamp = 0;
  // moduleObject.user 的取值通道：20_cjs 加载期 gw 尚未创建（_registry 是
  // 30_game 首文件），且 __kkp 在末段删除——由 game.js beginTick 经钩子注入
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
        // 官方 sloppy 直调 `(fn)(module, module.exports)`：this=undefined——非严格
        // 模块体落到 globalThis、模块体自带 'use strict' 时保持 undefined；
        // apply(void 0) 精确复刻
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
  /** 模块表替换（set_modules→__kk.resetModules）：官方 timestamp 变更路径——
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
