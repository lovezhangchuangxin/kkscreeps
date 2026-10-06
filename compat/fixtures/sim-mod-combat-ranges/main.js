// atk(p1,[attack]) 距离 2 打 foe(p2)；medic(p1,[heal]) 距离 2/4 治
// ally/pal（射程门返回码即玩家面证据；ally/pal 满血，治疗量不可观，
// 结算侧命中由 foeHits 证）。玩家代码先于结算：time>=3 记录时已结算
// 3 轮，配合 --ticks 4 使断言反映 3 轮效果。射程常量一并入 Memory，
// 证明规则生效值已注入玩家全局。
module.exports.loop = function () {
  const foe = Game.rooms['W1N1'].find(FIND_CREEPS).filter(c => !c.my)[0];
  Memory.ranges = [ATTACK_RANGE, HEAL_RANGE, RANGED_HEAL_RANGE].join(',');
  Memory.codes = {
    attack: Game.creeps.atk.attack(foe),
    heal: Game.creeps.medic.heal(Game.creeps.ally),
    rangedHeal: Game.creeps.medic.rangedHeal(Game.creeps.pal),
  };
  if (Game.time >= 3) {
    Memory.record = { foeHits: foe ? foe.hits : null };
  }
};
