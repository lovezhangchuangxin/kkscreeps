// G08.a 建筑字段玩家面：清单装配的存储键（含冒号命名空间）直供玩家。
// 玩家代码先于结算：本轮读到的是此前各轮结算后的终值。手放结构不带
// 自定义键（完工默认只在建造流写入），首写由 tick 整集补齐。
module.exports.loop = function () {
  // time>=3 才记录（已结算 3 轮）；--ticks 2 的检查点保存轮不满足
  // 断言，退出码 2，冷恢复后补齐。
  if (Game.time < 4) return;
  const room = Game.rooms['W1N1'];
  const gauge = room.find(FIND_STRUCTURES).filter((s) => s.structureType === 'bl:gauge')[0];
  const beacon = room.find(FIND_STRUCTURES).filter((s) => s.structureType === 'bk:beacon')[0];
  Memory.hasGauge = !!gauge;
  Memory.hasBeacon = !!beacon;
  if (gauge) {
    Memory.gauge = {
      heat: gauge['bl:heat'],
      cycles: gauge['bl:cycles'],
      energy: gauge.store ? gauge.store.energy : null,
      cls: gauge.constructor.name,
    };
  }
  if (beacon) {
    Memory.beacon = {
      signal: beacon['bk:signal'],
      flashes: beacon['bk:flashes'],
      cls: beacon.constructor.name,
    };
  }
};
