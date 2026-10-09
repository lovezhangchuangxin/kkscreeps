/* 跨 API 共用弃用提示；每 tick 去重状态由该闭包独占，重置仍由原钩子调用。
 * 玩家覆写共享 lodash 的 indexOf/contains 或 Array.prototype.push 可劫持
 * 去重（多打/抑制自家告警）——官方 register.deprecated 同用全局共享 _，
 * 官方同构的自伤面；消息文本恒为六条常量串且每 tick 重置，无越界放大。
 * 官方 game.js:118 调用时解析 globals.console（玩家重绑 console 全局会使
 * 告警改道甚至丢失）；本仓装配期捕获收集型 console 是有意加固——告警
 * 恒进宿主捕获通道，不受玩家重绑影响（登记分歧，套件不测玩家重绑）。 */
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
