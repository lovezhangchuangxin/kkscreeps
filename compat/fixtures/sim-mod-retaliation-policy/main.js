// 同一玩家脚本用于两个策略包与原生对照；只在 tick 0 攻击一次。
module.exports.loop = function () {
  const target = Game.rooms.W1N1.find(FIND_HOSTILE_CREEPS)[0];
  if (Game.time === 1) Memory.code = Game.creeps.attacker.attack(target);
  Memory.target_hits = target?.hits ?? null;
  Memory.actor_hits = Game.creeps.attacker.hits;
};
