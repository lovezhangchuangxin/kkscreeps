/* kk-runner prelude · 30_game 段 6：工地与旗帜装配（对照官方
 * engine/src/game/construction-sites.js + flags.js）。
 *
 * A3b：ConstructionSite 字段表挂类原型（官方 construction-sites.js:38-46），
 * getter 经 gw.rawData(this.id) 回查。Flag 对照官方 flags.js:25-30——own 定格
 * 面 name/color/secondaryColor（构造器赋值，不走 defineGameObjectProperties，
 * 也没有 id——官方 Flag 构造器不挂 this.id）；memory 是独立 getter/setter
 * （flags.js:31-50）挂类原型。
 * 依赖：gw.viewerName、gw.ensureMemory、gw.rawData、gw.mkStubMethod、
 * gw.defineGameObjectProperties。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;

  function flagMemoryFor(name) {
    const m = gw.ensureMemory();
    if (!m.flags) m.flags = {};
    if (!m.flags[name]) m.flags[name] = {};
    return m.flags[name];
  }

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
  // remove 官方 construction-sites.js:50-62（A4 换真——桩保持计数语义，
  // 不加消亡回查：官方方法体经 getter 访问触发回查，桩无字段访问故不抛，
  // 逐方法对齐留给换真批次）
  globalThis.ConstructionSite.prototype.remove = gw.mkStubMethod('constructionSite.remove');

  // ---------------------------------------------------------------- Flag：own 定格 + memory
  // 官方 flags.js:25-30：构造器赋值 name/color/secondaryColor（值属性，不走
  // defineGameObjectProperties，也无 toJSON——stringify 直接枚举 own 面）。
  // 装配期定格（不回查——官方同款值属性）
  Object.defineProperty(globalThis.Flag.prototype, 'memory', {
    get() {
      return flagMemoryFor(this.name);
    },
    set(value) {
      const m = gw.ensureMemory();
      if (!m.flags) m.flags = {};
      m.flags[this.name] = value;
    },
  });

  globalThis.Flag.prototype.remove = gw.mkStubMethod('flag.remove');
  globalThis.Flag.prototype.setPosition = gw.mkStubMethod('flag.setPosition');
  globalThis.Flag.prototype.setColor = gw.mkStubMethod('flag.setColor');

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
