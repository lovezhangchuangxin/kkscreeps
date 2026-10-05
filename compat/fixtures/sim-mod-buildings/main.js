// N2b 建筑场景 bot：同一 main.js 服务三个世界——
// 安装 smelter 的世界走 建造→投料生产→搬运产物→受击维修 的真实闭环，
// 组合世界追加 panel（道路寻路、免费生产）验证第二种规则；
// 未安装任何包的对照世界里，工地创建与资源搬运在玩家预检即被拒绝。
module.exports.loop = function () {
  Memory.loops = (Memory.loops || 0) + 1;
  const room = Game.rooms[Memory.room];
  const storage = room.find(FIND_STRUCTURES).find((s) => s.structureType === 'storage');
  const hasSmelter = typeof SmelterFoundry === 'function';
  const hasPanel = typeof PanelCollector === 'function';
  if (!hasSmelter) {
    // 对照世界：类型白名单与资源目录都不认识 Mod 内容，且无任何残留。
    Memory.plain_world = true;
    Memory.no_globals = typeof SmelterFoundry === 'undefined'
      && typeof PanelCollector === 'undefined';
    const builder = Game.creeps.builder;
    Memory.site_rejected = room.createConstructionSite(14, 12, 'smelter:foundry') === ERR_INVALID_ARGS;
    Memory.withdraw_rejected = builder.withdraw(storage, 'smelter:ore', 10) === ERR_INVALID_ARGS;
    Memory.no_sites = room.find(FIND_CONSTRUCTION_SITES).length === 0;
    Memory.no_foundry = room.find(FIND_STRUCTURES).every((s) => s.structureType !== 'smelter:foundry');
    return;
  }
  const builder = Game.creeps.builder;
  const feeder = Game.creeps.feeder;
  const trader = Game.creeps.trader;
  const guard = Game.creeps.guard;
  const medic = Game.creeps.medic;
  const foundry = room.find(FIND_MY_STRUCTURES).find((s) => s.structureType === 'smelter:foundry');
  const panel = hasPanel
    ? room.find(FIND_MY_STRUCTURES).find((s) => s.structureType === 'panel:collector') : undefined;
  const sites = room.find(FIND_CONSTRUCTION_SITES);
  // 真实玩家路径创建工地（预检白名单经 Mod 建筑目录放行）。
  if (!foundry && !sites.some((s) => s.structureType === 'smelter:foundry')
    && !Memory.site_created) {
    Memory.site_code = room.createConstructionSite(14, 12, 'smelter:foundry');
    Memory.site_created = true;
  }
  if (hasPanel && !panel && !sites.some((s) => s.structureType === 'panel:collector')
    && !Memory.panel_site_created) {
    Memory.panel_site_code = room.createConstructionSite(15, 14, 'panel:collector');
    Memory.panel_site_created = true;
  }

  // 建造：成本资源任一为空回仓补货，两种资源都有则施工。
  // panel 只有能量成本（单资源规则走同一通用接口）。
  const site = sites.find((s) => s.structureType === 'smelter:foundry')
    || (hasPanel && sites.find((s) => s.structureType === 'panel:collector'));
  // 一次性预检探针：空载 creep 对 Mod 工地 build 须被玩家面
  // ERR_NOT_ENOUGH_RESOURCES 拒绝（成本资源集判断，内建则查能量）。
  if (Memory.probe_loops === undefined && site && builder) {
    Memory.build_precheck_empty = builder.build(site) === ERR_NOT_ENOUGH_RESOURCES;
    Memory.probe_loops = Memory.loops;
  }
  if (site && builder) {
    const energy = builder.store.energy || 0;
    const ore = builder.store[SMELTER_ORE] || 0;
    const free = builder.store.getFreeCapacity();
    const needsOre = site.structureType === 'smelter:foundry';
    if (energy === 0 && free >= 600) {
      const code = builder.withdraw(storage, 'energy', Math.min(600, free));
      if (code === ERR_NOT_IN_RANGE) builder.moveTo(storage, { reusePath: 20 });
    } else if (needsOre && ore === 0 && free >= 100) {
      const code = builder.withdraw(storage, SMELTER_ORE, Math.min(100, free));
      if (code === ERR_NOT_IN_RANGE) builder.moveTo(storage, { reusePath: 20 });
    } else if (energy > 0 && (!needsOre || ore > 0)) {
      if (builder.pos.inRangeTo(site, 3)) builder.build(site);
      else builder.moveTo(site, { reusePath: 20 });
    } else {
      builder.moveTo(site, { reusePath: 20 });
    }
  } else if (builder && foundry) {
    Memory.build_complete = true;
  }

  if (foundry) {
    Memory.foundry_hits = foundry.hits;
    Memory.foundry_hits_max = foundry.hitsMax;
    Memory.foundry_ingots = foundry.ingots;
    Memory.foundry_owner = foundry.owner.username;
    // 代价矩阵语义：obstacle 建筑格为 255。用 costCallback 回读玩家可见矩阵。
    if (Memory.grid_foundry === undefined) {
      room.findPath(new RoomPosition(10, 10, Memory.room), new RoomPosition(30, 30, Memory.room), {
        costCallback(name, matrix) {
          Memory.grid_foundry = matrix.get(foundry.pos.x, foundry.pos.y);
        },
      });
    }
    // 投料：矿石或能量任一为空回仓补货，transfer 进熔炉仓库。
    if (feeder) {
      const energy = feeder.store.energy || 0;
      const ore = feeder.store[SMELTER_ORE] || 0;
      const free = feeder.store.getFreeCapacity();
      if (energy === 0 && free >= 200) {
        const code = feeder.withdraw(storage, 'energy', Math.min(200, free));
        if (code === ERR_NOT_IN_RANGE) feeder.moveTo(storage, { reusePath: 20 });
      } else if (ore === 0 && free >= 150) {
        const code = feeder.withdraw(storage, SMELTER_ORE, Math.min(150, free));
        if (code === ERR_NOT_IN_RANGE) feeder.moveTo(storage, { reusePath: 20 });
      } else if (feeder.pos.inRangeTo(foundry, 1)) {
        const transferOre = foundry.store[SMELTER_ORE] < 60;
        feeder.transfer(foundry, transferOre ? SMELTER_ORE : 'energy',
          transferOre ? ore : Math.min(energy, 200));
      } else {
        feeder.moveTo(foundry, { reusePath: 20 });
      }
    }
    // 产物搬运：熔炉有锭则取出送回仓库。
    if (trader) {
      const ingots = trader.store[SMELTER_INGOT] || 0;
      if (ingots === 0) {
        if ((foundry.ingots || 0) >= 10) {
          const code = trader.withdraw(foundry, SMELTER_INGOT, 50);
          if (code === ERR_NOT_IN_RANGE) trader.moveTo(foundry, { reusePath: 20 });
        }
      } else if (trader.pos.inRangeTo(storage, 1)) {
        trader.transfer(storage, SMELTER_INGOT, ingots);
      } else {
        trader.moveTo(storage, { reusePath: 20 });
      }
      Memory.storage_ingots = storage.store[SMELTER_INGOT] || 0;
    }
    // 受击与维修：guard 远程打 6 次，medic 修到满。
    if (guard) {
      if ((Memory.attacks || 0) < 6) {
        const code = guard.rangedAttack(foundry);
        if (code === ERR_NOT_IN_RANGE) guard.moveTo(foundry, { reusePath: 20 });
        else if (code === OK) Memory.attacks = (Memory.attacks || 0) + 1;
      }
    }
    if (medic && foundry.hits < foundry.hitsMax && medic.store.energy > 0) {
      const code = medic.repair(foundry);
      if (code === ERR_NOT_IN_RANGE) medic.moveTo(foundry, { reusePath: 20 });
      else if (code === OK) Memory.repairs = (Memory.repairs || 0) + 1;
    } else if (medic && foundry.hits < foundry.hitsMax) {
      const code = medic.withdraw(storage, 'energy', 100);
      if (code === ERR_NOT_IN_RANGE) medic.moveTo(storage, { reusePath: 20 });
    }
  }
  if (panel) {
    Memory.panel_energy = panel.store.energy || 0;
    Memory.panel_hits = panel.hits;
    if (Memory.grid_panel === undefined) {
      room.findPath(new RoomPosition(10, 10, Memory.room), new RoomPosition(30, 30, Memory.room), {
        costCallback(name, matrix) {
          // road 语义建筑格成本为 1（可通行且按道路寻路）。
          Memory.grid_panel = matrix.get(panel.pos.x, panel.pos.y);
        },
      });
    }
  }
  Memory.storage_ore = storage ? (storage.store[SMELTER_ORE] || 0) : -1;
};
