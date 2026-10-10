// 敌方脚本参与者（非真实 AI）：驻守 W2N1 的 reserver，贴近控制器持续
// 预留构成"敌方预留"威胁（TooAngel handleHostileReservedRoom 的触发
// 面）。受击不反击、不逃跑——被击杀即完成进攻面；不采掘、不扩张，
// 威胁只来自预留本身。
module.exports.loop = function () {
  for (const creep of Object.values(Game.creeps)) {
    const controller = creep.room.controller;
    if (!controller) continue;
    if (!creep.pos.isNearTo(controller.pos)) {
      creep.moveTo(controller.pos, { range: 1 });
      continue;
    }
    creep.reserveController(controller);
  }
};
