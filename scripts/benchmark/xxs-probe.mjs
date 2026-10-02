// Runs production isolated-vm runner and processor workers; no test simulator or HTTP backend.
import fs from 'node:fs/promises';
import { config } from 'xxscreeps/config/index.js';
import { performance } from 'node:perf_hooks';
import { Database, Shard } from 'xxscreeps/engine/db/index.js';
import * as User from 'xxscreeps/engine/db/user/index.js';
import * as Code from 'xxscreeps/engine/db/user/code.js';
import { importPayload, seedShard } from 'xxscreeps/scripts/payload.js';
import { initializeGameEnvironment, runOneShot } from 'xxscreeps/game/index.js';
import { createRoomObject } from 'xxscreeps/game/object.js';
import { flushUsers } from 'xxscreeps/game/room/room.js';
import { RoomPosition } from 'xxscreeps/game/position.js';
import { StructureController } from 'xxscreeps/mods/classic/controller/controller.js';
import { Source } from 'xxscreeps/mods/classic/source/source.js';
import { create as createSpawn } from 'xxscreeps/mods/classic/spawn/spawn.js';
import { create as createCreep } from 'xxscreeps/mods/classic/creep/creep.js';
import * as C from 'xxscreeps:mods/constants';
import { getServiceChannel } from 'xxscreeps/engine/service/index.js';
import { getRunnerChannel, runnerUsersSetKey, getConsoleChannel } from 'xxscreeps/engine/runner/model.js';
import { activeRoomsKey, begetRoomProcessQueue, getProcessorChannel } from 'xxscreeps/engine/processor/model.js';
import { runShardInitializers, runShardTickProcessors } from 'xxscreeps/engine/processor/shard.js';
import { Worker, waitForWorker } from 'xxscreeps/utility/worker.js';
import 'xxscreeps:mods/game';
import 'xxscreeps:mods/main';
initializeGameEnvironment();
const args = JSON.parse(await fs.readFile('probe-args.json', 'utf8'));
const fixture = JSON.parse(await fs.readFile(args.fixture, 'utf8'));
await using db = await Database.connect();
await using shard = await Shard.connect(db, 'shard0');
const payload = Object.fromEntries(fixture.shard.rooms.map(r => [r.name, {
  layout: r.terrain.reduce((rows, v, i) => {
    if (i % 50 === 0) rows.push('');
    rows[rows.length - 1] += v === 1 ? '#' : v === 2 ? ',' : ' ';
    return rows;
  }, [])
}]));
await seedShard(shard, importPayload(payload));
for (const [id, name] of Object.entries(User.npcUsers)) await User.create(db, id, name);
for (const id of fixture.shard.players) {
  await User.create(db, id, 'player' + id);
  await Code.saveContent(db, id, 'main', new Map([['main', `module.exports.loop = function () {\n${fixture.code}\n};`]]));
}
const world = await shard.loadWorld();
for (const spec of fixture.shard.rooms) {
  const room = await shard.loadRoom(spec.name, 0);
  runOneShot(world, room, 0, '', () => {
    const controller = createRoomObject(new StructureController(), new RoomPosition(1, 1, room.name));
    controller['#user'] = room['#user'] = spec.owner;
    room['#level'] = spec.rcl;
    controller['#progress'] = 0;
    controller['#downgradeTime'] = C.CONTROLLER_DOWNGRADE[spec.rcl] + 1;
    room['#insertObject'](controller);
    for (const {
      kind,
      spec: o
    } of fixture.objects.filter(o => o.room === room.name)) {
      const pos = new RoomPosition(...o.pos, room.name);
      let object;
      if (kind === 'creep') {
        object = createCreep(pos, o.body, o.name, o.owner);
        object.store['#add']('energy', o.store.energy);
      } else if (o.structureType === 'spawn') object = createSpawn(pos, o.owner, o.name);else if (o.structureType === 'source') {
        object = createRoomObject(new Source(), pos);
        object.energy = o.energy;
        object.energyCapacity = o.energyCapacity;
        object['#nextRegenerationTime'] = 0;
      } else throw new Error('Unsupported fixture object');
      room['#insertObject'](object);
    }
  });
  room['#flushObjects'](null);
  flushUsers(room);
  await shard.saveRoom(room.name, 0, room);
  await shard.copyRoomFromPreviousTick(room.name, 1);
}
async function observe() {
  return Promise.all(fixture.shard.rooms.map(async ({
    name
  }) => {
    const room = await shard.loadRoom(name, shard.time);
    return runOneShot(world, room, shard.time, '', () => ({
      room: name,
      progress: room.controller.progress,
      level: room.controller.level,
      sources: room['#objects'].filter(o => o instanceof Source).map(o => ({
        x: o.pos.x,
        y: o.pos.y,
        energy: o.energy,
        capacity: o.energyCapacity
      })).sort((a, b) => a.y - b.y || a.x - b.x),
      creeps: room['#objects'].filter(o => o.constructor.name === 'Creep').map(o => ({
        name: o.name,
        x: o.pos.x,
        y: o.pos.y,
        energy: o.store.energy,
        hits: o.hits,
        body: o.body.map(p => p.type),
        spawning: o.spawning
      })).sort((a, b) => a.name.localeCompare(b.name)),
      spawns: room['#objects'].filter(o => o.constructor.name === 'StructureSpawn').map(o => ({
        name: o.name,
        x: o.pos.x,
        y: o.pos.y,
        hits: o.hits,
        energy: o.store.energy,
        spawning: !!o.spawning
      })).sort((a, b) => a.name.localeCompare(b.name))
    }));
  }));
}
const initial = await observe();
const channel = await getServiceChannel(shard).subscribe();
let failures = [];
let stopConsole = [];
const completion = {};
const workers = [];
const workerDone = [];
let shuttingDown = false;
const workerFailure = Promise.withResolvers();
workerFailure.promise.catch(() => {});
function startWorker(module) {
  const worker = Worker.create(module);
  workers.push(worker);
  worker.on('error', error => workerFailure.reject(error));
  const done = waitForWorker(worker);
  workerDone.push(done);
  done.then(() => {
    if (!shuttingDown) workerFailure.reject(new Error('Service exited before shutdown'));
  }, error => workerFailure.reject(error));
  return worker;
}
for (const id of fixture.shard.players) stopConsole.push(await getConsoleChannel(shard, id).listen(message => {
  for (const frame of JSON.parse(message)) {
    if (frame.fd === 2) failures.push(frame.data);else if (frame.data.includes("BENCH_DONE:")) (completion[id] ||= []).push(frame.data.trim());
  }
}));
function waitMessage(test, label) {
  const deferred = Promise.withResolvers();
  const cancel = channel.listen(message => {
    if (test(message)) deferred.resolve(message);
  });
  let timer;
  const result = Promise.race([deferred.promise, workerFailure.promise, new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error('Service timeout: ' + label)), 30000);
  })]).finally(() => {
    clearTimeout(timer);
    cancel();
  });
  // An earlier setup failure may bypass the later await; retain the rejection for its caller.
  result.catch(() => {});
  return result;
}

