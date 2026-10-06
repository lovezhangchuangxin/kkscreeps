// 第 0 tick：w1 向右进入西区边缘格；strong（move,move）与 weak
// （move）竞争 (40,19)；free 独自向西。之后各 tick 只记录位置。
module.exports.loop = function () {
  if (Game.time === 0) {
    Game.creeps.w1.move(RIGHT);
    Game.creeps.strong.move(TOP);
    Game.creeps.weak.move(TOP_LEFT);
    Game.creeps.free.move(LEFT);
  }
  if (Game.time >= 1) {
    Memory.positions = {};
    for (const name of ['w1', 'strong', 'weak', 'free']) {
      const creep = Game.creeps[name];
      if (creep) Memory.positions[name] = [creep.pos.x, creep.pos.y];
    }
  }
};
