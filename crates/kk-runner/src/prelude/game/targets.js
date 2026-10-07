/* 当前 tick 的共用目标检查；raw 类型存在性与公开类实例检查由调用方分别完成。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const _ = globalThis.__kkp.lodash;
  // 官方 register.assertTargetObject（game.js:122-126）：Memory 序列化壳拦截
  // （isPlainObject && isString(id) && id.length == 24 → throw）。24 判按官方
  // 原样保留——我方 id 是 18 字符 kk 前缀（自身格式的序列化壳走 INVALID_TARGET
  // 分支），从 vanilla 迁移的 bot 其 Memory 残留 24 字符 Mongo id 同样命中。
  function assertTargetObject(obj) {
    if (obj && _.isPlainObject(obj) && _.isString(obj.id) && obj.id.length === 24) {
      throw new Error("It seems you're trying to use a serialized game object stored in Memory which is not allowed. Please use `Game.getObjectById` to retrieve a live object reference instead.");
    }
  }

  /** 官方 register.creeps[target.id] 判定的等价物：raw 投影里存在且是 creep
   *  （官方 register.creeps 收全部可见 creep，不分敌我）。 */
  function isKnownCreep(id) {
    const o = id == null ? undefined : gw.rawById.get(id);
    return !!o && o.type === 'creep';
  }

  /** 检查 id 是否存在于当前 tick 的 raw 数据且 type 为 structure 或 spawn。
   * Source/Mineral 也使用 raw type=structure，本函数不能独立验证完整的结构目标范围；
   * 调用方还需检查 instanceof 或排除其 structureType。 */
  function isKnownStructure(id) {
    const o = id == null ? undefined : gw.rawById.get(id);
    return !!o && (o.type === 'structure' || o.type === 'spawn');
  }
  globalThis.__kkp.targets = { assertTargetObject, isKnownCreep, isKnownStructure };
})();
