// 初轮发出移动，后续只观察；障碍格仍有结构对象，未通过删除障碍实现穿越。
module.exports.loop = function () {
  if (Game.time === 0) {
    for (const name of ['wall', 'rampart', 'free', 'occupied']) Game.creeps[name].move(RIGHT);
  } else {
    Memory.positions = {};
    for (const name of ['wall', 'rampart', 'free', 'occupied']) {
      const c = Game.creeps[name];
      Memory.positions[name] = [c.pos.x, c.pos.y];
    }
  }
};
