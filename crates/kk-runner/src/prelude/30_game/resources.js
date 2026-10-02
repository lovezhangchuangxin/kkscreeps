/* 定义掉落资源的 resourceType 与 amount，通过 getter 读取当前 tick 数据。
 * energy 别名定义在 structures_spawn.js；非 energy 资源缺少该字段时返回 undefined。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;

  // resourceType 缺省 energy；amount 缺省 0。energy 别名在 structures_spawn.js 定义。
  gw.defineGameObjectProperties(globalThis.Resource.prototype, gw.rawData, {
    resourceType: (o) => o.resourceType || 'energy',
    amount: (o) => o.amount || 0,
  });

  gw.assemblers.resource = (o, base) => base;
})();
