// N2a 资源目录场景 bot：同一 main.js 服务两个世界——
// 安装 ore 包的世界按阶段机走 提取→转入容器→掉落→拾取 的真实闭环；
// 未安装 ore 的对照世界里，Mod 资源在玩家预检即被 ERR_INVALID_ARGS 拒绝。
exports.loop = function () {
  const creep = Game.creeps.hauler;
  const room = Game.rooms[Memory.room];
  const storage = room.find(FIND_STRUCTURES).find((s) => s.structureType === 'storage');
  if (!RESOURCES_ALL.includes('ore:crystal')) {
    // 对照世界：常量目录无此资源，动作预检直接拒绝。
    Memory.absent = true;
    Memory.rejected = creep.withdraw(storage, 'ore:crystal', 20) === ERR_INVALID_ARGS;
    Memory.drop_rejected = creep.drop('ore:crystal') === ERR_INVALID_ARGS;
    return;
  }
  Memory.saw_crystal = true;
  const container = room.find(FIND_STRUCTURES).find((s) => s.structureType === 'container');
  if (!Memory.phase) Memory.phase = 0;
  if (Memory.phase === 0) {
    const code = creep.withdraw(storage, 'ore:crystal', 20);
    Memory.withdraw_code = code;
    if (code === OK) Memory.phase = 1;
  } else if (Memory.phase === 1) {
    const code = creep.withdraw(storage, 'ore:dust', 30);
    Memory.dust_code = code;
    if (code === OK) Memory.phase = 2;
  } else if (Memory.phase === 2) {
    // 新资源转入 Generic 容器：容量来自 JS 覆盖后的 container 规则（50）。
    const code = creep.transfer(container, 'ore:dust', 30);
    Memory.transfer_code = code;
    if (code === OK) Memory.phase = 3;
  } else if (Memory.phase === 3) {
    const code = creep.drop('ore:crystal');
    Memory.drop_code = code;
    if (code === OK) Memory.phase = 4;
  } else if (Memory.phase === 4) {
    const pile = room.find(FIND_DROPPED_RESOURCES).find((p) => p.resourceType === 'ore:crystal');
    if (pile) {
      Memory.pile_seen = true;
      const code = creep.pickup(pile);
      if (code === OK) Memory.phase = 5;
    }
  } else {
    Memory.carried = creep.store['ore:crystal'];
    Memory.used = creep.store.getUsedCapacity('ore:crystal');
    Memory.container_dust = container.store['ore:dust'];
    Memory.container_cap = CONTAINER_CAPACITY;
    // 市场/跨房边界锁定：玩家预检随 RESOURCES_ALL 放行（返回 OK），
    // 结算三道门显式拒绝——订单簿保持为空、terminal 库存不动。
    if (!Memory.market_tried) {
      const terminal = room.find(FIND_STRUCTURES).find((s) => s.structureType === 'terminal');
      Memory.create_order_code = Game.market.createOrder('sell', 'ore:crystal', 1, 100, 'W1N1');
      Memory.send_code = terminal.send('ore:crystal', 10, 'W2N1');
      Memory.market_tried = true;
    }
    Memory.my_orders = Object.keys(Game.market.orders).length;
  }
};
