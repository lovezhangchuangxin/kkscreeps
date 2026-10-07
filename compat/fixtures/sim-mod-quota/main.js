// 三轮探针：首轮连建 10 tower / 5 wall 工地（单轮内预检只看已提交
// 对象，全放行；结算按配额收口）；次轮各补建一个——此刻预检读
// 「已提交数 vs 注入配额表」，是预检与结算同源的判定点（10 个已提交
// tower：官方 6 拒，覆盖 20 放行；3 个已提交 wall：官方 2500 放行，
// 覆盖 3 拒）；第三轮回读已提交世界的工地数，核对结算配额门。
exports.loop = function () {
  const room = Game.rooms.W1N1;
  if (!Memory.phase) {
    Memory.quotaTower8 = CONTROLLER_STRUCTURES.tower['8'];
    Memory.quotaWall8 = CONTROLLER_STRUCTURES.constructedWall['8'];
    let towers = 0;
    for (let i = 0; i < 10; i++) {
      if (room.createConstructionSite(2 + i, 3, STRUCTURE_TOWER) === OK) towers++;
    }
    Memory.towerOkFirstTick = towers;
    let walls = 0;
    for (let i = 0; i < 5; i++) {
      if (room.createConstructionSite(30 + i, 20, STRUCTURE_WALL) === OK) walls++;
    }
    Memory.wallOkFirstTick = walls;
    Memory.phase = 1;
    return;
  }
  if (Memory.phase === 1) {
    Memory.towerExtra = room.createConstructionSite(2, 5, STRUCTURE_TOWER);
    Memory.wallExtra = room.createConstructionSite(30, 22, STRUCTURE_WALL);
    Memory.phase = 2;
    return;
  }
  Memory.worldTowerSites = room.find(FIND_CONSTRUCTION_SITES)
    .filter((s) => s.structureType === STRUCTURE_TOWER).length;
  Memory.worldWallSites = room.find(FIND_CONSTRUCTION_SITES)
    .filter((s) => s.structureType === STRUCTURE_WALL).length;
};
