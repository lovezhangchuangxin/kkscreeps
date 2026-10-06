// 每个tick 两个 (move,work) 重量 1 的移动者向右：scout 从 (19,25)
// 进入沼泽带（x≥20），walker 在纯平原 (5,25)。玩家代码先于结算执行，
// time>=3 的记录时点已结算 3 次；配合 --ticks 4 使断言反映 3 次落位。
module.exports.loop = function () {
  Game.creeps.scout.move(RIGHT);
  Game.creeps.walker.move(RIGHT);
  if (Game.time >= 3) {
    Memory.positions = {};
    for (const name of ['scout', 'walker']) {
      const creep = Game.creeps[name];
      if (creep) Memory.positions[name] = [creep.pos.x, creep.pos.y];
    }
  }
};
