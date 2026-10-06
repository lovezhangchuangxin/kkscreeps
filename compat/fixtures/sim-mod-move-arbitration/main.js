// 第 0 tick：a/b 交换原格；stranger 请求移入 parked 占据的格（门槛
// 获准、链内必被取消——准入输入不含占位信息，只有链预演可见）；
// strong/weak 竞争同一目的格（strong 功率高胜出，weak 落选）。
// 之后各 tick 只记录位置。
module.exports.loop = function () {
  if (Game.time === 0) {
    Game.creeps.a.move(TOP);
    Game.creeps.b.move(BOTTOM);
    Game.creeps.stranger.move(TOP);
    Game.creeps.strong.move(TOP);
    Game.creeps.weak.move(TOP_LEFT);
  }
  if (Game.time >= 1) {
    Memory.positions = {};
    for (const name of ['a', 'b', 'stranger', 'parked', 'strong', 'weak']) {
      const creep = Game.creeps[name];
      if (creep) Memory.positions[name] = [creep.pos.x, creep.pos.y];
    }
  }
};
