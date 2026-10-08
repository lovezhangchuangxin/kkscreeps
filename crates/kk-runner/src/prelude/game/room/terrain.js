/* 地形位码与安全拷出；原始数组只留在本段闭包，玩家面只能读取或复制。
 * 来源：上游 rooms.js 的 Room.Terrain 与地形 look 语义。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  // 地形数组可能锚定跨玩家共享内存；捕获真 TypedArray 构造器与 set，
  // 避免玩家替换构造器或动态派发的 .set 外泄内部数组。
  // 真 set 在进入拷贝前进行 TypedArray 品牌检查，不执行玩家代码。
  const __Uint8Array = Uint8Array;
  const __u8Set = Uint8Array.prototype.set;
  const __reflectApply = Reflect.apply;
  const TERRAIN_MASK_WALL = globalThis.TERRAIN_MASK_WALL;
  const TERRAIN_MASK_SWAMP = globalThis.TERRAIN_MASK_SWAMP;
  // ---------------------------------------------------------------- Terrain
  // 官方 rooms.js:1223-1241 Room.Terrain 逐字移植：闭包式对象（非 class），
  // 无地形数据的房间构造即抛错；get 返回位码（wall=1/swamp=2/plain=0），
  // getRawBuffer 填充给定 TypedArray或新建 Uint8Array。地形数据经世界 JSON
  // 注入（terrainByRoom，beginTick 填充）。

  function Terrain(roomName) {
    const array = gw.terrainByRoom['' + roomName];
    if (!array) throw new Error('Could not access room ' + roomName);
    this.get = function (x, y) {
      const value = array[y * 50 + x];
      return value & TERRAIN_MASK_WALL || value & TERRAIN_MASK_SWAMP || 0;
    };
    this.getRawBuffer = function (destinationArray) {
      if (destinationArray) {
        // 经捕获的真 set 拷贝：destination 须是真 TypedArray（V8 品牌
        // 校验，伪造对象在此抛错），拷贝是引擎内部 memmove——两条路径
        // 都不把内部数组（可能是共享视图）交给玩家代码。
        __reflectApply(__u8Set, destinationArray, [array]);
        return destinationArray;
      }
      return new __Uint8Array(array);
    };
  }
  // 官方 Terrain 挂法：Room 的静态属性（rooms.js:1135 `new Room.Terrain(...)`），
  // 非裸全局
  globalThis.Room.Terrain = Terrain;
  gw.Terrain = Terrain;

  // 地形单词（look 面）：swamp 先判、wall 后覆盖（rooms.js:663-671）。
  // 无地形数据的房间（玩家自造未注册房间名）自然 TypeError——与官方
  // _lookSpatialRegister 直读 staticTerrainData[id]（rooms.js:665）的行为一致；
  // 世界内全部房间恒有地形（world_json 恒发 terrain）
  function terrainWordAt(roomName, x, y) {
    const t = gw.terrainByRoom[roomName][y * 50 + x];
    let result = 'plain';
    if (t & TERRAIN_MASK_SWAMP) result = 'swamp';
    if (t & TERRAIN_MASK_WALL) result = 'wall';
    return result;
  }
  gw.terrainWordAt = terrainWordAt;
})();
