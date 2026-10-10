// 长期存活观察器：只观察公开状态与上一拍事件，不代替 bot 发意图或改配置。
// main.js 及其依赖由验收工具从锁定的 TooAngel src/ 原样复制。
// 与经济夹具 acceptance.js 的关键差异：所有持久化统计必须有界——出生
// 改为孵化名字表（出世即移除），采样数组封顶，不用 seen 全量映射，避免
// 观察器自身把 Memory 推向官方 2MB 上限；Memory 字节数排除 __kkAcceptance
// 自身后按采样拍记录，用于长期增长曲线与上限裕度断言。
module.exports.loop = function () {
  const stats = (Memory.__kkAcceptance ||= {
    loops: 0,
    births: 0,
    deaths: 0,
    harvested: 0,
    transferred: 0,
    built: 0,
    upgraded: 0,
    hatching: {},
    caughtErrors: 0,
    errorSamples: [],
    cpuSkips: 0,
    maxControllerLevel: 0,
    levelUpTicks: [],
    structureFirstTicks: {},
    roleBirths: {},
    worldSamples: [],
  });
  stats.loops++;
  stats.eventsThroughTick = Game.time - 1;

  // 场景只有一名玩家、无 NPC；死亡事件因此可以归到该玩家。
  for (const room of Object.values(Game.rooms)) {
    for (const event of room.getEventLog()) {
      const amount = event.data?.amount || 0;
      if (event.event === EVENT_HARVEST && amount > 0) stats.harvested += amount;
      if (event.event === EVENT_TRANSFER && amount > 0) stats.transferred += amount;
      if (event.event === EVENT_BUILD && amount > 0) stats.built += amount;
      if (event.event === EVENT_UPGRADE_CONTROLLER && amount > 0) stats.upgraded += amount;
      if (event.event === EVENT_OBJECT_DESTROYED && event.data?.type === 'creep') stats.deaths++;
    }
    const level = room.controller?.level || 0;
    if (level > stats.maxControllerLevel) {
      stats.maxControllerLevel = level;
      stats.levelUpTicks.push(Game.time);
    }
  }
  for (const creep of Object.values(Game.creeps)) {
    if (creep.spawning) {
      stats.hatching[creep.name] = true;
    } else if (creep.name in stats.hatching) {
      delete stats.hatching[creep.name];
      stats.births++;
      // TooAngel 名字形如 角色-随机数；按角色前缀归类，键空间有界。
      const role = creep.name.split('-')[0];
      stats.roleBirths[role] = (stats.roleBirths[role] || 0) + 1;
    }
  }

  // 采样拍：每 500 拍记录 Memory 字节数（剔除观察器自身）、storage 能量、
  // creep 数与 RCL；每 25 拍扫描关键建筑的首现拍。数组封顶防无界增长。
  if (Game.time % 500 === 0 && stats.worldSamples.length < 400) {
    const botOnly = { ...Memory, __kkAcceptance: undefined };
    const room = Game.rooms.W1N1;
    const storage = room?.storage;
    stats.worldSamples.push([
      Game.time,
      JSON.stringify(botOnly).length,
      storage ? storage.store.getUsedCapacity(RESOURCE_ENERGY) : -1,
      Object.keys(Game.creeps).length,
      room?.controller?.level || 0,
    ]);
  }
  if (Game.time % 25 === 0) {
    const seen = {};
    for (const structure of Object.values(Game.structures)) seen[structure.structureType] = true;
    for (const type of Object.keys(seen)) {
      if (stats.structureFirstTicks[type] === undefined)
        stats.structureFirstTicks[type] = Game.time;
    }
  }

  // 锁定版本的 main/brain/room/creep 会捕获异常并 console.log，宿主成功
  // 不足以证明 bot 成功；CPU 桶耗尽跳帧同样只走 console。完整窗口拦截。
  const log = console.log;
  console.log = function (...args) {
    const text = args.map(String).join(' ');
    if (/ERROR:|Exception|Executing (?:room|creep role) failed:/.test(text)) {
      stats.caughtErrors++;
      if (stats.errorSamples.length < 5) stats.errorSamples.push(text.slice(0, 1000));
    }
    if (/Skipping tick CPU Bucket/.test(text)) stats.cpuSkips++;
    return log.apply(console, args);
  };
  try {
    require('main').loop();
  } finally {
    console.log = log;
  }
  if (stats.caughtErrors) throw new Error('TooAngel caught exception: ' + stats.errorSamples[0]);
};
