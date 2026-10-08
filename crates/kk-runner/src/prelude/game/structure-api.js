/* 装配结构读面与动作：控制器、塔、link、terminal，以及孵化模块的安装阶段。
 * 复用 structures.js 的常量；方法记录指令，Rust 结算负责实际修改世界。
 * 孵化名称和待出生视图由 structures/spawn.js 的闭包持有。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  const _ = globalThis.__kkp.lodash;
  // 复用 structures.js 提供的常量与时间函数。
  const C = gw.__structuresC;
  const now = gw.now;
  const { assertTargetObject, isKnownCreep, isKnownStructure } = globalThis.__kkp.targets;
  const { deprecated, resetDeprecated } = globalThis.__kkp.deprecation;
  const {
    installCoreFields,
    installAssembler: installStructureAssembler,
    installSpecialFields,
  } = globalThis.__kkp.structureFields;

  // ---------------------------------------------------------------- per-tick 状态
  // 保持单个钩子的位置及名称→诊断重置顺序；名称状态由孵化模块独占。
  // runtime/world-tick.js beginTick 调用，来源：上游 structures.js 的 make 重置。
  const { createSpawnApi } = globalThis.__kkp;
  const {
    resetNames: resetSpawnNames,
    installView: installSpawnView,
    installMethods: installSpawnMethods,
    installNotify: installSpawnNotify,
    installAssembler: installSpawnAssembler,
  } = createSpawnApi({ deprecated, assertTargetObject, isKnownCreep });
  (gw.perTickReset = gw.perTickReset || []).push(() => {
    resetSpawnNames();
    resetDeprecated();
  });

  installSpawnView();

  installCoreFields();

  installSpawnMethods();

  // 结构受攻击通知设置。
  /** Structure.notifyWhenAttacked 基类版（官方 structures.js:89-101）：
   *  无房 INVALID_TARGET；非己结构（my === false——undefined 中性结构放行）
   *  或房 controller 归他人 → NOT_OWNER；非布尔参数 INVALID_ARGS；与当前
   *  存值不同才记 intent（官方 `!=`——undefined 与任何布尔不等，首调恒记）。
   *  挂 Structure 基类原型（官方同位）：controller 经 OwnedStructure 继承链
   *  同样持有本方法（官方语义即如此——其 intent 在 processor 层因 controller
   *  不在 CONSTRUCTION_COST 而空转）；source/mineral 官方不 extends Structure，
   *  我方原型链同构（RoomObject 直挂）不受影响。 */
  globalThis.Structure.prototype.notifyWhenAttacked = function notifyWhenAttacked(enabled) {
    if (!this.room) return C.ERR_INVALID_TARGET;
    const ctl = this.room.controller;
    if (this.my === false || (ctl && ctl.owner && !ctl.my)) {
      return C.ERR_NOT_OWNER;
    }
    if (typeof enabled !== 'boolean') return C.ERR_INVALID_ARGS;
    if (enabled !== gw.rawData(this.id).notifyWhenAttacked) {
      gw.intents.set(this.id, 'notifyWhenAttacked', { enabled });
    }
    return C.OK;
  };

  // 动作方法挂在类原型上；装配器不能再添加同名实例方法，否则会遮蔽这些实现。

  // 控制器操作方法。
  /** StructureController.unclaim：官方 structures.js:200-208——!my 门后记
   *  intent（结算侧 unclaim.js 清主权，settle/controller.rs）。 */
  globalThis.StructureController.prototype.unclaim = function unclaim() {
    if (!this.my) return C.ERR_NOT_OWNER;
    gw.intents.set(this.id, 'unclaim', {});
    return C.OK;
  };

  /** 先检查归属、可用次数、冷却、升级封锁与降级剩余时间，再检查自家已有安全模式。
   * 同 tick 多个有效请求只保留最后一个，并移除上一个请求的指令费用。
   * 缺少 downgradeTime 时此实现读取 0，会影响 ERR_TIRED 预检；Rust 结算另行复核。
   * 来源：上游 engine/src/game/structures.js 的 activateSafeMode。 */
  globalThis.StructureController.prototype.activateSafeMode = function activateSafeMode() {
    if (!this.my) return C.ERR_NOT_OWNER;
    if (this.safeModeAvailable <= 0) return C.ERR_NOT_ENOUGH_RESOURCES;
    if (
      this.safeModeCooldown ||
      this.upgradeBlocked > 0 ||
      this.ticksToDowngrade <
        C.CONTROLLER_DOWNGRADE[this.level] / 2 - C.CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD
    ) {
      return C.ERR_TIRED;
    }
    // 扫描所有自家控制器是否已有生效的安全模式，按索引读取以避开数组迭代器补丁。
    const objs = gw.world.objects || [];
    for (let i = 0; i < objs.length; i++) {
      const o = objs[i];
      if (
        o.type === 'structure' &&
        o.structureType === 'controller' &&
        o.owner === gw.viewerName &&
        o.safeMode &&
        o.safeMode > now()
      ) {
        return C.ERR_BUSY;
      }
    }
    if (gw.lastActivateSafeMode) {
      gw.intents.remove(gw.lastActivateSafeMode, 'activateSafeMode');
    }
    gw.lastActivateSafeMode = this.id;
    gw.intents.set(this.id, 'activateSafeMode', {});
    return C.OK;
  };

  installSpawnNotify();

  installSpawnAssembler();

  installStructureAssembler();

  // 销毁需要本房自家控制器，且房内没有敌方 creep/powerCreep，然后记录房间指令。
  // 方法挂在 Structure.prototype；controller 通过继承获得它，Source/Mineral 不继承 Structure。
  // 来源：上游 engine/src/game/structures.js 的 Structure.destroy。
  globalThis.Structure.prototype.destroy = function destroy() {
    if (!this.room) return C.ERR_INVALID_TARGET;
    if (!this.room.controller || !this.room.controller.my) return C.ERR_NOT_OWNER;
    if (
      this.room.find(C.FIND_HOSTILE_CREEPS).length > 0 ||
      this.room.find(C.FIND_HOSTILE_POWER_CREEPS).length > 0
    ) {
      return C.ERR_BUSY;
    }
    gw.intents.pushByName('room', 'destroyStructure', { roomName: this.room.name, id: this.id });
    return C.OK;
  };

  installSpecialFields();

  // 结构动作的 API 预检与指令记录。
  /** 塔动作共用能量与 isActive 检查；目标检查由各动作先完成。
   * 没有控制器时 isActive 返回 false，因此返回 ERR_RCL_NOT_ENOUGH。 */
  function towerCommonGates(tower) {
    const o = gw.rawData(tower.id);
    if (!o.store || o.store.energy < C.TOWER_ENERGY_COST) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    if (!tower.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    return C.OK;
  }

  /** 检查本人权限、当前 tick 的目标类型与存活状态，再检查能量和名额。
   * 此 API 不检查距离，距离衰减由结算处理。来源：上游 engine/src/game/structures.js 的 StructureTower.attack。 */
  globalThis.StructureTower.prototype.attack = function attack(target) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    // Mod 耐久对象目标族（G09.b）与 creep attack 同款判别（经 gw 共享，
    // creep.js 装配期挂载）；结算侧塔伤害经同一 object_hits 门接纳。
    const modTargetOk =
      gw.modDurabilityTargetOk !== undefined ? gw.modDurabilityTargetOk(target) : false;
    const okTarget =
      target &&
      target.id &&
      (isKnownCreep(target.id) || isKnownStructure(target.id) || modTargetOk) &&
      (target instanceof globalThis.Creep ||
        target instanceof globalThis.StructureSpawn ||
        target instanceof globalThis.Structure ||
        modTargetOk);
    if (!okTarget) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const gate = towerCommonGates(this);
    if (gate !== C.OK) {
      return gate;
    }
    gw.intents.set(this.id, 'attack', { id: target.id });
    return C.OK;
  };

  /** 目标限当前 tick 中存活的 Creep，检查能量与启用状态后记录 heal。
   * 此实现尚不接受 PowerCreep。来源：上游 engine/src/game/structures.js 的 StructureTower.heal。 */
  globalThis.StructureTower.prototype.heal = function heal(target) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (!(target && target.id && isKnownCreep(target.id) && target instanceof globalThis.Creep)) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const gate = towerCommonGates(this);
    if (gate !== C.OK) {
      return gate;
    }
    gw.intents.set(this.id, 'heal', { id: target.id });
    return C.OK;
  };

  /** 官方 Tower.repair（:812-834）：目标限 structure register + instanceof。 */
  globalThis.StructureTower.prototype.repair = function repair(target) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    const okTarget =
      target &&
      target.id &&
      isKnownStructure(target.id) &&
      (target instanceof globalThis.Structure || target instanceof globalThis.StructureSpawn);
    if (!okTarget) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    const gate = towerCommonGates(this);
    if (gate !== C.OK) {
      return gate;
    }
    gw.intents.set(this.id, 'repair', { id: target.id });
    return C.OK;
  };

  /** 官方 Link.transferEnergy（structures.js:488-531）十二步逐字：
   *  ①amount<0→INVALID_ARGS ②目标门（structure register+instanceof+非
   *  自身+structureType==link）→INVALID_TARGET ③!target.my→NOT_OWNER
   *  ④源是敌方 link（my===false 精确比较）且站 rampart→NOT_OWNER（官方
   *  奇行 :503-505）⑤源冷却→TIRED ⑥无 controller→RCL ⑦RCL 配额
   *  （isActive 等价，见 towerCommonGates 注）⑧无能量→NOT_ENOUGH_ENERGY
   *  ⑨amount 缺省=min(源能量, 目标余量) ⑩源能量<amount→NOT_ENOUGH_ENERGY
   *  ⑪目标无容量键/满→FULL ⑫跨房→NOT_IN_RANGE（**同房限定**——官方
   *  :526-528；结算层的本格坐标距离即全语义）。 */
  globalThis.StructureLink.prototype.transferEnergy = function transferEnergy(target, amount) {
    if (amount < 0) {
      return C.ERR_INVALID_ARGS;
    }
    if (
      !target ||
      !target.id ||
      !isKnownStructure(target.id) ||
      !(target instanceof globalThis.Structure) ||
      target === this ||
      target.structureType !== C.STRUCTURE_LINK
    ) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (!target.my) {
      return C.ERR_NOT_OWNER;
    }
    if (
      this.my === false &&
      _.any(this.pos.lookFor(C.LOOK_STRUCTURES), (i) => i.structureType === C.STRUCTURE_RAMPART)
    ) {
      return C.ERR_NOT_OWNER;
    }
    if (this.cooldown > 0) {
      return C.ERR_TIRED;
    }
    if (!this.room.controller) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    const o = gw.rawData(this.id);
    if (!o.store || !o.store.energy) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    if (!amount) {
      const t = gw.rawData(target.id);
      amount = Math.min(
        o.store.energy,
        t.storeCapacityResource ? t.storeCapacityResource.energy - t.store.energy : 0,
      );
    }
    if (this.energy < amount) {
      return C.ERR_NOT_ENOUGH_ENERGY;
    }
    const t = gw.rawData(target.id);
    if (
      !t.storeCapacityResource ||
      !t.storeCapacityResource.energy ||
      t.store.energy + amount > t.storeCapacityResource.energy
    ) {
      return C.ERR_FULL;
    }
    if (target.pos.roomName !== this.pos.roomName) {
      return C.ERR_NOT_IN_RANGE;
    }
    gw.intents.set(this.id, 'transfer', { id: target.id, amount, resourceType: 'energy' });
    return C.OK;
  };

  /** 官方 Rampart.setPublic（structures.js:652-657）：!my → intent
   *  {isPublic: !!isPublic}——结算 ramparts/set-public.js 同式写回。 */
  globalThis.StructureRampart.prototype.setPublic = function setPublic(isPublic) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    gw.intents.set(this.id, 'setPublic', { isPublic: !!isPublic });
    return C.OK;
  };

  /** 官方 StructureTerminal.send（structures.js:714-745）九步校验序逐字
   *  （TERMINAL-SEND-013 的 36 变体矩阵锁的就是此序）——2B 批 P1：
   *  ①!my→NOT_OWNER ②RCL（官方直调 checkStructureAgainstController——
   *  terminal 在 CONTROLLER_STRUCTURES 表内，isActive 四门等价【towerCommonGates
   *  注同款，无 controller 房走③门返 RCL=合意外延】）③房名 regex→INVALID_ARGS
   *  ④RESOURCES_ALL→INVALID_ARGS ⑤store 保有（`!store || !store[rt] ||
   *  store[rt] < amount`）→NOT_ENOUGH_RESOURCES ⑥cooldownTime>time→TIRED
   *  ⑦能量成本（rt≠energy：energy<cost / rt==energy：energy<amount+cost）
   *  →NOT_ENOUGH_RESOURCES ⑧description（非 string 或 >100）→INVALID_ARGS
   *  ⑨intents.set(id,'send',{resourceType,amount,targetRoomName,description})。
   *  **无最小量门**（TERMINAL_MIN_SEND 死常量不消费——TERMINAL-SEND-014 锁
   *  amount=1 可发）；intent payload 官方键序（send.js 消费 targetRoomName/
   *  resourceType/amount/description，description 经 userString 帽在清洗层）。 */
  globalThis.StructureTerminal.prototype.send = function send(
    resourceType,
    amount,
    targetRoomName,
    description,
  ) {
    if (!this.my) {
      return C.ERR_NOT_OWNER; // :715-717
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH; // :718-720（isActive 等价见注）
    }
    if (!/^(W|E)\d+(N|S)\d+$/.test(targetRoomName)) {
      return C.ERR_INVALID_ARGS; // :721-723
    }
    if (!_.contains(C.RESOURCES_ALL || [], resourceType)) {
      return C.ERR_INVALID_ARGS; // :724-726
    }
    const o = gw.rawData(this.id);
    if (!o.store || !o.store[resourceType] || o.store[resourceType] < amount) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :727-729
    }
    if (o.cooldownTime > gw.world.time) {
      return C.ERR_TIRED; // :730-732
    }
    // :733-737 距离/成本——与 Game.market.calcTransactionCost 同式（不环绕
    // plain Chebyshev 登记；Rust 侧 settle/market.rs calc_rooms_distance 同判）
    const xy1 = gw.roomNameToXY(this.pos.roomName);
    const xy2 = gw.roomNameToXY(targetRoomName);
    const range = Math.max(Math.abs(xy2[0] - xy1[0]), Math.abs(xy2[1] - xy1[1]));
    const cost = Math.ceil(amount * (1 - Math.exp(-range / 30)));
    if (
      (resourceType !== C.RESOURCE_ENERGY && o.store.energy < cost) ||
      (resourceType === C.RESOURCE_ENERGY && o.store.energy < amount + cost)
    ) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :738-741
    }
    if (description && (!_.isString(description) || description.length > 100)) {
      return C.ERR_INVALID_ARGS; // :742-744（length 为 UTF-16 码元——清洗层
      // userString 的 chars() 口径差异为既有登记）
    }
    gw.intents.set(this.id, 'send', { resourceType, amount, targetRoomName, description });
    return C.OK; // :745
  };

  // ---- lab 四动作（M2E）。官方 processor 对失败静默 no-op，错误码全在
  // driver 预检层；下列各方法按 screeps-ok 验证矩阵钉死的优先级序输出
  // 错误码（LAB-RUN-013/LAB-REVERSE-013/BOOST-CREEP-010/UNBOOST-006 的
  // singles+pairs 期望取声明序前者的码——即此处的判定顺序）。

  /** lab 动作共用的目标门：存在、在结构注册表且是 StructureLab 实例。 */
  function assertLabTarget(lab) {
    return !!(
      lab &&
      lab.id &&
      isKnownStructure(lab.id) &&
      lab instanceof globalThis.Structure &&
      lab.structureType === C.STRUCTURE_LAB
    );
  }

  /** StructureLab.runReaction（LAB-RUN-013 序）：not-owner > cooldown >
   *  rcl > invalid-target > range > full > not-enough > invalid-args。
   *  invalid-target 含试剂 lab 为调用 lab 自身的形态（官方 driver 目标门
   *  拒 self，与 reverseReaction 同款；processor 无此检查，结算侧自指向
   *  因自持矿物≠产物自然 no-op）。full 读调用 lab 自持矿物的常量容量
   *  （官方驱动同式，不读已绑槽）；invalid-args 含两形态：试剂对无产物、
   *  调用 lab 已含非产物矿物。
   *  来源：processor/intents/labs/run-reaction.js 与官方 driver 预检。 */
  globalThis.StructureLab.prototype.runReaction = function runReaction(lab1, lab2) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (this.cooldown > 0) {
      return C.ERR_TIRED;
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (
      !assertLabTarget(lab1) ||
      !assertLabTarget(lab2) ||
      lab1.id === this.id ||
      lab2.id === this.id
    ) {
      assertTargetObject(lab1);
      assertTargetObject(lab2);
      return C.ERR_INVALID_TARGET;
    }
    if (inRangeLabs(this, [lab1, lab2]) === false) {
      return C.ERR_NOT_IN_RANGE;
    }
    if (
      this.mineralType &&
      this.store[this.mineralType] + C.LAB_REACTION_AMOUNT > this.mineralCapacity
    ) {
      return C.ERR_FULL;
    }
    const m1 = lab1.mineralType;
    const m2 = lab2.mineralType;
    if (
      !m1 ||
      lab1.store[m1] < C.LAB_REACTION_AMOUNT ||
      !m2 ||
      lab2.store[m2] < C.LAB_REACTION_AMOUNT
    ) {
      return C.ERR_NOT_ENOUGH_RESOURCES;
    }
    const product = C.REACTIONS[m1] && C.REACTIONS[m1][m2];
    if (!product || (this.mineralType && this.mineralType !== product)) {
      return C.ERR_INVALID_ARGS;
    }
    gw.intents.set(this.id, 'runReaction', { lab1: lab1.id, lab2: lab2.id });
    return C.OK;
  };

  /** StructureLab.reverseReaction（LAB-REVERSE-013 序）：not-owner >
   *  cooldown > rcl > invalid-target > range > same-lab(INVALID_ARGS) >
   *  not-enough > invalid-reverse-pair(INVALID_ARGS) > full。
   *  invalid-target 含输出 lab 为调用 lab 自身的形态（矩阵对
   *  invalidTargetBeforeSameLab 用调用 lab 充两参锁 INVALID_TARGET——
   *  官方 driver 目标门拒 self；processor 无此检查，结算侧自指向会因
   *  试剂不匹配自然 no-op）。试剂对存在性取 REACTIONS 逆查；输出 lab
   *  已持不相容矿物或化合物无试剂对同为 invalid-reverse-pair 形态。
   *  full 只在输出 lab 已持同种矿物且加量越容量时成立。 */
  globalThis.StructureLab.prototype.reverseReaction = function reverseReaction(lab1, lab2) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (this.cooldown > 0) {
      return C.ERR_TIRED;
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (
      !assertLabTarget(lab1) ||
      !assertLabTarget(lab2) ||
      lab1.id === this.id ||
      lab2.id === this.id
    ) {
      assertTargetObject(lab1);
      return C.ERR_INVALID_TARGET;
    }
    if (inRangeLabs(this, [lab1, lab2]) === false) {
      return C.ERR_NOT_IN_RANGE;
    }
    if (lab1.id === lab2.id) {
      return C.ERR_INVALID_ARGS;
    }
    const mineral = this.mineralType;
    if (!mineral || this.store[mineral] < C.LAB_REACTION_AMOUNT) {
      return C.ERR_NOT_ENOUGH_RESOURCES;
    }
    const variant = reactionVariant(mineral, lab1, lab2);
    if (!variant) {
      return C.ERR_INVALID_ARGS;
    }
    const outputs = [
      [lab1, variant[0]],
      [lab2, variant[1]],
    ];
    for (let i = 0; i < outputs.length; i++) {
      const [lab, reagent] = outputs[i];
      if (
        lab.mineralType === reagent &&
        lab.store[reagent] + C.LAB_REACTION_AMOUNT > lab.mineralCapacity
      ) {
        return C.ERR_FULL;
      }
    }
    gw.intents.set(this.id, 'reverseReaction', { lab1: lab1.id, lab2: lab2.id });
    return C.OK;
  };

  /** StructureLab.boostCreep（BOOST-CREEP-010 序）：not-owner > rcl >
   *  invalid-target > range > not-enough-energy > not-enough-mineral >
   *  not-found。无冷却门（官方同形）。not-found 判定与结算同源：
   *  身体中存在未强化且 BOOSTS 表命中该矿物的部件。 */
  globalThis.StructureLab.prototype.boostCreep = function boostCreep(target, bodyPartsCount) {
    if (!this.my) {
      return C.ERR_NOT_OWNER;
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (
      !target ||
      !target.id ||
      !isKnownCreep(target.id) ||
      !(target instanceof globalThis.Creep) ||
      target.spawning
    ) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (cheb1(this, target) === false) {
      return C.ERR_NOT_IN_RANGE;
    }
    if (this.store.energy < C.LAB_BOOST_ENERGY) {
      return C.ERR_NOT_ENOUGH_RESOURCES;
    }
    const mineral = this.mineralType;
    if (!mineral || this.store[mineral] < C.LAB_BOOST_MINERAL) {
      return C.ERR_NOT_ENOUGH_RESOURCES;
    }
    const body = target.body || [];
    let candidates = 0;
    for (let i = 0; i < body.length; i++) {
      const p = body[i];
      if (!p.boost && C.BOOSTS[p.type] && C.BOOSTS[p.type][mineral]) {
        candidates += 1;
      }
    }
    if (candidates === 0) {
      return C.ERR_NOT_FOUND;
    }
    // 官方 driver 第二形态（API 文档载明；processor 无此检查）：bodyPartsCount
    // 真值且超过可强化部件数时同样拒绝，不会部分强化。
    if (bodyPartsCount && bodyPartsCount > candidates) {
      return C.ERR_NOT_FOUND;
    }
    gw.intents.set(this.id, 'boostCreep', { id: target.id, bodyPartsCount });
    return C.OK;
  };

  /** StructureLab.unboostCreep（UNBOOST-006 序）：invalid-target >
   *  lab-not-owner > creep-not-owner > rcl > cooldown > not-found > range
   *  （官方 `!this.my || !target.my` 同级 NOT_OWNER 门）。not-found 判
   *  目标身体存在任一强化部件。 */
  globalThis.StructureLab.prototype.unboostCreep = function unboostCreep(target) {
    if (
      !target ||
      !target.id ||
      !isKnownCreep(target.id) ||
      !(target instanceof globalThis.Creep)
    ) {
      assertTargetObject(target);
      return C.ERR_INVALID_TARGET;
    }
    if (!this.my || !target.my) {
      return C.ERR_NOT_OWNER;
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH;
    }
    if (this.cooldown > 0) {
      return C.ERR_TIRED;
    }
    const body = target.body || [];
    let boosted = false;
    for (let i = 0; i < body.length; i++) {
      if (body[i].boost) {
        boosted = true;
        break;
      }
    }
    if (!boosted) {
      return C.ERR_NOT_FOUND;
    }
    if (cheb1(this, target) === false) {
      return C.ERR_NOT_IN_RANGE;
    }
    gw.intents.set(this.id, 'unboostCreep', { id: target.id });
    return C.OK;
  };

  /** 官方射程门（run/reverse）：两坐标差均 ≤2，跨房恒超程（官方
   *  RoomPosition 距离语义）。返回 false 表示超程。 */
  function inRangeLabs(self, labs) {
    for (let i = 0; i < labs.length; i++) {
      const lab = labs[i];
      if (
        lab.pos.roomName !== self.pos.roomName ||
        Math.abs(lab.pos.x - self.pos.x) > 2 ||
        Math.abs(lab.pos.y - self.pos.y) > 2
      ) {
        return false;
      }
    }
    return true;
  }

  /** 官方射程门（boost/unboost）：Chebyshev ≤1，跨房恒超程。
   *  返回 false 表示超程。 */
  function cheb1(self, target) {
    if (target.pos.roomName !== self.pos.roomName) {
      return false;
    }
    return Math.max(Math.abs(target.pos.x - self.pos.x), Math.abs(target.pos.y - self.pos.y)) <= 1;
  }

  /** REACTIONS 逆查：产物的两个有向试剂对中取首个与两输出 lab 现有
   *  内容相容的方向（空 lab 接收任一试剂）。与官方 getReactionVariants +
   *  _.find 等价（表迭代序与排序序的差异只影响空 lab 的分配方向，
   *  不影响存在性判定）。 */
  function reactionVariant(mineral, lab1, lab2) {
    let pair = null;
    for (const r1 of Object.keys(C.REACTIONS)) {
      const inner = C.REACTIONS[r1];
      for (const r2 of Object.keys(inner)) {
        if (inner[r2] === mineral) {
          pair = [r1, r2];
          break;
        }
      }
      if (pair) break;
    }
    if (!pair) {
      return null;
    }
    const ok = (m, reagent) => !m || m === reagent;
    if (ok(lab1.mineralType, pair[0]) && ok(lab2.mineralType, pair[1])) {
      return pair;
    }
    if (ok(lab1.mineralType, pair[1]) && ok(lab2.mineralType, pair[0])) {
      return [pair[1], pair[0]];
    }
    return null;
  }

  /** StructureFactory.produce（官方 game/structures.js:1434-1469 九步逐字，
   *  FACTORY-PRODUCE-011 矩阵钉序）：not-owner > cooldown > invalid-args
   *  （非配方资源）> level-mismatch(INVALID_TARGET) > rcl（checkStructureAgainstController
   *  ——isActive 等价）> power-effect(BUSY) > not-enough > full。BUSY 门按
   *  官方形状查 effects 数组（power 域未接线时恒无 → 有级配方直接 BUSY）；
   *  full 含 `!storeCapacity` 先拒形态与平面容量合计（store 总量−组件
   *  合计+产量 > storeCapacity）。 */
  globalThis.StructureFactory.prototype.produce = function produce(resourceType) {
    if (!this.my) {
      return C.ERR_NOT_OWNER; // :1435-1437
    }
    if (this.cooldown > 0) {
      return C.ERR_TIRED; // :1439-1441
    }
    const recipe = C.COMMODITIES[resourceType];
    if (!recipe) {
      return C.ERR_INVALID_ARGS; // :1443-1445
    }
    const raw = gw.rawData(this.id);
    if (recipe.level && recipe.level !== raw.level) {
      return C.ERR_INVALID_TARGET; // :1448-1450
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH; // :1453-1455
    }
    if (recipe.level && raw.level > 0) {
      const effects = raw.effects || [];
      let operated = false;
      for (let i = 0; i < effects.length; i++) {
        const e = effects[i];
        if (
          e.power === C.PWR_OPERATE_FACTORY &&
          e.level === recipe.level &&
          e.endTime >= now()
        ) {
          operated = true;
          break;
        }
      }
      if (!operated) {
        return C.ERR_BUSY; // :1458-1460
      }
    }
    let missing = false;
    for (const p of Object.keys(recipe.components)) {
      if ((raw.store ? raw.store[p] || 0 : 0) < recipe.components[p]) {
        missing = true;
        break;
      }
    }
    if (missing) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :1463-1465
    }
    if (!raw.storeCapacity) {
      return C.ERR_FULL; // :1468 前半（无平面容量即满）
    }
    let total = 0;
    const store = raw.store || {};
    for (const r of Object.keys(store)) total += store[r] || 0;
    let componentsTotal = 0;
    for (const p of Object.keys(recipe.components)) {
      componentsTotal += recipe.components[p];
    }
    if (total - componentsTotal + (recipe.amount || 1) > raw.storeCapacity) {
      return C.ERR_FULL; // :1468 后半
    }
    gw.intents.set(this.id, 'produce', { resourceType });
    return C.OK;
  };

  /** StructureNuker.launchNuke（官方 game/structures.js:1356-1389 逐字，
   *  NUKE-LAUNCH-008 矩阵钉序）：not-owner > invalid-args（instanceof
   *  RoomPosition——plainObject 拒）> [房间状态 novice/respawn→INVALID_TARGET
   *  ——kk 无 roomStatus 数据，跳过] > TIRED > RCL（checkStructureAgainstController
   *  ——isActive 等价，nuker 在 CONTROLLER_STRUCTURES 表内）> NOT_IN_RANGE
   *  （roomNameToXY 差绝对值 > NUKE_RANGE）> NOT_ENOUGH_RESOURCES
   *  （energy<energyCapacity || ghodium<ghodiumCapacity）。 */
  globalThis.StructureNuker.prototype.launchNuke = function launchNuke(pos) {
    if (!this.my) {
      return C.ERR_NOT_OWNER; // :1357-1359
    }
    if (!(pos instanceof gw.RoomPosition)) {
      return C.ERR_INVALID_ARGS; // :1360-1362
    }
    if (this.cooldown > 0) {
      return C.ERR_TIRED; // :1370-1372
    }
    if (!this.isActive()) {
      return C.ERR_RCL_NOT_ENOUGH; // :1373-1375
    }
    const [tx, ty] = gw.roomNameToXY(pos.roomName);
    const [x, y] = gw.roomNameToXY(this.room.name);
    if (Math.abs(tx - x) > C.NUKE_RANGE || Math.abs(ty - y) > C.NUKE_RANGE) {
      return C.ERR_NOT_IN_RANGE; // :1379-1381
    }
    if (this.energy < this.energyCapacity || this.ghodium < this.ghodiumCapacity) {
      return C.ERR_NOT_ENOUGH_RESOURCES; // :1382-1384
    }
    gw.intents.set(this.id, 'launchNuke', {
      roomName: pos.roomName,
      x: pos.x,
      y: pos.y,
    });
    return C.OK;
  };
})();
