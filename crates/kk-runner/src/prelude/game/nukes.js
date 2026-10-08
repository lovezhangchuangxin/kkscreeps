/* 飞行核弹的读面：剩余落地时间与发射房名。
 * 来源：上游 engine/src/game/nukes.js:36-37（timeToLand = landTime - time）。
 */
(() => {
  'use strict';

  const gw = globalThis.__kkp.gw;

  gw.defineGameObjectProperties(globalThis.Nuke.prototype, gw.rawData, {
    timeToLand: (o) => o.landTime - gw.world.time,
    launchRoomName: (o) => o.launchRoomName,
  });
})();
