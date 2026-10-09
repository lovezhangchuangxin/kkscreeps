exports.loop = function () {
  if (Game.time === 1) Memory.code = Game.creeps.attacker.rangedMassAttack();
  const room = Game.rooms.W1N1;
  const enemies = room.find(FIND_HOSTILE_CREEPS);
  Memory.nearHits = enemies.find((creep) => creep.name === 'near').hits;
  Memory.farHits = enemies.find((creep) => creep.name === 'far').hits;
  Memory.containerHits = room.lookForAt(LOOK_STRUCTURES, 12, 10)[0].hits;
  Memory.rampartHits = room.lookForAt(LOOK_STRUCTURES, 14, 10)[0].hits;
  Memory.allyHits = Game.creeps.ally.hits;
};
