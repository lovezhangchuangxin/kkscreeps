/* kk-runner prelude · 30_game 段 7：Resource 装配（对照官方
 * engine/src/game/resources.js——掉落资源对象）。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;

  gw.assemblers.resource = (o, base) => Object.assign(base, {
    resourceType: o.resourceType || 'energy',
    amount: o.amount || 0,
  });
})();
