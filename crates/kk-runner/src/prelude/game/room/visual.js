/* 绘图缓冲按房间名共享，每 tick 清空；Room 和 Game.map 共用此状态。
 * 来源：上游 rooms.js 的 RoomVisual 与 console.js 的 addVisual。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const __Object = Object;
  const __jsonStringify = JSON.stringify;
  // 绘图字符串按房间名共享：同房间多个 RoomVisual 实例操作同一缓冲区。
  // 房间限制 500×1024、map 限制 1000×1024 个 UTF-16 码元，超限抛错；
  // beginTick 清空全部缓冲区。来源：上游 engine/src/game/console.js 的 addVisual。
  const visualStore = __Object.create(null);
  gw.addVisual = function addVisual(roomName, data) {
    roomName = roomName || '';
    if (!data) {
      return;
    }
    const sizeLimit = roomName === 'map' ? 1000 : 500;
    const dataString = typeof data === 'string' ? data : __jsonStringify(data) + '\n';
    if ((visualStore[roomName] || '').length + dataString.length > sizeLimit * 1024) {
      if (roomName === 'map') {
        throw new Error('MapVisual size has exceeded ' + sizeLimit + ' KB limit');
      }
      throw new Error(
        'RoomVisual size in room ' + roomName + ' has exceeded ' + sizeLimit + ' KB limit',
      );
    }
    visualStore[roomName] = (visualStore[roomName] || '') + dataString;
  };
  gw.getVisualSize = function getVisualSize(roomName) {
    roomName = roomName || '';
    return visualStore[roomName] ? visualStore[roomName].length : 0;
  };
  gw.clearVisual = function clearVisual(roomName) {
    visualStore[roomName || ''] = '';
  };
  gw.getVisual = function getVisual(roomName) {
    return visualStore[roomName || ''];
  };
  gw.visualReset = function visualReset() {
    for (const k in visualStore) delete visualStore[k];
  };

  // 官方 RoomVisual（rooms.js:1144-1218）逐字：六绘制方法（坐标/位置双形）+
  // getSize/clear/export/import，全部经 console.addVisual 族路由（我方 gw 桥）。
  class RoomVisual {
    constructor(roomName) {
      this.roomName = roomName;
    }
    circle(x, y, style) {
      if (typeof x === 'object') {
        style = y;
        y = x.y;
        x = x.x;
      }
      gw.addVisual(this.roomName, { t: 'c', x, y, s: style });
      return this;
    }
    line(x1, y1, x2, y2, style) {
      if (typeof x1 === 'object' && typeof y1 === 'object') {
        style = x2;
        x2 = y1.x;
        y2 = y1.y;
        y1 = x1.y;
        x1 = x1.x;
      }
      gw.addVisual(this.roomName, { t: 'l', x1, y1, x2, y2, s: style });
      return this;
    }
    rect(x, y, w, h, style) {
      if (typeof x === 'object') {
        style = h;
        h = w;
        w = y;
        y = x.y;
        x = x.x;
      }
      gw.addVisual(this.roomName, { t: 'r', x, y, w, h, s: style });
      return this;
    }
    poly(points, style) {
      if (Array.isArray(points) && points.some(Boolean)) {
        points = points.map((i) => (i.x !== undefined ? [i.x, i.y] : i));
        gw.addVisual(this.roomName, { t: 'p', points, s: style });
      }
      return this;
    }
    text(text, x, y, style) {
      if (typeof x === 'object') {
        style = y;
        y = x.y;
        x = x.x;
      }
      gw.addVisual(this.roomName, { t: 't', text, x, y, s: style });
      return this;
    }
    getSize() {
      return gw.getVisualSize(this.roomName);
    }
    clear() {
      gw.clearVisual(this.roomName);
      return this;
    }
    export() {
      return gw.getVisual(this.roomName);
    }
    import(data) {
      gw.addVisual(this.roomName, '' + data);
      return this;
    }
  }
  // RoomVisual 全局绑定不可写。来源：上游 engine/src/game/rooms.js 的 RoomVisual 导出。
  Object.defineProperty(globalThis, 'RoomVisual', {
    enumerable: true,
    value: RoomVisual,
  });
})();
