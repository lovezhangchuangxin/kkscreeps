// 固定建造场景 bot：按阶段依次修路、补完工地、修理预伤墙、拆毁塔。
// 全部动作在射程内完成（含 moveTo 兜底）；能量不足时捡同格能量堆。
// Memory 计数器（built_road 等）供场景断言钉住各阶段的动作次数。
module.exports.loop = function () {
  Memory.loops = (Memory.loops || 0) + 1;
  const creep = Game.creeps.builder;
  if (!creep) return;
  const room = creep.room;
  const move = (target) => creep.moveTo(target, { reusePath: 20 });
  if (creep.store.energy < 20) {
    const pile = creep.pos.findClosestByRange(FIND_DROPPED_RESOURCES, {
      filter: (r) => r.resourceType === RESOURCE_ENERGY,
    });
    if (pile) {
      if (creep.pickup(pile) === ERR_NOT_IN_RANGE) move(pile);
      return;
    }
  }
  const sites = room.find(FIND_CONSTRUCTION_SITES);
  const build = (site) => {
    const result = creep.build(site);
    if (result === ERR_NOT_IN_RANGE) move(site);
    else if (result === OK)
      Memory['built_' + site.structureType] = (Memory['built_' + site.structureType] || 0) + 1;
  };
  const road = sites.find((s) => s.structureType === STRUCTURE_ROAD);
  if (road) {
    build(road);
    return;
  }
  Memory.road_done = true;
  const ext = sites.find((s) => s.structureType === STRUCTURE_EXTENSION);
  if (ext) {
    build(ext);
    return;
  }
  Memory.ext_done = true;
  const wall = room.find(FIND_STRUCTURES).find((s) => s.structureType === STRUCTURE_WALL);
  if (wall && wall.hits < 1200) {
    const result = creep.repair(wall);
    if (result === ERR_NOT_IN_RANGE) move(wall);
    else if (result === OK) Memory.repaired = (Memory.repaired || 0) + 1;
    return;
  }
  Memory.repair_done = true;
  const tower = room.find(FIND_STRUCTURES).find((s) => s.structureType === STRUCTURE_TOWER);
  if (tower) {
    const result = creep.dismantle(tower);
    if (result === ERR_NOT_IN_RANGE) move(tower);
    else if (result === OK) Memory.dismantled = (Memory.dismantled || 0) + 1;
    return;
  }
  Memory.tower_done = true;
};
