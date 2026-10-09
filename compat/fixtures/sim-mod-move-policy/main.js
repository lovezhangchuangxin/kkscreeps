// 第 0 tick 三个不同身体的 creep 挤同一目的格；之后观察胜者并记录位置。
// 默认排序按 MOVE 功率/重量比：light(2 MOVE) > mid(work,move) > heavy(2 work,move)。
module.exports.loop = function () {
  const plan = { mid: BOTTOM, light: LEFT, heavy: TOP };
  if (Game.time === 1) {
    for (const name in plan) {
      const creep = Game.creeps[name];
      if (creep) creep.move(plan[name]);
    }
  }
  if (Game.time >= 2) {
    Memory.winner = 'none';
    Memory.positions = {};
    for (const name in plan) {
      const creep = Game.creeps[name];
      if (creep) {
        Memory.positions[name] = [creep.pos.x, creep.pos.y];
        if (creep.pos.x === 10 && creep.pos.y === 11) Memory.winner = name;
      }
    }
  }
};
