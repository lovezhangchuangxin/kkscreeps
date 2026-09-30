/* kk-runner prelude · 30_game 段 7：Resource 装配（对照官方
 * engine/src/game/resources.js——掉落资源对象）。
 *
 * A3b：字段表挂 Resource.prototype（官方 resources.js:38-42），getter 经
 * gw.rawData(this.id) 回查当前 tick 投影。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;

  // 官方表：resourceType（缺省 energy）+ amount。M2B 扩面 P4：`energy` 别名
  // （官方 resources.js:36 `o.energy`）已真化，挂在 structures_spawn.js 的
  // Resource 段——energy 堆投影带 `energy=amount`、非 energy 堆无该键
  // （=undefined，官方同形）
  gw.defineGameObjectProperties(globalThis.Resource.prototype, gw.rawData, {
    resourceType: (o) => o.resourceType || 'energy',
    amount: (o) => o.amount || 0,
  });

  gw.assemblers.resource = (o, base) => base;
})();
