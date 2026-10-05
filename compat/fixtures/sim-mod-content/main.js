exports.loop = function () {
  const nodes = Game.rooms[Memory.room].find(FIND_ALPHA_NODES);
  if (nodes.length !== 1 || !(nodes[0] instanceof AlphaNode)) throw new Error('alpha API missing');
  // 三个场景都安装 alpha；beta 只在选中它的场景出现，typeof 对未安装的
  // 全局与常量都安全（不产生 ReferenceError）。
  Memory.value = nodes[0].value;
  Memory.hasBeta = typeof BetaBeacon;
  Memory.betaFind = typeof FIND_BETA_BEACONS;
};
