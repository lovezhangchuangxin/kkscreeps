// 验收入口只观察公开状态与上一拍事件，不代替 bot 发意图或修改其配置。
// main.js 及其依赖由验收工具从锁定的 TooAngel src/ 原样复制。
module.exports.loop = function () {
  const stats = (Memory.__kkAcceptance ||= {
    loops: 0,
    births: 0,
    birthsAfterDeath: 0,
    deaths: 0,
    harvested: 0,
    transferred: 0,
    built: 0,
    upgraded: 0,
    seen: {},
    caughtErrors: 0,
    errorSamples: [],
  });
  stats.loops++;
  stats.eventsThroughTick = Game.time - 1;

  // 场景只有一名玩家、无 NPC；死亡事件因此可以归到该玩家。
  // 事件来自真实结算，统计不把已接受但未成功执行的意图算作产出。
  for (const room of Object.values(Game.rooms)) {
    for (const event of room.getEventLog()) {
      const amount = event.data?.amount || 0;
      if (event.event === EVENT_HARVEST && amount > 0) stats.harvested += amount;
      if (event.event === EVENT_TRANSFER && amount > 0) stats.transferred += amount;
      if (event.event === EVENT_BUILD && amount > 0) stats.built += amount;
      if (event.event === EVENT_UPGRADE_CONTROLLER && amount > 0) stats.upgraded += amount;
      if (event.event === EVENT_OBJECT_DESTROYED && event.data?.type === 'creep') stats.deaths++;
    }
  }
  for (const creep of Object.values(Game.creeps)) {
    if (creep.spawning || stats.seen[creep.id]) continue;
    stats.seen[creep.id] = true;
    stats.births++;
    if (stats.deaths > 0) stats.birthsAfterDeath++;
  }

  // 锁定版本的 main/brain/room/creep 会捕获异常并 console.log，宿主成功
  // 不足以证明 bot 成功。完整运行窗口拦截这些消息（不依赖前 20 条日志）。
  const log = console.log;
  console.log = function (...args) {
    const text = args.map(String).join(' ');
    if (/ERROR:|Exception|Executing (?:room|creep role) failed:/.test(text)) {
      stats.caughtErrors++;
      if (stats.errorSamples.length < 5) stats.errorSamples.push(text.slice(0, 1000));
    }
    return log.apply(console, args);
  };
  try {
    require('main').loop();
  } finally {
    console.log = log;
  }
  if (stats.caughtErrors) throw new Error('TooAngel caught exception: ' + stats.errorSamples[0]);
};
