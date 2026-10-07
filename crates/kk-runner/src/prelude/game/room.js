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

  function makeRoom(name, r) {
    const queries = makeRoomQueries(name);
    const { controller, storageObj, terminalObj, roomObjs } = queries;
    // 官方 Room 构造器/Structure 构造器（rooms.js:422-437、structures.js:41-56）
    // 的挂载面：name/energyAvailable/energyCapacityAvailable/survivalInfo/visual
    // 恒为自有属性（survivalInfo 普通房恒 undefined），controller/storage/
    // terminal 仅在对应结构存在时成为自有属性——缺失时键不存在（读 undefined），
    // 数据属性表面与官方一致。官方无 mode 字段。
    const room = {
      name,
      // 官方 game.js:325-328：非 off 的 spawn/extension 计入（spawn 300 固定、
      // extension 按 RCL 表）——由 Rust 侧 world_json 房间投影携带
      energyAvailable: r.energyAvailable ?? 0,
      energyCapacityAvailable: r.energyCapacityAvailable ?? 300,
      survivalInfo: undefined,
      visual: new RoomVisual(name),
    };
    if (controller !== undefined) room.controller = controller;
    if (storageObj !== undefined) room.storage = storageObj;
    if (terminalObj !== undefined) room.terminal = terminalObj;
    queries.install(room);
    room.getTerrain = function () { return new Terrain(name); };
    const creation = makeRoomCreation(name, roomObjs);
    /** 官方 rooms.js:540-549 toJSON：for-in 枚举（含原型可枚举法——stringify
     *  时函数值自然丢弃）+ 跳过 _ 前缀与五键。 */
    room.toJSON = function () {
      const result = {};
      for (const i in this) {
        if (i[0] === '_' || i === 'toJSON' || i === 'toString'
          || i === 'controller' || i === 'storage' || i === 'terminal') {
          continue;
        }
        result[i] = this[i];
      }
      return result;
    };
    room.findPath = makeRoomFindPath(name);
    room.createConstructionSite = creation.createConstructionSite;
    // 装配时分别保存事件 JSON 字符串和解析数组；修改解析数组不会反向改变 raw 返回值。
    // 同 tick 的非 raw 调用复用解析数组。来源：上游 engine/src/game/rooms.js 的 getEventLog。
    const eventLogRaw = __jsonStringify((r && r.events) || []);
    const eventLogParsed = __jsonParse(eventLogRaw);
    room.getEventLog = function (raw) {
      return raw ? eventLogRaw : eventLogParsed;
    };
    room.createFlag = creation.createFlag;
    // 官方 rooms.js:1128-1131：map.findExit 转发（无 opts 参——两参形态）
    room.findExitTo = function (target) {
      return gw.currentGame().map.findExit(name, target);
    };
    // 官方 rooms.js:971-976：越界坐标返 null（构造器抛错前拦截——不依赖
    // 'Invalid arguments' 通道）
    room.getPositionAt = (x, y) => {
      if (x < 0 || x > 49 || y < 0 || y > 49) return null;
      return new gw.RoomPosition(x, y, name);
    };
    // 官方 rooms.js:551-571：memory 为 getter/setter（gw.memoryFor 官方同形——
    // 含 'undefined' 病态检查与非对象分支），tooangel 等 bot 直接对 room.memory 赋值
    __Object.defineProperty(room, 'memory', {
      configurable: true,
      get() {
        return gw.memoryFor('rooms', name);
      },
      set(v) {
        gw.setMemoryFor('rooms', name, v, 'room');
      },
    });
    if (__RoomCtor && __RoomCtor.prototype) {
      __Object.setPrototypeOf(room, __RoomCtor.prototype);
    }
    const result = probe(room, 'Room[' + name + ']');
    trackRoomPathfinding(result, roomObjs);
    return result;
  }
  gw.makeRoom = makeRoom;
})();
