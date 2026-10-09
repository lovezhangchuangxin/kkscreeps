// 第 0 tick：拉者让位；未配对的 work creep 经 move(puller) 请求移入拉者
// 原格；超载（work,work,move）与轻载（move）各走一步。方向形态 move()
// 在玩家面预检疲劳/MOVE 部件（同官方），无件移动者只有朝向相邻 creep
// 的 move(target) 形态能到达引擎仲裁，准入策略在那里覆盖原生门槛。
module.exports.loop = function () {
  if (Game.time === 1) {
    Game.creeps.puller.move(TOP);
    if (Game.creeps.f) Game.creeps.f.move(Game.creeps.puller);
    Game.creeps.heavy.move(BOTTOM);
    Game.creeps.light.move(TOP);
  }
  if (Game.time >= 2) {
    Memory.positions = {};
    for (const name of ['puller', 'f', 'heavy', 'light']) {
      const creep = Game.creeps[name];
      if (creep) Memory.positions[name] = [creep.pos.x, creep.pos.y];
    }
  }
};
