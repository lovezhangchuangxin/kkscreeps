// 只容忍需要移动或同轮库存竞争的返回码，其余错误必须使场景失败。
exports.perform = function (creep, target, method) {
  const resourceAction = method === 'transfer' || method === 'withdraw';
  const rc = resourceAction ? creep[method](target, RESOURCE_ENERGY) : creep[method](target);
  if (rc === ERR_NOT_IN_RANGE) {
    const range = method === 'upgradeController' ? 3 : 1;
    const moved = creep.moveTo(target, { range, reusePath: 5 });
    if (moved !== OK && moved !== ERR_TIRED) throw new Error('move failed: ' + moved);
  } else if (rc !== OK && rc !== ERR_FULL && rc !== ERR_NOT_ENOUGH_RESOURCES) {
    throw new Error(method + ' failed: ' + rc);
  }
};
