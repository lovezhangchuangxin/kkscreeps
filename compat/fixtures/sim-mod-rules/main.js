exports.loop = function () {
  const meters = Game.rooms[Memory.room].find(FIND_RULES_METERS);
  if (meters.length !== 1 || !(meters[0] instanceof RulesMeter))
    throw new Error('meter API missing');
  Memory.value = meters[0].value;
  Memory.pool = meters[0].pool;
};
