/* 单房间创建预检读取本 tick 的原始对象；指令与计数仍由 Game 轮次状态持有。
 * 来源：上游 rooms.js 的工地/旗帜接口与 utils.js 的建造预检。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const TERRAIN_MASK_WALL = globalThis.TERRAIN_MASK_WALL;
  // 创建与直线寻路方法使用加载时捕获的常量。
  const C_CONTROLLER_STRUCTURES = globalThis.CONTROLLER_STRUCTURES;
  const C_CONSTRUCTION_COST = globalThis.CONSTRUCTION_COST;
  const C_MAX_CONSTRUCTION_SITES = globalThis.MAX_CONSTRUCTION_SITES;
  const C_FLAGS_LIMIT = globalThis.FLAGS_LIMIT;
  const C_COLORS_ALL = globalThis.COLORS_ALL;
  const C_COLOR_WHITE = globalThis.COLOR_WHITE;
  const C_ERR_NOT_OWNER = globalThis.ERR_NOT_OWNER;
  const C_ERR_RCL_NOT_ENOUGH = globalThis.ERR_RCL_NOT_ENOUGH;
  const C_ERR_INVALID_TARGET = globalThis.ERR_INVALID_TARGET;
  const C_ERR_FULL = globalThis.ERR_FULL;
  const C_ERR_NAME_EXISTS = globalThis.ERR_NAME_EXISTS;
  const C_OK = globalThis.OK;
  gw.makeRoomCreation = function (name, roomObjs) {
    const methods = {};
    // 创建预检使用 raw structureType 区分结构类型，type 区分结构、spawn 与工地。
    /** 在自家所有 Game.spawns 和 Game.constructionSites 中检查 spawn 重名。
     * 跨房工地也参与检查；与上游 register.spawns 的差异是此处不检查他人 spawn 名。 */
    function spawnNameTaken(name) {
      return Object.values(gw.currentGame().spawns).some((s) => s.name === name)
        || Object.values(gw.currentGame().constructionSites).some(
          (o) => o.structureType === 'spawn' && o.name === name,
        );
    }
    /** 官方 utils.js:338-354 checkControllerAvailability：现存同类型（含
     *  工地）< CONTROLLER_STRUCTURES[type][rcl]（rcl：有主 controller 的
     *  level，无主/无 controller 为 0——官方 (user||owner) 真值门的等价物）。
     *  表缺类型/配额 undefined → NaN 比较恒 false，官方同形。 */
    // Mod 建筑不占用内建 RCL 配额表（当前版本声明面无逐级配额）。
    function checkControllerAvailability(structureType, room) {
      const modBuildings = gw.modApi === undefined ? undefined : gw.modApi.buildings;
      if (modBuildings !== undefined && modBuildings[structureType] !== undefined) {
        return true;
      }
      const controller = room.controller;
      let rcl = 0;
      if (controller && controller.level && controller.owner) rcl = controller.level;
      let cnt = 0;
      for (const o of roomObjs) {
        if (o.structureType === structureType
          && (o.type === 'structure' || o.type === 'spawn' || o.type === 'constructionSite')) {
          cnt++;
        }
      }
      const quota = C_CONTROLLER_STRUCTURES[structureType];
      return cnt < (quota ? quota[rcl] : undefined);
    }
    /** 检查同格同类型或工地占位；extractor 需矿物，随后提前返回，不使用其他造价对象互斥检查。
     * 其他非 road/rampart 结构与造价对象互斥，房间边界拒绝建造。
     * 来源：上游 engine/src/utils.js 的 checkConstructionSite 对象分支。 */
    function checkConstructionSiteObjects(structureType, x, y) {
      const typeOf = (o) => ((o.type === 'structure' || o.type === 'spawn') && o.structureType)
        ? o.structureType : o.type;
      const modBuildings = gw.modApi === undefined ? undefined : gw.modApi.buildings;
      const costBearing = (t) => C_CONSTRUCTION_COST[t]
        || (modBuildings !== undefined && modBuildings[t] !== undefined);
      let hasMineral = false;
      let hasExtractor = false;
      const isExtractor = structureType === 'extractor';
      for (const o of roomObjs) {
        if (o.x !== x || o.y !== y) continue;
        const t = typeOf(o);
        if (t === structureType) return false;
        if (o.type === 'constructionSite') return false;
        if (t === 'mineral') hasMineral = true;
        if (t === 'extractor') hasExtractor = true;
        if (isExtractor) continue;
        if (structureType !== 'rampart' && structureType !== 'road'
          && t !== 'rampart' && t !== 'road' && costBearing(t)) {
          return false;
        }
      }
      if (isExtractor) return hasMineral && !hasExtractor;
      if (x <= 0 || y <= 0 || x >= 49 || y >= 49) return false;
      return true;
    }
    /** 非 road/container 在 1/48 边缘要求所选边界三格都是墙；extractor 提前通过，其他非 road 不可建墙上。
     * 来源：上游 engine/src/utils.js 的 checkConstructionSite 地形分支。 */
    function checkConstructionSiteTerrain(structureType, x, y) {
      const terrain = gw.terrainByRoom[name];
      if (!terrain) return false;
      const isWall = (tx, ty) => (terrain[ty * 50 + tx] & TERRAIN_MASK_WALL) !== 0;
      if (structureType !== 'road' && structureType !== 'container'
        && (x === 1 || x === 48 || y === 1 || y === 48)) {
        // 先检查 x 边，再检查 y 边；角落位置取最后命中的 y 边三格。
        // 来源：上游 engine/src/utils.js 的 checkConstructionSite 地形检查。
        let border;
        if (x === 1) border = [[0, y - 1], [0, y], [0, y + 1]];
        if (x === 48) border = [[49, y - 1], [49, y], [49, y + 1]];
        if (y === 1) border = [[x - 1, 0], [x, 0], [x + 1, 0]];
        if (y === 48) border = [[x - 1, 49], [x, 49], [x + 1, 49]];
        for (let i = 0; i < border.length; i++) {
          if (!isWall(border[i][0], border[i][1])) return false;
        }
      }
      if (structureType === 'extractor') return true;
      if (structureType !== 'road' && isWall(x, y)) return false;
      return true;
    }
    /** 按坐标、类型、spawn 名、控制器权限、名额、占位与总工地数的顺序检查，然后记录创建指令。
     * 本 tick 创建计数与世界快照中的已有工地数分别统计。
     * 来源：上游 engine/src/game/rooms.js 的 createConstructionSite。 */
    methods.createConstructionSite = function (firstArg, secondArg, structureType, name) {
      const xy = gw.fetchXYArguments(firstArg, secondArg);
      const x = xy[0], y = xy[1];
      if (x === undefined || y === undefined || x < 0 || x > 49 || y < 0 || y > 49) {
        return ERR_INVALID_ARGS;
      }
      if (typeof secondArg === 'string' && structureType === undefined) {
        structureType = secondArg;
      }
      const modBuildings = gw.modApi === undefined ? undefined : gw.modApi.buildings;
      if (!C_CONSTRUCTION_COST[structureType]
        && !(modBuildings !== undefined && modBuildings[structureType] !== undefined)) {
        return ERR_INVALID_ARGS;
      }
      if (structureType === 'spawn' && typeof name === 'string') {
        if (name.length > 100) return ERR_INVALID_ARGS;
        if (gw.createdSpawnNames.has(name)) return ERR_INVALID_ARGS;
        if (spawnNameTaken(name)) {
          return ERR_INVALID_ARGS;
        }
      }
      if (this.controller && this.controller.level > 0 && !this.controller.my) {
        return C_ERR_NOT_OWNER;
      }
      if (this.controller && this.controller.reservation
        && this.controller.reservation.username !== gw.viewerName) {
        return C_ERR_NOT_OWNER;
      }
      if (!checkControllerAvailability(structureType, this)) {
        return C_ERR_RCL_NOT_ENOUGH;
      }
      if (!checkConstructionSiteObjects(structureType, x, y)
        || !checkConstructionSiteTerrain(structureType, x, y)) {
        return C_ERR_INVALID_TARGET;
      }
      // 官方 rooms.js:1071-1073：自家工地总数 + 本 tick 已建数 < 帽（官方
      // 分开数是快照不随建增长；我方投影即本 tick 状态，两者合流同值）
      const ownSiteCount = Object.keys(gw.currentGame().constructionSites).length;
      if (ownSiteCount + gw.createdConstructionSites >= C_MAX_CONSTRUCTION_SITES) {
        return C_ERR_FULL;
      }
      const intent = { x, y, structureType };
      if (structureType === 'spawn') {
        if (typeof name !== 'string') {
          let cnt = 1;
          do {
            name = 'Spawn' + cnt;
            cnt++;
          } while (spawnNameTaken(name) || gw.createdSpawnNames.has(name));
        }
        gw.createdSpawnNames.add(name);
        intent.name = name;
      }
      // 官方 :1075 `const roomName = ""+this.name`——闭包参数 name 已被
      // spawn 名遮蔽，经 this.name 取房名
      intent.roomName = this.name;
      gw.createdConstructionSites++;
      gw.intents.pushByName('room', 'createConstructionSite', intent);
      return C_OK;
    };
    /** 官方 createFlag（rooms.js:978-1027，P2 转真）：fetchXY/边界 →
     *  FLAGS_LIMIT → 颜色白名单（缺省 COLOR_WHITE/secondary=color）→ 名自增
     *  'Flag1..' → 重名（register.flags + 本 tick 已建）→ 长度 ≤100 →
     *  本地 Game.flags 装配 + intent → 返回名。 */
    methods.createFlag = function (firstArg, secondArg, name, color, secondaryColor) {
      const xy = gw.fetchXYArguments(firstArg, secondArg);
      let x = xy[0], y = xy[1];
      if (x === undefined || y === undefined || x < 0 || x > 49 || y < 0 || y > 49) {
        return ERR_INVALID_ARGS;
      }
      if (Object.keys(gw.currentGame().flags).length >= C_FLAGS_LIMIT) {
        return C_ERR_FULL;
      }
      if (firstArg !== null && typeof firstArg === 'object') {
        secondaryColor = color;
        color = name;
        name = secondArg;
      }
      if (!color) color = C_COLOR_WHITE;
      if (!secondaryColor) secondaryColor = color;
      if (!C_COLORS_ALL.includes(color)) return ERR_INVALID_ARGS;
      if (!C_COLORS_ALL.includes(secondaryColor)) return ERR_INVALID_ARGS;
      if (!name) {
        let cnt = 1;
        do {
          name = 'Flag' + cnt;
          cnt++;
        } while (gw.currentGame().flags[name] !== undefined || gw.createdFlagNames.has(name));
      }
      if (gw.currentGame().flags[name] !== undefined || gw.createdFlagNames.has(name)) {
        return C_ERR_NAME_EXISTS;
      }
      if (name.length > 100) return ERR_INVALID_ARGS;
      gw.createdFlagNames.add(name);
      // 新旗立即加入本 tick 的 Game.flags，但不加入 objectsById；下一 tick 由世界数据重新装配。
      // 旗名遮蔽局部 name，因此房间名要从 this.name 读取。
      const roomName = this.name;
      const shell = gw.wrapObject(
        { type: 'flag', id: undefined, room: roomName, x, y, name, color, secondaryColor },
        gw.world.time,
      );
      // 新旗帜不经过 buildGame 的回填循环，必须在这里设置其 room 引用。
      shell.room = this;
      gw.currentGame().flags[name] = shell;
      gw.intents.pushByName('room', 'createFlag', {
        roomName, x, y, name, color, secondaryColor,
      });
      return name;
    };
    return methods;
  };
})();
