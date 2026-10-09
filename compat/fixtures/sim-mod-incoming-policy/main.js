// 前两轮攻击，第三轮观察受击公式、身体和原始攻击事件。
module.exports.loop = function () {
  const target = Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0];
  const actor = Game.creeps.attacker;
  if (Game.time < 3) Memory.code = actor.attack(target);
  Memory.target_hits = target.hits;
  Memory.tough_hits = target.body[0].hits;
  Memory.actor_hits = actor.hits;
  Memory.raw_damage = Game.rooms.W1N1.getEventLog().find(
    (event) => event.event === EVENT_ATTACK && event.objectId === actor.id,
  )?.data.damage;
};
