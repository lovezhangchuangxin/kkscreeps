/* 跨 API 共用弃用提示；每 tick 去重状态由该闭包独占，重置仍由原钩子调用。 */
(() => {
  'use strict';
  const _ = globalThis.__kkp.lodash;
  const deprecatedShown = [];
  // 每 tick 同一弃用消息只输出一次，使用加载时捕获的 console 对象。
  // 来源：上游 engine/src/game/game.js 的 register.deprecated。
  const __console = globalThis.console;
  function deprecated(msg) {
    if (!_.contains(deprecatedShown, msg)) {
      deprecatedShown.push(msg);
      __console.log(msg);
    }
  }

  function resetDeprecated() {
    deprecatedShown.length = 0;
  }
  globalThis.__kkp.deprecation = { deprecated, resetDeprecated };
})();
