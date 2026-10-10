/* 根据当前世界数据装配可见 Room，提供 find、look、地形、创建指令与绘图接口。
 * 每 tick 重建房间对象和查询缓存；地形数组由 runtime/world-tick.js 的 beginTick 注入。
 * findPath 走带房间代价矩阵的原生搜索；绘图数据仅保留在本 tick 的缓冲区。
 * 来源：上游 engine/src/game/rooms.js 与 console.js 的玩家接口。
 */
(() => {
  'use strict';

  const { probe } = globalThis.__kkp;
  const gw = globalThis.__kkp.gw;
  const __Object = Object;
  const __RoomCtor = globalThis.Room;
  const __jsonStringify = JSON.stringify;
  const __jsonParse = JSON.parse;
  const Terrain = gw.Terrain;
  const RoomVisual = globalThis.RoomVisual;
  // 装配期捕获工厂；宿主装配收尾 删除临时 __kkp 后仍通过闭包调用。
  const makeRoomQueries = gw.makeRoomQueries;
  const makeRoomCreation = gw.makeRoomCreation;
  const makeRoomFindPath = gw.makeRoomFindPath;
  const trackRoomPathfinding = gw.trackRoomPathfinding;

  const __weakGet = WeakMap.prototype.get;
  const __weakSet = WeakMap.prototype.set;
  const __reflectApply = Reflect.apply;
  const contexts = new WeakMap();

  // 公共方法只安装一次，玩家包装原型后不会被下一 tick 的装配覆盖。
  // 单房索引/缓存留在私有上下文；同时登记原对象与探测代理，允许借用
  // 原型方法的 call/apply 按接收者选择房间，而不捕获首次房间。
  // 来源：上游 engine/src/game/rooms.js 的 Room.prototype 方法与 privateStore。
  for (const method of [
    'find',
    'lookAt',
    'lookForAt',
    'lookAtArea',
    'lookForAtArea',
    'findPath',
    'createConstructionSite',
    'createFlag',
    'getEventLog',
  ]) {
    __RoomCtor.prototype[method] = function (...args) {
      const methods = __reflectApply(__weakGet, contexts, [this]);
      return __reflectApply(methods[method], this, args);
    };
  }
  __RoomCtor.prototype.getTerrain = function () {
    return new Terrain(this.name);
  };
  /** 官方 toJSON 枚举原型可枚举成员，排除私有键和结构引用；函数值由
   * stringify 自然丢弃。来源：rooms.js 的 Room.prototype.toJSON。 */
  __RoomCtor.prototype.toJSON = function () {
    const result = {};
    for (const i in this) {
      if (
        i[0] === '_' ||
        i === 'toJSON' ||
        i === 'toString' ||
        i === 'controller' ||
        i === 'storage' ||
        i === 'terminal'
      )
        continue;
      result[i] = this[i];
    }
    return result;
  };
  __RoomCtor.prototype.findExitTo = function (target) {
    return gw.currentGame().map.findExit(this.name, target);
  };
  __RoomCtor.prototype.getPositionAt = function (x, y) {
    if (x < 0 || x > 49 || y < 0 || y > 49) return null;
    return new gw.RoomPosition(x, y, this.name);
  };
  // memory 沿用每次按房名读取的契约，不保存房间对象的局部副本。
  __Object.defineProperty(__RoomCtor.prototype, 'memory', {
    configurable: true,
    get() {
      return gw.memoryFor('rooms', this.name);
    },
    set(v) {
      gw.setMemoryFor('rooms', this.name, v, 'room');
    },
  });

  function makeRoom(name, r, bucket) {
    const queries = makeRoomQueries(name, bucket);
    const { controller, storageObj, terminalObj, roomObjs } = queries;
    // name/energyAvailable/energyCapacityAvailable/survivalInfo/visual 恒为
    // 自有属性；controller/storage/terminal 只在对应结构存在时挂载。
    // 来源：官方 Room 构造器与 Structure 构造器的挂载面。
    const room = {
      name,
      energyAvailable: r.energyAvailable ?? 0,
      energyCapacityAvailable: r.energyCapacityAvailable ?? 300,
      survivalInfo: undefined,
      visual: new RoomVisual(name),
    };
    if (controller !== undefined) room.controller = controller;
    if (storageObj !== undefined) room.storage = storageObj;
    if (terminalObj !== undefined) room.terminal = terminalObj;
    const methods = __Object.create(null);
    queries.install(methods);
    const creation = makeRoomCreation(name, roomObjs);
    methods.findPath = makeRoomFindPath(name);
    methods.createConstructionSite = creation.createConstructionSite;
    methods.createFlag = creation.createFlag;
    // raw 与解析数组各自保存；同 tick 复用解析数组，修改它不改变 raw。
    const eventLogRaw = __jsonStringify((r && r.events) || []);
    const eventLogParsed = __jsonParse(eventLogRaw);
    methods.getEventLog = function (raw) {
      return raw ? eventLogRaw : eventLogParsed;
    };
    __Object.setPrototypeOf(room, __RoomCtor.prototype);
    const result = probe(room, 'Room[' + name + ']');
    __reflectApply(__weakSet, contexts, [room, methods]);
    __reflectApply(__weakSet, contexts, [result, methods]);
    trackRoomPathfinding(result, roomObjs);
    return result;
  }
  gw.makeRoom = makeRoom;
})();
