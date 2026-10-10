// TooAngel 多房进攻验收观察器：只观察公开状态与上一拍事件，不代替
// bot 发意图或修改其配置（与 sim-tooangel/acceptance.js 同纪律）。
// 进攻指标：W2N1 可见性与敌方预留观察、attackunreserve 出生、对
// reserver 的攻击事件与击杀（按目标 id 归因）；另保留经济计数证明
// 进攻期间经济未被拖垮。W2N1 仅在 bot 有眼线（scout/进攻 creep 在
// 场）时可见，事件读取同样只在可见拍进行。
module.exports.loop = function () {
  const stats = (Memory.__kkAcceptance ||= {
    loops: 0,
    births: 0,
    deaths: 0,
    harvested: 0,
    built: 0,
    upgraded: 0,
    seen: {},
    offenseBirths: 0,
    offenseFirstBirthTick: null,
    w2FirstVisibleTick: null,
    w2VisibleTicks: 0,
    w2HostileReservationTick: null,
    reserverId: null,
    reserverLastSeenTick: null,
    attackEventsOnReserver: 0,
    reserverDamage: 0,
    reserverDeadTick: null,
    reserverDestroyedTick: null,
    scoutBirths: 0,
    scoutFirstBirthTick: null,
    cpuSkips: 0,
    caughtErrors: 0,
    errorSamples: [],
  });
  stats.loops++;
  stats.eventsThroughTick = Game.time - 1;

  const home = Game.rooms.W1N1;
  if (home) {
    for (const event of home.getEventLog()) {
      const amount = event.data?.amount || 0;
      if (event.event === EVENT_HARVEST && amount > 0) stats.harvested += amount;
      if (event.event === EVENT_BUILD && amount > 0) stats.built += amount;
      if (event.event === EVENT_UPGRADE_CONTROLLER && amount > 0) stats.upgraded += amount;
      if (event.event === EVENT_OBJECT_DESTROYED && event.data?.type === 'creep') stats.deaths++;
    }
  }

  const w2 = Game.rooms.W2N1;
  if (w2) {
    stats.w2VisibleTicks++;
    if (stats.w2FirstVisibleTick === null) stats.w2FirstVisibleTick = Game.time;
    const reservation = w2.controller && w2.controller.reservation;
    if (reservation && reservation.username !== 'bot' && stats.w2HostileReservationTick === null) {
      stats.w2HostileReservationTick = Game.time;
    }
    const hostileCreeps = w2.find(FIND_HOSTILE_CREEPS);
    if (stats.reserverId === null) {
      const claimer = hostileCreeps.find((creep) => creep.body.some((part) => part.type === CLAIM));
      if (claimer) stats.reserverId = claimer.id;
    }
    if (stats.reserverId !== null) {
      if (hostileCreeps.some((creep) => creep.id === stats.reserverId)) {
        stats.reserverLastSeenTick = Game.time;
      } else if (stats.reserverDeadTick === null) {
        // 可见拍缺失是死亡的下界信号；击杀由销毁事件精确断言。
        stats.reserverDeadTick = Game.time;
      }
    }
    for (const event of w2.getEventLog()) {
      if (
        event.event === EVENT_ATTACK &&
        event.data?.targetId === stats.reserverId &&
        // 攻击者归属核验：只计 bot 方的攻击（与战斗批塔归因同纪律）。
        Game.getObjectById(event.objectId)?.my === true
      ) {
        // EVENT_ATTACK 的伤害字段是 damage（amount 是采集/治疗族）。
        stats.attackEventsOnReserver++;
        stats.reserverDamage += event.data?.damage || 0;
      }
      if (event.event === EVENT_OBJECT_DESTROYED && event.data?.type === 'creep') {
        stats.deaths++;
        if (event.objectId === stats.reserverId) stats.reserverDestroyedTick = Game.time;
      }
    }
  }

  for (const creep of Object.values(Game.creeps)) {
    if (creep.spawning || stats.seen[creep.id]) continue;
    stats.seen[creep.id] = true;
    stats.births++;
    if (creep.name.startsWith('attackunreserve')) {
      stats.offenseBirths++;
      if (stats.offenseFirstBirthTick === null) stats.offenseFirstBirthTick = Game.time;
    }
    if (creep.name.startsWith('scout')) {
      stats.scoutBirths++;
      if (stats.scoutFirstBirthTick === null) stats.scoutFirstBirthTick = Game.time;
    }
  }

  // 与经济验收同款：拦截锁定 bot 自捕获的异常与 CPU 桶跳拍，宿主成功
  // 不掩盖 bot 失败。
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
