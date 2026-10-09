// 护卫优先自疗并靠近敌方；近战反击使 Invader 受伤，观察实际恢复与移动。
exports.loop = () => {
  Memory.stats ||= {
    melee: 0,
    ranged: 0,
    healed: 0,
    npcMelee: 0,
    npcRanged: 0,
    npcHealed: 0,
    npcMoved: 0,
    deaths: 0,
    npcRecovered: 0,
    guardInjured: 0,
    previousGuardHits: null,
    lastHits: {},
    last: {},
  };
  const s = Memory.stats;
  const room = Game.rooms.W1N1;
  for (const event of room.getEventLog()) {
    if (event.event === EVENT_ATTACK) {
      if (event.data.attackType === EVENT_ATTACK_TYPE_MELEE) s.melee += event.data.damage;
      if (event.data.attackType === EVENT_ATTACK_TYPE_RANGED) s.ranged += event.data.damage;
      if (event.objectId !== Memory.guardId) {
        if (event.data.attackType === EVENT_ATTACK_TYPE_MELEE) s.npcMelee += event.data.damage;
        if (event.data.attackType === EVENT_ATTACK_TYPE_RANGED) s.npcRanged += event.data.damage;
      }
    }
    if (event.event === EVENT_HEAL) {
      s.healed += event.data.amount;
      if (event.objectId !== Memory.guardId) s.npcHealed += event.data.amount;
    }
    if (event.event === EVENT_OBJECT_DESTROYED && event.data.type === 'creep') s.deaths++;
  }
  for (const npc of room.find(FIND_HOSTILE_CREEPS)) {
    const current = `${npc.pos.x},${npc.pos.y}`;
    if (s.last[npc.id] && s.last[npc.id] !== current) s.npcMoved++;
    s.last[npc.id] = current;
    if (s.lastHits[npc.id] !== undefined)
      s.npcRecovered += Math.max(0, npc.hits - s.lastHits[npc.id]);
    s.lastHits[npc.id] = npc.hits;
  }
  const guard = Game.creeps.guard;
  if (guard) {
    Memory.guardId = guard.id;
    if (s.previousGuardHits !== null)
      s.guardInjured += Math.max(0, s.previousGuardHits - guard.hits);
    s.previousGuardHits = guard.hits;
    guard.heal(guard);
    const targets = room.find(FIND_HOSTILE_CREEPS).sort((a, b) => a.hits - b.hits);
    const target = targets[0];
    if (target) {
      guard.moveTo(target, { range: 1 });
    }
  }
  s.eventsThroughTick = Game.time - 1;
  Memory.loops = (Memory.loops || 0) + 1;
};
