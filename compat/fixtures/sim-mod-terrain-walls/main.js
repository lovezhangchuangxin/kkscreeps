// 墙列 x=25 全高；runner 从 (22,25) 向 (28,25) 移动。声明开启时
// 路径穿墙 6 步抵达；默认物理地形时目标不可达，沿墙面绕行靠近但
// 不越墙列。首轮同时记录 PathFinder.search 的直连结果与 getTerrain
// 物理查询值——物理查询不随声明改写，四种场景都应为
// TERRAIN_MASK_WALL(1)。
module.exports.loop = function () {
  const creep = Game.creeps.runner;
  if (Game.time === 1) {
    const ret = PathFinder.search(new RoomPosition(22, 25, 'W1N1'), {
      pos: new RoomPosition(28, 25, 'W1N1'),
      range: 0,
    });
    Memory.pf = { len: ret.path.length, incomplete: ret.incomplete };
    Memory.terrainWall = Game.rooms.W1N1.getTerrain().get(25, 25);
  }
  if (creep) {
    creep.moveTo(28, 25, { reusePath: 0 });
    if (Game.time >= 7) {
      Memory.position = [creep.pos.x, creep.pos.y];
    }
  }
};
