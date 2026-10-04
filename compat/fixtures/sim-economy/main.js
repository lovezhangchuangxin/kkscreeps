// 固定经济场景 bot：真实寻路、采集、搬运、升级及死亡后的补员。
const actions = require('actions');

module.exports.loop = function () {
  Memory.loops = (Memory.loops || 0) + 1;
  const room = Game.rooms.W1N1;
  const spawn = Game.spawns.Spawn1;
  const source = room.find(FIND_SOURCES)[0];
  const container = room.find(FIND_STRUCTURES).find(s => s.structureType === STRUCTURE_CONTAINER);

  if (!spawn.spawning) {
    for (const role of ['miner', 'upgrader']) {
      if (!Game.creeps[role]) {
        const rc = spawn.spawnCreep([WORK, CARRY, MOVE], role, { memory: { role } });
        if (rc === OK) Memory.spawned = (Memory.spawned || 0) + 1;
        else if (rc !== ERR_NOT_ENOUGH_ENERGY) throw new Error('spawn failed: ' + rc);
        break;
      }
    }
  }

  for (const creep of Object.values(Game.creeps)) {
    if (creep.spawning) continue;
    if (creep.memory.role === 'miner') {
      if (creep.memory.delivering && !creep.store.energy) creep.memory.delivering = false;
      if (!creep.store.getFreeCapacity()) creep.memory.delivering = true;
      if (creep.memory.delivering) {
        const target = spawn.store.getFreeCapacity(RESOURCE_ENERGY) ? spawn : container;
        if (target.store.getFreeCapacity(RESOURCE_ENERGY)) actions.perform(creep, target, 'transfer');
      } else if (source.energy) {
        actions.perform(creep, source, 'harvest');
      }
    } else if (creep.memory.role === 'upgrader') {
      if (creep.memory.upgrading && !creep.store.energy) creep.memory.upgrading = false;
      if (!creep.store.getFreeCapacity() || (creep.store.energy && !container.store.energy)) creep.memory.upgrading = true;
      if (creep.memory.upgrading) actions.perform(creep, room.controller, 'upgradeController');
      else if (container.store.energy) actions.perform(creep, container, 'withdraw');
    } else {
      throw new Error('unknown role: ' + creep.name);
    }
  }
};
