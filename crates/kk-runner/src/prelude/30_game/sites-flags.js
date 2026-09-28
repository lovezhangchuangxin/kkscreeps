/* kk-runner prelude · 30_game 段 6：工地与旗帜装配（对照官方
 * engine/src/game/construction-sites.js + flags.js）。
 *
 * 依赖：gw.viewerName、gw.ensureMemory（flag.memory 懒 getter）、gw.mkStubMethod。
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

  // ---------------------------------------------------------------- 工地装配
  gw.assemblers.constructionSite = (o, base) => {
    const obj = Object.assign(base, {
      structureType: o.structureType,
      progress: o.progress || 0,
      progressTotal: o.progressTotal || 100,
      my: o.owner === gw.viewerName,
    });
    obj.remove = gw.mkStubMethod('constructionSite.remove');
    return obj;
  };

  // ---------------------------------------------------------------- 旗帜装配
  gw.assemblers.flag = (o, base, pos) => {
    const obj = {
      name: o.name,
      color: o.color || 10,
      secondaryColor: o.secondaryColor || 10,
      pos,
    };
    // 官方 flags.js:31：memory 是懒 getter（赋值也允许——补 setter）
    Object.defineProperty(obj, 'memory', {
      configurable: true,
      get: () => flagMemoryFor(o.name),
      set: (v) => { const m = gw.ensureMemory(); if (!m.flags) m.flags = {}; m.flags[o.name] = v; },
    });
    obj.remove = gw.mkStubMethod('flag.remove');
    obj.setPosition = gw.mkStubMethod('flag.setPosition');
    obj.setColor = gw.mkStubMethod('flag.setColor');
    return obj;
  };
})();
