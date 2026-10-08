/* 装配 ConstructionSite 与 Flag，并将移除、移动和颜色修改记录为房间指令。
 * 工地字段通过 getter 回查当前 tick；旗帜名称、颜色和位置在装配时保存。
 * Flag 没有 id，memory 独立读写 Memory.flags；工地移除需要本人或房主权限。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;
  // P2 增删改的返回码/色表（官方裸全局同源）
  const C_ERR_NOT_OWNER = globalThis.ERR_NOT_OWNER;
  const C_ERR_INVALID_TARGET = globalThis.ERR_INVALID_TARGET;
  const C_ERR_INVALID_ARGS = globalThis.ERR_INVALID_ARGS;
  const COLORS_ALL = globalThis.COLORS_ALL;

  // ---------------------------------------------------------------- 工地：原型字段表
  // 官方 construction-sites.js:38-46（我方投影字段直存；name/owner 缺省语义
  // 保持现状——匿名工地 name undefined）
  gw.defineGameObjectProperties(globalThis.ConstructionSite.prototype, gw.rawData, {
    structureType: (o) => o.structureType,
    progress: (o) => o.progress || 0,
    progressTotal: (o) => o.progressTotal || 100,
    name: (o) => o.name,
    owner: (o) => (o.owner !== undefined ? { username: o.owner } : undefined),
    my: (o) => (o.owner === undefined ? undefined : o.owner === gw.viewerName),
  });
  // 非本人工地且房主也非本人时拒绝移除；有效请求记录房间 removeConstructionSite 指令。
  // 来源：上游 engine/src/game/construction-sites.js 的 remove。
  globalThis.ConstructionSite.prototype.remove = function remove() {
    // 官方 :56-58 逐字：!my（含 undefined——无主工地同拒）且房主非本人
    if (this.my !== true && !(this.room && this.room.controller && this.room.controller.my)) {
      return C_ERR_NOT_OWNER;
    }
    gw.intents.pushByName('room', 'removeConstructionSite', {
      roomName: gw.rawData(this.id).room,
      id: this.id,
    });
    return 0;
  };

  // ---------------------------------------------------------------- Flag：own 定格 + memory
  // 官方 flags.js:25-30：构造器赋值 name/color/secondaryColor（值属性，不走
  // defineGameObjectProperties，也无 toJSON——stringify 直接枚举 own 面）。
  // 装配期定格（不回查——官方同款值属性）
  // 官方 flags.js:31-53：memory 为 getter/setter（gw.memoryFor 官方同形——
  // 含 'undefined' 病态检查与非对象分支）
  Object.defineProperty(globalThis.Flag.prototype, 'memory', {
    get() {
      return gw.memoryFor('flags', this.name);
    },
    set(value) {
      gw.setMemoryFor('flags', this.name, value, 'flag');
    },
  });

  // remove 记录 removeFlag；移动或改色组合 removeFlag+createFlag，结算先移除再创建。
  // 来源：上游 engine/src/game/flags.js 的三个修改方法。
  globalThis.Flag.prototype.remove = function remove() {
    gw.intents.pushByName('room', 'removeFlag', {
      roomName: this.pos.roomName,
      name: this.name,
    });
    return 0;
  };
  // 官方 flags.js:63-75：仅 undefined 拦截、无越界预检（勘正：原注释称
  // 越界折 ERR_INVALID_TARGET 不实——fetchXYArguments 不查边界）——越界
  // 坐标 intent 照记，结算 createFlag 0..49 门静默丢 → removeFlag 已生效=
  // 旗净消亡，官方同形
  globalThis.Flag.prototype.setPosition = function setPosition(firstArg, secondArg) {
    const xy = gw.fetchXYArguments(firstArg, secondArg);
    const x = xy[0];
    const y = xy[1];
    const roomName = xy[2] || this.pos.roomName;
    if (x === undefined || y === undefined) {
      return C_ERR_INVALID_TARGET;
    }
    gw.intents.pushByName('room', 'removeFlag', {
      roomName: this.pos.roomName,
      name: this.name,
    });
    gw.intents.pushByName('room', 'createFlag', {
      roomName,
      x,
      y,
      name: this.name,
      color: this.color,
      secondaryColor: this.secondaryColor,
    });
    return 0;
  };
  // 官方 flags.js:76-90：色白名单（secondary 缺省=color）→ removeFlag+
  // createFlag 同位重建
  globalThis.Flag.prototype.setColor = function setColor(color, secondaryColor) {
    if (!COLORS_ALL.includes(color)) {
      return C_ERR_INVALID_ARGS;
    }
    secondaryColor = secondaryColor || color;
    if (!COLORS_ALL.includes(secondaryColor)) {
      return C_ERR_INVALID_ARGS;
    }
    gw.intents.pushByName('room', 'removeFlag', {
      roomName: this.pos.roomName,
      name: this.name,
    });
    gw.intents.pushByName('room', 'createFlag', {
      roomName: this.pos.roomName,
      x: this.pos.x,
      y: this.pos.y,
      name: this.name,
      color,
      secondaryColor,
    });
    return 0;
  };

  // ---------------------------------------------------------------- 装配
  gw.assemblers.constructionSite = (o, base) => base;

  // flag：对照官方构造器 own 面（name/color/secondaryColor 定格 + 无 id——
  // 官方 Flag 构造器不挂 this.id，flags.js:25-30；objectsById 索引用投影 id，
  // 摘除壳上的 id 不影响 Map 持有的引用）
  gw.assemblers.flag = (o, base, pos) => {
    delete base.id;
    base.name = o.name;
    base.color = o.color || 10;
    base.secondaryColor = o.secondaryColor || o.color || 10;
    base.pos = pos;
    return base;
  };
})();
