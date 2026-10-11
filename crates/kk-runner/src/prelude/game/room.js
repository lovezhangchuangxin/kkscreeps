/* 根据当前世界数据装配可见 Room，提供 find、look、地形、创建指令与绘图接口。
 * Room 实例跨 tick 常驻（同一性保持与 wrapper 常驻同属已接受观察面）：
 * 信封字段（energy/visual/eventLog）逐拍刷新，查询索引由 game.js 按房差分
 * 整建或格位迁移（见 room/queries.js）；地形数组由 runtime/world-tick.js
 * 的 beginTick 注入。findPath 走带房间代价矩阵的原生搜索；绘图数据仅保留
 * 在本 tick 的缓冲区。来源：上游 engine/src/game/rooms.js 与 console.js
 * 的玩家接口。 */
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
  const __mapGet = Map.prototype.get;
  const __mapSetR = Map.prototype.set;
  const __reflectApply = Reflect.apply;
  const contexts = new WeakMap();
  // 常驻 Room 表：房名 → entry。entry 持原始实例与探测代理、查询控制器、
  // 方法表与 eventLog 缓存；methods.getEventLog 经 entry 读当前拍缓存。
  const residentRooms = new Map();

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

  /** 查常驻 entry（无则 undefined）——装配主环值变动行格位迁移用。 */
  gw.roomEntryOf = function (name) {
    return __reflectApply(__mapGet, residentRooms, [name]);
  };

  /** 取或建常驻 Room entry。创建时装原型/方法表/查询上下文一次；查询
   * 索引内容与结构引用由 game.js 的房间差分驱动（rebuild 后调
   * remountStructRefs）。bucket.os 身份由装配层保持恒定，创建预检闭包
   * 捕获的数组跨拍有效。 */
  gw.ensureRoomEntry = function (name, bucket) {
    let entry = __reflectApply(__mapGet, residentRooms, [name]);
    if (entry !== undefined) return entry;
    const queries = makeRoomQueries(name);
    queries.rebuild(bucket);
    // name/energyAvailable/energyCapacityAvailable/survivalInfo/visual 恒为
    // 自有属性；controller/storage/terminal 只在对应结构存在时挂载。
    // 来源：官方 Room 构造器与 Structure 构造器的挂载面。
    const room = {
      name,
      energyAvailable: 0,
      energyCapacityAvailable: 300,
      survivalInfo: undefined,
      visual: new RoomVisual(name),
    };
    const methods = __Object.create(null);
    queries.install(methods);
    const creation = makeRoomCreation(name, queries.roomObjs);
    methods.findPath = makeRoomFindPath(name);
    methods.createConstructionSite = creation.createConstructionSite;
    methods.createFlag = creation.createFlag;
    entry = { room, queries, methods, eventLogRaw: '[]', eventLogParsed: [] };
    // raw 与解析数组各自保存；同 tick 复用解析数组，修改它不改变 raw。
    // 闭包经 entry 读当前拍缓存（refreshRoom 按需更新）。
    methods.getEventLog = function (raw) {
      return raw ? entry.eventLogRaw : entry.eventLogParsed;
    };
    __Object.setPrototypeOf(room, __RoomCtor.prototype);
    const result = probe(room, 'Room[' + name + ']');
    entry.result = result;
    __reflectApply(__weakSet, contexts, [room, methods]);
    __reflectApply(__weakSet, contexts, [result, methods]);
    __reflectApply(__mapSetR, residentRooms, [name, entry]);
    return entry;
  };

  /** 逐拍信封刷新：energy/visual/eventLog/出口缓存/寻路登记。恒做（值
   * 本身来自每拍新解析的信封）。eventLog 每拍新 parse：parsed 数组以内部
   * 引用交出（拍内跨调用共享、玩家可原地改），跨拍复用会让篡改存活，
   * 官方面每拍新实例。寻路 grids 缓存逐拍重建语义由重新登记保持（官方
   * 面 Room 实例每拍新建，缓存随实例消亡）。 */
  gw.refreshRoom = function (name, entry, r) {
    const room = entry.room;
    const avail = r === undefined || r === null ? undefined : r.energyAvailable;
    const capacity = r === undefined || r === null ? undefined : r.energyCapacityAvailable;
    room.energyAvailable = avail ?? 0;
    room.energyCapacityAvailable = capacity ?? 300;
    room.visual = new RoomVisual(name);
    const raw = __jsonStringify((r && r.events) || []);
    entry.eventLogRaw = raw;
    entry.eventLogParsed = __jsonParse(raw);
    entry.queries.resetFindCache();
    trackRoomPathfinding(entry.result, entry.queries.roomObjs);
    return entry;
  };

  /** 查询整建后重挂结构引用：controller/storage/terminal 按在否挂载或
   * 摘除（官方对应字段只在结构存在时是自有属性）。 */
  gw.remountStructRefs = function (entry) {
    const queries = entry.queries;
    const room = entry.room;
    if (queries.controller !== undefined) room.controller = queries.controller;
    else delete room.controller;
    if (queries.storageObj !== undefined) room.storage = queries.storageObj;
    else delete room.storage;
    if (queries.terminalObj !== undefined) room.terminal = queries.terminalObj;
    else delete room.terminal;
  };
})();
