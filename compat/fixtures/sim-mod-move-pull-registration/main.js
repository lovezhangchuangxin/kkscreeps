// 第 0 tick 两组拉拽并行登记：p1 的 hauler(move) 拉同主人 cart(work)
// 让位上移；p2 的 thief(move) 拉跨主人 victim(p1,work) 让位右上。
// 被拉者都经 move(拉者) 形态请求拉者当前格（无 MOVE 部件者的唯一
// 通路）；敌方对象经房间 find 获取（Game.creeps 只含本方 creep）。
// 之后各 tick 只记录位置。
module.exports.loop = function () {
  const others = Game.rooms.W1N1 ? Game.rooms.W1N1.find(FIND_CREEPS) : [];
  const findCreep = (name) => Game.creeps[name] || others.find((c) => c.name === name);
  if (Game.time === 1) {
    if (Game.creeps.hauler) {
      Game.creeps.hauler.pull(Game.creeps.cart);
      Game.creeps.hauler.move(TOP);
      Game.creeps.cart.move(Game.creeps.hauler);
      const thief = findCreep('thief');
      if (thief) Game.creeps.victim.move(thief);
    }
    if (Game.creeps.thief) {
      const victim = findCreep('victim');
      if (victim) {
        Game.creeps.thief.pull(victim);
        Game.creeps.thief.move(TOP_RIGHT);
      }
    }
  }
  if (Game.time >= 2) {
    Memory.positions = {};
    for (const name of ['hauler', 'cart', 'thief', 'victim']) {
      const creep = findCreep(name);
      if (creep) Memory.positions[name] = [creep.pos.x, creep.pos.y];
    }
  }
};
