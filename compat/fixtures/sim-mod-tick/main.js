exports.loop = function () {
  const objects = Game.rooms[Memory.room].find(FIND_CLOCKWORK_COUNTERS);
  if (objects.length !== 1 || !(objects[0] instanceof ClockworkCounter)) {
    throw new Error('counter query or player class missing');
  }
  Memory.value = objects[0].value;
  Memory.loops = (Memory.loops || 0) + 1;
};
