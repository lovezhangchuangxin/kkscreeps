// 工作量账本只记录采集窗口；初始化与预热仍执行完全相同的玩家逻辑。
const counters = (globalThis.__benchmarkCounters ||= { ticks: 0, rooms: {} });
counters.ticks++;
Memory.creeps ||= {};
Memory.benchmark ||= { padding: 'm'.repeat(8192), rooms: {} };
const measured = Game.time > BENCH.warmup;
const sorted = (items) => items.sort((a, b) => a.pos.y - b.pos.y || a.pos.x - b.pos.x);
const ownedRooms = Object.values(Game.rooms).filter((r) => r.controller && r.controller.my);
function record(ledger, action, result) {
  if (result === OK && measured) ledger[action] = (ledger[action] || 0) + 1;
  return result;
}
const navigation = {};
const layouts = (globalThis.__benchmarkLayouts ||= {});
const blockedMatrix = (globalThis.__benchmarkBlockedMatrix ||= (() => {
  const matrix = new PathFinder.CostMatrix();
  for (let y = 0; y < 50; y++) for (let x = 0; x < 50; x++) matrix.set(x, y, 255);
  return matrix;
})());
const directions = [
  [0, -1],
  [1, -1],
  [1, 0],
  [1, 1],
  [0, 1],
  [-1, 1],
  [-1, 0],
  [-1, -1],
];
function travel(creep, target, range, ledger) {
  if (creep.pos.inRangeTo(target, range)) return false;
  const nav = navigation[creep.room.name];
  const memory = (Memory.creeps[creep.name] ||= {});
  const key = target.pos.x + ',' + target.pos.y + ':' + range;
  let path = memory.route;
  if (
    memory.goal !== key ||
    !path ||
    Game.time - memory.routeTick >= 20 ||
    !path.length ||
    Math.max(Math.abs(path[0][0] - creep.pos.x), Math.abs(path[0][1] - creep.pos.y)) > 1
  ) {
    const start = creep.pos.y * 50 + creep.pos.x;
    const queue = [start];
    const parents = new Int16Array(2500).fill(-1);
    parents[start] = start;
    let end = -1;
    for (let i = 0; i < queue.length; i++) {
      const cell = queue[i],
        x = cell % 50,
        y = Math.floor(cell / 50);
      if (Math.max(Math.abs(x - target.pos.x), Math.abs(y - target.pos.y)) <= range) {
        end = cell;
        break;
      }
      for (const [dx, dy] of directions) {
        const nx = x + dx,
          ny = y + dy,
          next = ny * 50 + nx;
        if (nx < 1 || nx > 48 || ny < 1 || ny > 48 || parents[next] !== -1 || nav.blocked.has(next))
          continue;
        parents[next] = cell;
        queue.push(next);
      }
    }
    path = [];
    if (end !== -1)
      for (let cell = end; cell !== start; cell = parents[cell])
        path.unshift([cell % 50, Math.floor(cell / 50)]);
    // 同一候选路线作为 costMatrix 走廊，仍调用原生寻路；避免实现各自的同长路径择优差异改变工作量。
    if (path.length) {
      const matrix = blockedMatrix.clone();
      for (const [x, y] of path) matrix.set(x, y, 1);
      matrix.set(creep.pos.x, creep.pos.y, 1);
      const goal = path[path.length - 1];
      const result = PathFinder.search(
        creep.pos,
        { pos: new RoomPosition(...goal, creep.room.name), range: 0 },
        {
          maxRooms: 1,
          roomCallback: () => matrix,
        },
      );
      if (result.incomplete) throw new Error('Common corridor path incomplete');
      if (result.path.length !== path.length)
        throw new Error('Common corridor path length mismatch');
      // 同长路线可以有不同择优；实际移动沿共享 BFS 路线，保留两端相同运输与结算工作量。
      if (measured) ledger.pathSearch = (ledger.pathSearch || 0) + 1;
    }
    memory.goal = key;
    memory.route = path;
    memory.routeTick = Game.time;
  }
  while (path.length && path[0][0] === creep.pos.x && path[0][1] === creep.pos.y) path.shift();
  if (path.length) {
    const [x, y] = path[0];
    const cell = y * 50 + x;
    if (!nav.blocked.has(cell) && !nav.reserved.has(cell)) {
      nav.reserved.add(cell);
      record(ledger, 'move', creep.move(creep.pos.getDirectionTo(x, y)));
    }
  }
  return true;
}
function take(creep, target, resource, ledger) {
  if (travel(creep, target, 1, ledger)) return;
  record(ledger, 'withdraw', creep.withdraw(target, resource));
}
function give(creep, target, resource, ledger) {
  if (travel(creep, target, 1, ledger)) return;
  record(ledger, 'transfer', creep.transfer(target, resource));
}
for (const room of ownedRooms) {
  const ledger = (counters.rooms[room.name] ||= { ticks: 0 });
  if (measured) ledger.ticks++;
  const structures = sorted(room.find(FIND_STRUCTURES));
  const byType = {};
  for (const s of structures) (byType[s.structureType] ||= []).push(s);
  const at = (type, x, y) => (byType[type] || []).find((s) => s.pos.x === x && s.pos.y === y);
  if (Game.time === 1) {
    if (Game.cpu.limit !== 100 || Game.cpu.tickLimit !== 500 || Game.cpu.bucket !== 10000)
      throw new Error('CPU budget mismatch');
    if (ownedRooms.length !== 6 || room.controller.level !== 8)
      throw new Error('Mature room mismatch');
    for (const [type, quota] of Object.entries(BENCH.functional_quotas))
      if ((byType[type] || []).length !== quota) throw new Error('Building quota mismatch ' + type);
  }
  const storage = byType.storage[0];
  const sources = sorted(room.find(FIND_SOURCES));
  const mineral = room.find(FIND_MINERALS)[0];
  const roster = Object.values(Game.creeps)
    .filter((c) => c.name.startsWith(room.name + '_'))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  if (!layouts[room.name]) {
    const blocked = new Set();
    const terrain = room.getTerrain();
    for (let y = 0; y < 50; y++)
      for (let x = 0; x < 50; x++)
        if (terrain.get(x, y) === TERRAIN_MASK_WALL) blocked.add(y * 50 + x);
    for (const s of structures)
      if (OBSTACLE_OBJECT_TYPES.includes(s.structureType)) blocked.add(s.pos.y * 50 + s.pos.x);
    for (const s of [...sources, mineral]) blocked.add(s.pos.y * 50 + s.pos.x);
    layouts[room.name] = blocked;
  }
  const blocked = new Set(layouts[room.name]);
  for (const c of roster) blocked.add(c.pos.y * 50 + c.pos.x);
  navigation[room.name] = { blocked, reserved: new Set() };

  const roleOf = (c) => c.name.slice(room.name.length + 1).split('_')[0];
  const missing = BENCH.roles.filter((role) => !roster.some((c) => roleOf(c) === role));
  for (const spawn of byType.spawn) {
    if (!spawn.spawning && missing.length) {
      const role = missing[0];
      const body = /^miner[01]$/.test(role)
        ? [...Array(5).fill(WORK), CARRY, ...Array(6).fill(MOVE)]
        : role.startsWith('haul') || role.startsWith('fill')
          ? [...Array(10).fill(CARRY), ...Array(10).fill(MOVE)]
          : [...Array(3).fill(WORK), ...Array(10).fill(CARRY), ...Array(13).fill(MOVE)];
      const result = record(
        ledger,
        'spawn',
        spawn.spawnCreep(body, room.name + '_' + role + '_' + Game.time),
      );
      if (result === OK) missing.shift();
    }
  }
  for (const creep of roster) {
    if (creep.spawning) continue;
    const role = roleOf(creep);
    Memory.creeps[creep.name] ||= { role, home: room.name };
    const energy = creep.store[RESOURCE_ENERGY];
    if (/^miner[01]$/.test(role)) {
      const index = +role.slice(-1);
      if (energy === creep.store.getCapacity())
        give(creep, at('container', 9, index ? 40 : 8), RESOURCE_ENERGY, ledger);
      else if (!travel(creep, sources[index], 1, ledger))
        record(ledger, 'harvest', creep.harvest(sources[index]));
    } else if (role.startsWith('haul')) {
      if (energy) give(creep, storage, RESOURCE_ENERGY, ledger);
      else take(creep, at('container', 9, role.endsWith('1') ? 40 : 8), RESOURCE_ENERGY, ledger);
    } else if (role.startsWith('fill')) {
      if (!energy) take(creep, storage, RESOURCE_ENERGY, ledger);
      else {
        const targets = [...byType.spawn, ...byType.extension, ...byType.tower];
        const target = targets.find(
          (s, i) => i % 3 === +role.slice(-1) && s.store.getFreeCapacity(RESOURCE_ENERGY) > 0,
        );
        if (target) give(creep, target, RESOURCE_ENERGY, ledger);
      }
    } else if (role === 'upgrade0') {
      if (!energy) take(creep, storage, RESOURCE_ENERGY, ledger);
      else if (!travel(creep, room.controller, 3, ledger))
        record(ledger, 'upgrade', creep.upgradeController(room.controller));
    } else if (role === 'repair0') {
      if (!energy) take(creep, storage, RESOURCE_ENERGY, ledger);
      else {
        const target = [...byType.road, ...byType.container, byType.rampart[0]].find(
          (s) => s.hits < (s.structureType === 'rampart' ? 1000000 : s.hitsMax),
        );
        if (target && !travel(creep, target, 3, ledger))
          record(ledger, 'repair', creep.repair(target));
      }
    } else if (role === 'mineral0') {
      if (creep.store.H && (creep.store.getFreeCapacity() === 0 || !mineral.mineralAmount))
        give(creep, storage, 'H', ledger);
      else if (energy) give(creep, storage, RESOURCE_ENERGY, ledger);
      else if (mineral.mineralAmount && !travel(creep, mineral, 1, ledger))
        record(ledger, 'mineral', creep.harvest(mineral));
    } else {
      const target =
        role === 'factory0'
          ? byType.factory[0]
          : role === 'power0'
            ? byType.powerSpawn[0]
            : role === 'link0'
              ? byType.link.find((s) => s.store.energy < 400)
              : byType.terminal[0];
      if (!energy) take(creep, storage, RESOURCE_ENERGY, ledger);
      else if (target && target.store.getFreeCapacity(RESOURCE_ENERGY) > 0)
        give(creep, target, RESOURCE_ENERGY, ledger);
    }
  }
  const inputs = [at('lab', 25, 30), at('lab', 26, 30)];
  if (Game.time % 50 === 0) {
    for (const lab of byType.lab.filter((s) => !inputs.includes(s)))
      record(ledger, 'reaction', lab.runReaction(...inputs));
    record(ledger, 'factory', byType.factory[0].produce('battery'));
    record(ledger, 'power', byType.powerSpawn[0].processPower());
  }
  if (Game.time % 100 === 25) {
    const destination = BENCH.destinations[room.name];
    record(ledger, 'terminal', byType.terminal[0].send(RESOURCE_ENERGY, 100, destination));
  }
  if (Game.time % 20 === 0) {
    for (let i = 0; i < byType.link.length; i += 2)
      if (byType.link[i].store.energy && byType.link[i + 1].store.getFreeCapacity(RESOURCE_ENERGY))
        record(ledger, 'link', byType.link[i].transferEnergy(byType.link[i + 1]));
    // 每座塔负责不同防线格，避免多个修复意图争抢同一已补满目标的实现差异。
    for (let i = 0; i < byType.tower.length; i++) {
      const target = byType.rampart[(i + 1) * 10];
      if (target.hits < 1000000) record(ledger, 'tower', byType.tower[i].repair(target));
    }
  }
  record(ledger, 'observe', byType.observer[0].observeRoom(room.name));
}
// 死亡清理使 Memory 有界；moveTo 的路径与角色记忆保留在玩家正常 Memory 中。
for (const name of Object.keys(Memory.creeps)) if (!Game.creeps[name]) delete Memory.creeps[name];
if (Game.time === BENCH.total_ticks) console.log('BENCH_DONE:' + JSON.stringify(counters));
