const counters = globalThis.__benchmarkCounters ||= {ticks: 0, spawn: 0, harvest: 0, upgrade: 0};
counters.ticks++;
const spawn = Object.values(Game.spawns)[0];
if (spawn && !spawn.spawning && spawn.store.energy >= 200 && Object.keys(Game.creeps).length < 6) {
  const result = spawn.spawnCreep(['work', 'carry', 'move'], 'w' + Game.time + '_' + spawn.name, {directions: [TOP, LEFT]});
  if (result !== OK) throw new Error('spawnCreep: ' + result);
  counters.spawn++;
}
for (const c of Object.values(Game.creeps)) {
  if (c.spawning) continue;
  let result;
  if (c.store.energy > 0) {
    result = c.upgradeController(c.room.controller);
    counters.upgrade++;
  } else {
    const source = c.room.find(FIND_SOURCES).sort((a, b) => a.pos.y - b.pos.y || a.pos.x - b.pos.x)[0];
    result = c.harvest(source);
    counters.harvest++;
  }
  if (result !== OK) throw new Error('worker action: ' + result);
}
