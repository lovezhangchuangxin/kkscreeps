exports.loop = function () {
  const healer = Game.creeps.healer;
  const target = Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0];
  if (Game.time === 1) Memory.attackCode = healer.attack(target);
  if (Game.time === 2) Memory.healCode = healer[Memory.method](target);
  Memory.targetHits = target.hits;
  Memory.targetBodyHits = target.body.map((part) => part.hits);
};
