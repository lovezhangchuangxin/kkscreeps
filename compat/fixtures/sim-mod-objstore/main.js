// 四轮探针（双 creep：每 creep 每 tick 只有一条 transfer 意图——后设
// 覆盖前者，官方语义）：mule 经营 crate，courier 经营 vault cache。
// T0 发动作（读值是提交前视图）；T1 读提交并补满；T2 探满载/提取；
// T3 读终值。
exports.loop = function () {
  const room = Game.rooms.W1N1;
  const mule = Game.creeps.mule;
  const crate =
    typeof FIND_HAULER_CRATES !== 'undefined' ? room.find(FIND_HAULER_CRATES)[0] : undefined;
  const cache =
    typeof FIND_VAULT_CACHES !== 'undefined' ? room.find(FIND_VAULT_CACHES)[0] : undefined;
  const courier = Game.creeps.courier;
  if (!Memory.phase) {
    if (crate === undefined) {
      Memory.initialEnergy = 0;
      Memory.initialCapacity = 0;
      Memory.transferCode = -7;
      Memory.markerCode = -7;
    } else {
      Memory.initialEnergy = crate.store.energy;
      Memory.initialCapacity = crate.storeCapacity;
      Memory.transferCode = mule.transfer(crate, RESOURCE_ENERGY, 200);
      Memory.preEnergy = mule.store.energy;
      Memory.markerCode = mule.transfer(room.find(FIND_HAULER_MARKERS)[0], RESOURCE_ENERGY, 10);
    }
    if (cache !== undefined && courier !== undefined) {
      Memory.vaultCapacity = cache.storeCapacity;
      Memory.vaultCode = courier.transfer(cache, RESOURCE_ENERGY, 100);
    }
    Memory.phase = 1;
    return;
  }
  if (Memory.phase === 1) {
    if (crate !== undefined) {
      Memory.committedCrate = crate.store.energy;
      Memory.committedMule = mule.store.energy;
      Memory.fillCode = mule.transfer(crate, RESOURCE_ENERGY, 100);
    }
    if (cache !== undefined && courier !== undefined) {
      Memory.committedVault = cache.store.energy;
      Memory.committedCourier = courier.store.energy;
      Memory.vaultFull = courier.transfer(cache, RESOURCE_ENERGY, 1);
    }
    Memory.phase = 2;
    return;
  }
  if (Memory.phase === 2) {
    if (crate !== undefined) {
      Memory.filledCrate = crate.store.energy;
      Memory.fullCode = mule.transfer(crate, RESOURCE_ENERGY, 1);
      Memory.withdrawCode = mule.withdraw(crate, RESOURCE_ENERGY, 50);
    }
    Memory.phase = 3;
    return;
  }
  if (Memory.phase === 3) {
    if (crate !== undefined) {
      Memory.afterWithdraw = crate.store.energy;
      Memory.finalMule = mule.store.energy;
    }
    if (cache !== undefined && courier !== undefined) {
      Memory.finalVault = cache.store.energy;
      Memory.finalCourier = courier.store.energy;
    }
    Memory.phase = 4;
  }
};
