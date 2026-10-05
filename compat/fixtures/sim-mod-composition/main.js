exports.loop = function () {
  const counters = Game.rooms[Memory.room].find(FIND_COMPOSITION_COUNTERS);
  if (counters.length !== 1 || !(counters[0] instanceof CompositionCounter)) throw new Error('counter API missing');
  if (Game.time > 0 && counters[0].trace !== 'ZA([C])az') throw new Error('composition order differs');
  Memory.loops = (Memory.loops || 0) + 1;
  Memory.value = counters[0].value;
  Memory.trace = counters[0].trace;
};
