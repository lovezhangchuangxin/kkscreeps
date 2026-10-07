// G02.d2：runner（move+work+work，官方门下动一停一）与 crawler（无
// MOVE，官方门下完全静止）每 tick 尝试向南移动；规则覆盖按门独立
// 放宽。位置在每 tick 玩家期记录，反映上一 tick 结算后的状态。
module.exports.loop = function () {
  if (Game.creeps.runner) Game.creeps.runner.move(BOTTOM);
  if (Game.creeps.crawler) Game.creeps.crawler.move(BOTTOM);
  if (Game.time >= 1) {
    Memory.positions = {
      runner: [Game.creeps.runner.pos.x, Game.creeps.runner.pos.y],
      crawler: [Game.creeps.crawler.pos.x, Game.creeps.crawler.pos.y],
    };
  }
};
