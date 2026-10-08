// 四轮探针（每 creep 每 tick 只有一条 attack/rangedAttack 意图——预检
// 拒绝不覆盖已记录意图）：raider 以近战经营 siege barrel（30/击），
// sniper 以远程经营 vault cache（10/击），mule 向 cache 转入能量。
// T0 发动作并读初值（读值是提交前视图）；T1 读提交并续击/转入；T2 再击
// 并核对双信封互不扰动；T3 读终值。
exports.loop = function () {
  const room = Game.rooms.W1N1;
  const raider = Game.creeps.raider;
  const barrel = typeof FIND_SIEGE_BARRELS !== 'undefined'
    ? room.find(FIND_SIEGE_BARRELS)[0] : undefined;
  const decoy = typeof FIND_SIEGE_DECOYS !== 'undefined'
    ? room.find(FIND_SIEGE_DECOYS)[0] : undefined;
  const sniper = Game.creeps.sniper;
  const mule = Game.creeps.mule;
  const cache = typeof FIND_VAULT_CACHES !== 'undefined'
    ? room.find(FIND_VAULT_CACHES)[0] : undefined;
  if (!Memory.phase) {
    if (barrel === undefined) {
      Memory.initialHits = 0;
      Memory.initialHitsMax = 0;
      Memory.attackCode = -7;
      Memory.decoyCode = -7;
    } else {
      Memory.initialHits = barrel.hits;
      Memory.initialHitsMax = barrel.hitsMax;
      Memory.attackCode = raider.attack(barrel);
      Memory.decoyCode = decoy === undefined ? -7 : raider.attack(decoy);
    }
    if (cache !== undefined && sniper !== undefined) {
      Memory.cacheInitialHits = cache.hits;
      Memory.cacheCapacity = cache.storeCapacity;
      Memory.snipeCode = sniper.rangedAttack(cache);
    }
    Memory.phase = 1;
    return;
  }
  if (Memory.phase === 1) {
    if (barrel !== undefined) {
      Memory.committedHits = barrel.hits;
      raider.attack(barrel);
    }
    if (cache !== undefined && sniper !== undefined) {
      Memory.cacheCommittedHits = cache.hits;
      Memory.transferCode = mule === undefined ? -7 : mule.transfer(cache, RESOURCE_ENERGY, 50);
      sniper.rangedAttack(cache);
    }
    Memory.phase = 2;
    return;
  }
  if (Memory.phase === 2) {
    if (barrel !== undefined) {
      Memory.secondHits = barrel.hits;
      raider.attack(barrel);
    }
    if (cache !== undefined) {
      Memory.cacheAfterTransfer = cache.hits;
      Memory.cacheEnergy = cache.store.energy;
      if (sniper !== undefined) sniper.rangedAttack(cache);
    }
    Memory.phase = 3;
    return;
  }
  if (Memory.phase === 3) {
    if (barrel !== undefined) {
      Memory.finalHits = barrel.hits;
    }
    if (cache !== undefined) {
      Memory.cacheFinalHits = cache.hits;
      Memory.cacheFinalEnergy = cache.store.energy;
    }
    Memory.phase = 4;
  }
};
