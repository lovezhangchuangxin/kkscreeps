// 固定 PvP 战斗场景 bot：近战/远程/结构目标选择与真实寻路移动。
// 双方共用同一模块，按 Memory.side 区分；不建造、不补给，纯战斗消耗。
// 结构目标只选 spawn：控制器无耐久面，列入目标会让攻击静默空转。
module.exports.loop = function () {
  Memory.loops = (Memory.loops || 0) + 1;
  const room = Game.rooms.W1N1;
  if (!room) return;
  for (const creep of Object.values(Game.creeps)) {
    if (creep.spawning) continue;
    const foe = creep.pos.findClosestByRange(FIND_HOSTILE_CREEPS);
    if (foe) {
      if (creep.pos.inRangeTo(foe.pos, 1)) {
        creep.attack(foe);
      } else if (
        creep.body.some((p) => p.type === RANGED_ATTACK) &&
        creep.pos.inRangeTo(foe.pos, 3)
      ) {
        creep.rangedAttack(foe);
      } else {
        creep.moveTo(foe, { reusePath: 50 });
      }
      continue;
    }
    const spawn = room
      .find(FIND_HOSTILE_STRUCTURES)
      .find((s) => s.structureType === STRUCTURE_SPAWN);
    if (spawn) {
      if (creep.pos.inRangeTo(spawn.pos, 1)) creep.attack(spawn);
      else creep.moveTo(spawn.pos, { reusePath: 50 });
    }
  }
};
