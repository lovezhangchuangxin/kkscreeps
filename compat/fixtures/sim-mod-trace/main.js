// 门卫场景驱动：第 1 轮 pass+hold，第 2 轮读回计数并镜像到 Memory，
// 第 3 轮再通行一次（检查点续跑场景的续跑轮产生新槽事件）。
// breaker 场景不装 gatekeeper 玩家面（无 FIND 常量），bot 静默空转——
// 故障由 link tick 前置处理器产生，与玩家行为无关。
exports.loop = function () {
  if (typeof FIND_GATEKEEPER_DOORS === 'undefined') return;
  const doors = Game.rooms.W1N1.find(FIND_GATEKEEPER_DOORS);
  if (doors.length !== 1) throw new Error('door query missing');
  if (Memory.phase === 1) {
    const codes = [doors[0].pass(), doors[0].hold()];
    Memory.codes = codes;
    Memory.phase = 2;
    return;
  }
  if (Memory.phase === 2) {
    Memory.doorValue = doors[0].value;
    Memory.phase = 3;
    return;
  }
  if (Memory.phase === 3) {
    doors[0].pass();
    Memory.phase = 4;
  }
};
