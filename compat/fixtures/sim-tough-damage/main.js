// 连续两轮攻击，第三轮从真实玩家 API 观察结算后的身体和事件。
module.exports.loop = function () {
  const target = Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0];
  const actor = Game.creeps.attacker;
  if (Game.time < 2) Memory.code = actor.attack(target);
  Memory.target_hits = target.hits;
  Memory.target_tough_hits = target.body[0].hits;
  Memory.actor_hits = actor.hits;
  Memory.actor_tough_hits = actor.body[0].hits;
  Memory.raw_damage = Game.rooms.W1N1.getEventLog()
    .find(event => event.event === EVENT_ATTACK && event.objectId === actor.id)?.data.damage;
};
