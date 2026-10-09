// 敌方脚本参与者（非真实 AI）：驻守远角的坦克，受击自疗。
// 站位与塔保持 ≥20 格使塔伤害衰减到 25%（150/tick），10×heal 自疗
// 120/tick——净 -30/tick（塔持续供能假设下约 133 拍击穿）。坦克只需
// 存活并在场到 398 拍：安全模式实际经 TooAngel handleReviveRoom 的
// 400 拍相位门触发（checkForSafeMode 的 attackTimer>100 路径是死代码
// ——enemies>0 数组比较恒 false），hostiles 判定把含 heal 部件的坦克
// 算作攻击者。实际时序是塔在 900 能量内打 90 发后断能，坦克自疗回满
// 并全窗存活，398 拍安全模式激活后自疗被禁止、只剩承伤。不追击、
// 不进攻，威胁只来自在场。
module.exports.loop = function () {
  for (const creep of Object.values(Game.creeps)) {
    if (creep.spawning) continue;
    if (creep.hits < creep.hitsMax) {
      creep.heal(creep);
    }
  }
};
