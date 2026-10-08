// 四轮探针：raider 近战经营 thorn spike（30/击、反击 15），sniper 远程
// 经营 reactor core（10/击、反击 10 全额），mule 向 core 转入能量。
// 反击的可观测面=攻击者同轮 hits 扣减（延迟队列）。
exports.loop = function () {
  const room = Game.rooms.W1N1;
  const raider = Game.creeps.raider;
  const spike = typeof FIND_THORN_SPIKES !== 'undefined'
    ? room.find(FIND_THORN_SPIKES)[0] : undefined;
  const sniper = Game.creeps.sniper;
  const mule = Game.creeps.mule;
  const core = typeof FIND_REACTOR_CORES !== 'undefined'
    ? room.find(FIND_REACTOR_CORES)[0] : undefined;
  if (!Memory.phase) {
    if (spike === undefined) {
      Memory.attackCode = -7;
    } else {
      Memory.attackCode = raider.attack(spike);
    }
    if (core !== undefined && sniper !== undefined) {
      Memory.snipeCode = sniper.rangedAttack(core);
      Memory.coreInitialHits = core.hits;
      Memory.transferCode = mule === undefined ? -7 : mule.transfer(core, RESOURCE_ENERGY, 50);
    }
    Memory.phase = 1;
    return;
  }
  if (Memory.phase === 1) {
    if (spike !== undefined) {
      Memory.spikeHits = spike.hits;
      Memory.raiderHits = raider.hits;
      raider.attack(spike);
    }
    if (core !== undefined && sniper !== undefined) {
      Memory.coreHits = core.hits;
      Memory.sniperHits = sniper.hits;
      Memory.coreEnergy = core.store.energy;
      sniper.rangedAttack(core);
    }
    Memory.phase = 2;
    return;
  }
  if (Memory.phase === 2) {
    if (spike !== undefined) {
      Memory.spikeHits2 = spike.hits;
      Memory.raiderHits2 = raider.hits;
      raider.attack(spike);
    }
    if (core !== undefined && sniper !== undefined) {
      sniper.rangedAttack(core);
    }
    Memory.phase = 3;
    return;
  }
  if (Memory.phase === 3) {
    if (spike !== undefined) {
      Memory.finalSpike = spike.hits;
      Memory.finalRaider = raider.hits;
    }
    if (core !== undefined) {
      Memory.finalCore = core.hits;
      Memory.finalSniper = sniper === undefined ? 0 : sniper.hits;
      Memory.finalEnergy = core.store.energy;
    }
    Memory.phase = 4;
  }
};
