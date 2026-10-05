// 第 0 tick：交换让位链 a(10,10)↔b(10,11) 互进对方原格，c(30,30)
// 南下一步。之后各 tick 只记录位置。障碍策略只追加阻挡，让位链
// 传播（追加取消 a 后 b 失去让位依赖）由引擎完成。
module.exports.loop = function () {
  if (Game.time === 0) {
    Game.creeps.a.move(BOTTOM);
    Game.creeps.b.move(TOP);
    Game.creeps.c.move(BOTTOM);
  }
  if (Game.time >= 1) {
    Memory.positions = {};
    for (const name of ['a', 'b', 'c']) {
      const creep = Game.creeps[name];
      if (creep) Memory.positions[name] = [creep.pos.x, creep.pos.y];
    }
  }
};
