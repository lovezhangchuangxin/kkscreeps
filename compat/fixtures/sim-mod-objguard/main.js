// 四轮探针（每 creep 每 tick 只有一条 attack 意图）：brawler 近战经营
// warden aegis（原始 30/击），skirmisher 近战经营 logbook monolith。
// T0 发动作并读初值（读值是提交前视图）；T1 读提交并续击；T2 再击；
// T3 读终值。包缺席时对应 creep 空转，场景共用同一探针。
exports.loop = function () {
  const room = Game.rooms.W1N1;
  const brawler = Game.creeps.brawler;
  const skirmisher = Game.creeps.skirmisher;
  const aegis =
    typeof FIND_WARDEN_AEGISES !== 'undefined' ? room.find(FIND_WARDEN_AEGISES)[0] : undefined;
  const monolith =
    typeof FIND_LOGBOOK_MONOLITHS !== 'undefined'
      ? room.find(FIND_LOGBOOK_MONOLITHS)[0]
      : undefined;
  const decoy =
    typeof FIND_WARDEN_DECOYS !== 'undefined' ? room.find(FIND_WARDEN_DECOYS)[0] : undefined;
  if (!Memory.phase) {
    if (aegis !== undefined) {
      Memory.aegisInitial = aegis.hits;
      Memory.brawlerCode = brawler.attack(aegis);
    }
    if (monolith !== undefined) {
      Memory.monoInitial = monolith.hits;
      Memory.skirmisherCode = skirmisher.attack(monolith);
    }
    if (decoy !== undefined) Memory.decoyCode = skirmisher.attack(decoy);
    Memory.phase = 1;
    return;
  }
  if (Memory.phase === 1) {
    if (aegis !== undefined) {
      Memory.aegisCommitted = aegis.hits;
      brawler.attack(aegis);
    }
    if (monolith !== undefined) {
      Memory.monoCommitted = monolith.hits;
      skirmisher.attack(monolith);
    }
    Memory.phase = 2;
    return;
  }
  if (Memory.phase === 2) {
    if (aegis !== undefined) {
      Memory.aegisMid = aegis.hits;
      brawler.attack(aegis);
    }
    if (monolith !== undefined) {
      Memory.monoMid = monolith.hits;
      skirmisher.attack(monolith);
    }
    Memory.phase = 3;
    return;
  }
  if (Memory.phase === 3) {
    if (aegis !== undefined) Memory.aegisFinal = aegis.hits;
    if (monolith !== undefined) Memory.monoFinal = monolith.hits;
    Memory.phase = 4;
  }
};