try {
  await shard.scratch.sAdd('initializeRooms', fixture.shard.rooms.map(r => r.name));
  const connected = waitMessage(m => m.type === 'processorConnected', 'processor connection');
  startWorker('xxscreeps/engine/service/processor.js');
  const initialized = waitMessage(m => m.type === 'processorInitialized', 'room initialization');
  await connected;
  await channel.publish({
    type: 'mainConnected'
  });
  await initialized;
  if ((await shard.scratch.zCard(activeRoomsKey)) !== fixture.shard.rooms.length) throw new Error('Room initialization incomplete');
  await begetRoomProcessQueue(shard, shard.time);
  await runShardInitializers(shard);
  const runnerConnected = waitMessage(m => m.type === 'runnerConnected', 'runner connection');
  startWorker('xxscreeps/engine/service/runner.js');
  await runnerConnected;
  async function tick() {
    const time = shard.time;
    const finished = waitMessage(m => m.type === 'tickFinished' && m.time === time, 'tick ' + time);
    await Promise.all([
      shard.scratch.copy('activeUsers', runnerUsersSetKey(time)),
      getProcessorChannel(shard).publish({type: 'process', time})
    ]);
    await getRunnerChannel(shard).publish({
      type: 'run',
      time
    });
    await finished;
    await runShardTickProcessors(shard, time);
    const nextTime = time + 1;
    await Promise.all([
      begetRoomProcessQueue(shard, nextTime),
      (async () => {
        await shard.data.set('time', nextTime);
        await shard.channel.publish({type: 'tick', time: nextTime});
        shard.time = nextTime;
      })()
    ]);
    if (failures.length) throw new Error('Sandbox failure: ' + failures.join('\n'));
  }
  for (let i = 0; i < args.warmup; i++) await tick();
  const before = await observe();
  const cpu = process.cpuUsage();
  const start = performance.now();
  let wall = [],
    rss = [process.memoryUsage.rss()];
  for (let i = 0; i < args.ticks; i++) {
    const t = performance.now();
    await tick();
    wall.push(performance.now() - t);
    rss.push(process.memoryUsage.rss());
  }
  const used = process.cpuUsage(cpu);
  const processWall = performance.now() - start;
  const after = await observe();
  for (let i = 0; i < after.length; i++) if (after[i].progress <= before[i].progress) throw new Error('No upgrade in ' + after[i].room);
  const quantile = (values, q) => [...values].sort((a, b) => a - b)[Math.round((values.length - 1) * q)];
  await fs.writeFile(args.report, JSON.stringify({
    engine: 'xxscreeps',
    players: fixture.shard.players.length,
    warmup: args.warmup,
    ticks: args.ticks,
    concurrency: config.runner.concurrency,
    cpu_budget: {limit: 100, bucket: config.runner.cpu.bucket, tick_limit: config.runner.cpu.tickLimit},
    metadata: {
      node: process.version,
      arch: process.arch,
      platform: process.platform,
      sandbox: config.runner.sandbox,
      processor_workers: Math.max(1, Math.min(config.processor.concurrency, Math.ceil(fixture.shard.players.length / 2))),
      v8: process.versions.v8,
      services: 'production runner/processor worker_threads; custom coordinator'
    },
    process: {
      scope: 'current_process_all_threads',
      wall_ms: processWall,
      cpu_core_ms: (used.user + used.system) / 1000,
      cpu_core_ms_per_tick: (used.user + used.system) / 1000 / args.ticks,
      rss_bytes_sampled_p50: quantile(rss, .5),
      rss_bytes_sampled_peak: Math.max(...rss)
    },
    phases: {
      phases: {
        wall: {
          count: wall.length,
          p50_ms: quantile(wall, .5),
          p95_ms: quantile(wall, .95),
          p99_ms: quantile(wall, .99),
          max_ms: Math.max(...wall)
        }
      }
    },
    initial,
    before,
    after,
    completion
  }, null, 2));
} finally {
  shuttingDown = true;
  await Promise.all([getProcessorChannel(shard).publish({
    type: 'shutdown'
  }), getRunnerChannel(shard).publish({
    type: 'shutdown'
  })]);
  let timer;
  try {
    await Promise.race([Promise.all(workerDone), new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error('Worker shutdown timeout')), 5000);
    })]);
  } finally {
    clearTimeout(timer);
    await Promise.all(workers.map(worker => worker.terminate()));
    for (const stop of stopConsole) stop();
    channel.disconnect();
  }
}
