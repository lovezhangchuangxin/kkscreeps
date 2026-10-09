// TooAngel 防御战斗验收观察器：只观察公开状态与上一拍事件，不代替
// bot 发意图或修改其配置（与 sim-tooangel/acceptance.js 同纪律）。
// 战斗指标：对敌攻击事件（塔按攻击者 objectId 单独归因）、敌方承受
// 伤害、防御者出生、安全模式激活；另保留经济计数证明战斗期间经济
// 未被拖垮。
module.exports.loop = function () {
  const stats = (Memory.__kkAcceptance ||= {
    loops: 0,
    births: 0,
    deaths: 0,
    harvested: 0,
    built: 0,
    upgraded: 0,
    seen: {},
    combatSeen: {},
    attackEvents: 0,
    towerAttackEvents: 0,
    towerDamage: 0,
    raiderDamage: 0,
    raiderHealed: 0,
    combatBirths: 0,
    towerFirstTick: null,
    defenderFirstTick: null,
    safeModeTick: null,
    safeModeAvailableTick0: null,
    caughtErrors: 0,
    errorSamples: [],
  });
  stats.loops++;
  stats.eventsThroughTick = Game.time - 1;

  const room = Game.rooms.W1N1;
  if (room && stats.safeModeAvailableTick0 === null) {
    stats.safeModeAvailableTick0 = room.controller ? room.controller.safeModeAvailable : null;
  }
  // 玩家面 safeMode 是剩余 tick 数（激活时远大于 0），不与 Game.time 比。
  if (
    room &&
    stats.safeModeTick === null &&
    room.controller &&
    (room.controller.safeMode ?? 0) > 0
  ) {
    stats.safeModeTick = Game.time;
  }

  if (room) {
    const hostileIds = new Set(room.find(FIND_HOSTILE_CREEPS).map((creep) => creep.id));
    const towerIds = new Set(
      room
        .find(FIND_MY_STRUCTURES)
        .filter((structure) => structure.structureType === STRUCTURE_TOWER)
        .map((structure) => structure.id),
    );
    for (const event of room.getEventLog()) {
      const amount = event.data?.amount || 0;
      if (event.event === EVENT_HARVEST && amount > 0) stats.harvested += amount;
      if (event.event === EVENT_BUILD && amount > 0) stats.built += amount;
      if (event.event === EVENT_UPGRADE_CONTROLLER && amount > 0) stats.upgraded += amount;
      if (event.event === EVENT_OBJECT_DESTROYED && event.data?.type === 'creep') stats.deaths++;
      if (event.event === EVENT_ATTACK) {
        stats.attackEvents++;
        if (hostileIds.has(event.data?.targetId)) {
          // EVENT_ATTACK 的伤害字段是 damage（amount 是采集/治疗族）。
          stats.raiderDamage += event.data?.damage || 0;
          if (towerIds.has(event.objectId)) {
            stats.towerAttackEvents++;
            stats.towerDamage += event.data?.damage || 0;
            if (stats.towerFirstTick === null) stats.towerFirstTick = Game.time;
          }
        }
      }
      if (event.event === EVENT_HEAL && hostileIds.has(event.objectId)) {
        stats.raiderHealed += amount;
      }
    }
  }

  for (const creep of Object.values(Game.creeps)) {
    if (creep.spawning || stats.seen[creep.id]) continue;
    stats.seen[creep.id] = true;
    stats.births++;
    const combat = creep.body.some((part) => part.type === ATTACK || part.type === RANGED_ATTACK);
    if (combat && !stats.combatSeen[creep.id]) {
      stats.combatSeen[creep.id] = true;
      stats.combatBirths++;
      if (stats.defenderFirstTick === null) stats.defenderFirstTick = Game.time;
    }
  }

  // 与经济验收同款：拦截锁定 bot 自捕获的异常，宿主成功不掩盖 bot 失败。
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
