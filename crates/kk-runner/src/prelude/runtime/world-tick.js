/* 双通道世界注入与 tick 初始化；保持索引、Memory/CJS、地形、API 重置的先后顺序。 */
(() => {
  'use strict';
  const gw = globalThis.__kkp.gw;
  const { memory } = globalThis.__kkp;
  const { ensureRequire, setCurrentUser } = globalThis.__kkp.cjs;
  const { resetIntents, intentsPayload } = globalThis.__kkp.intentCollector;
  const __jsonParse = JSON.parse;
  const __Object = Object;
  const __Array = Array;
  const __Map = Map;
  const __Set = Set;
  const __mapSet = Map.prototype.set;
  const __reflectApply = Reflect.apply;
  const __objectEntries = Object.entries;

  // refreshGame 由装配入口一次性传入；玩家执行时不回查已解绑的 __kkp。
  function createWorldTick(refreshGame) {
    return {
      /** 设置闭包 rawMemoryData，供下一次 beginTick 创建 RawMemory 时使用。
       * 不调用玩家可替换的全局 RawMemory.set，须在 beginTick 前完成。 */
      setRawMemoryHost: memory.setRawMemoryHost,
      /** 读取当前 RawMemory 数据作为待提交候选；真正保存到 PlayerState 由 Rust 协调器完成。 */
      rawMemoryDataHost: memory.rawMemoryDataHost,
      /** InterShardMemory 本地段注入（PlayerState.ism_local，null=未设置）。 */
      setIsmHost: memory.setIsmHost,
      /** 当前 ISM 本地段与脏标记；endTick 仅在 setLocal 触发时带出候选。 */
      ismLocalHost: memory.ismLocalHost,
      ismDirtyHost: memory.ismDirtyHost,
      /** 设置活动 segment 窗口，下一次 beginTick 将内容复制到新的 RawMemory.segments。 */
      setSegmentsWindowHost: memory.setSegmentsWindowHost,
      /** 宿主私有通道（P6）：注入本 tick 的 foreignSegment（官方
       *  data.foreignMemorySegment——null 串表示无）。 */
      setForeignSegmentHost: memory.setForeignSegmentHost,
      /** 宿主私有通道（P6）：tick 尾 segments 信封（writeBackMemory 成功打包的
       *  写回+四请求快照；endTick 随 rawMemory 一并带出——失败轮为 null）。 */
      segmentsOutHost: memory.segmentsOutHost,
      /** 每 tick 注入世界快照（宿主经私有句柄以字符串参数传入——下方双分支
       * 兼容 string/对象两种形态）并重建 Game。 */
      beginTick(worldJson, cpuMeta) {
        beginTickImpl(worldJson, cpuMeta, null, null);
      },
      /** KKPV 通道注入：worldJson 是不含对象数组与地形的世界信封；对象记录
       * 由可信解码器从私有块束（ArrayBuffer）解出，地形由公开地形束解出
       * 零拷贝视图。装配后与 JSON 通道的等同面：gw.world.objects 的成员、
       * 数组顺序（块房序×ID 序）、字段值与缺失形态、Store/嵌套记录的键序
       * （载荷保序）、terrainByRoom 的逐格地形值；两者共用同一段
       * beginTickImpl 后续步骤。两个束须同时提供。 */
      beginTickKkpv(worldJson, bundle, publicBundle, cpuMeta) {
        beginTickImpl(worldJson, cpuMeta, bundle, publicBundle);
      },
      writeBackMemory: memory.writeBackMemory,
      intentsPayload,
    };

    /** beginTick/beginTickKkpv 的共用实现；bundle 非 null 时对象数组来自
     * KKPV 私有块束（信封不含 objects 键），publicBundle 非 null 时地形
     * 来自公开地形束的零拷贝视图（信封不含 terrain 键），其余步骤两条
     * 通道完全一致。两个束必须同时提供或同时为空——单侧出现即装配错误。 */
    function beginTickImpl(worldJson, cpuMeta, bundle, publicBundle) {
      const hasPrivate = bundle !== null && bundle !== undefined;
      const hasPublic = publicBundle !== null && publicBundle !== undefined;
      if (hasPrivate !== hasPublic) {
        throw new Error('KKPV 通道的公开束与私有束须同时提供或同时为空');
      }
      // 使用捕获的 JSON.parse 解码，避免玩家替换全局函数后改写世界与用户身份。
      gw.world = typeof worldJson === 'string' ? __jsonParse(worldJson) : worldJson;
      let terrainViews = null;
      if (hasPrivate) {
        // 块头 tick 与信封 time 在解码器内交叉校验（同一投影的同界快照）。
        gw.world.objects = gw.kkpv.decodeBundle(bundle, gw.world.time);
        terrainViews = gw.kkpv.decodePublicBundle(publicBundle, gw.world.time);
      }
      // Game.cpu 真值合并（宿主 begin_tick 第二参数——官方 make.js 先算窗口
      // 再装 runtimeData 的等价物；world.cpu 占位基准被此覆盖，键恒权威）
      if (cpuMeta !== undefined && cpuMeta !== null) {
        gw.world.cpu = typeof cpuMeta === 'string' ? __jsonParse(cpuMeta) : cpuMeta;
      }
      // 注入侧剥顶层原型（官方 runtime.js:56 `systemFunctions.Object.setPrototypeOf
      //(data, null)` 同位）：顶层缺失键（cpu/user 兜底读）不被继承 getter 命中；
      // 内层对象官方同样不剥——缺失键走原型链的暴露面官方一致，不深剥
      __Object.setPrototypeOf(gw.world, null);
      gw.viewerName = gw.world.user ?? null;
      // 先建立当前 tick 的原始对象索引，再装配 Game；装配期间的 getter 已会回查该索引。
      gw.rawById = new __Map();
      const tickObjs = gw.world.objects || [];
      for (let i = 0; i < tickObjs.length; i++) {
        const o = tickObjs[i];
        if (o && o.id !== undefined) __reflectApply(__mapSet, gw.rawById, [o.id, o]);
      }
      resetIntents(); // 官方每 tick 新建收集器（runtime.js _start 每 tick 执行）
      // 重置安全模式最后请求与本 tick 占领计数，避免上一 tick 的状态影响本轮预检。
      gw.lastActivateSafeMode = null;
      gw.controllersClaimedInTick = 0;
      // 重置本 tick 的旗帜名、spawn 名和工地计数；世界查询快照尚不包含新建指令。
      gw.createdFlagNames = new __Set();
      gw.createdConstructionSites = 0;
      gw.createdSpawnNames = new __Set();
      // 官方每 tick init（runtime.js:471-484 / make.js:160 等价物）：RawMemory
      // **重建**（官方 rawMemory 是 _start 局部变量每 tick 新建——_parsed 单
      // tick 生命周期；玩家可删可换全局绑定，每 tick 恢复）、Memory 重装懒
      // getter（上一 tick 首访后已固化为值属性，configurable 允许覆盖）。
      // 解析缓存只存当 tick RawMemory._parsed（set/delete 均失效它），无闭包旁路
      memory.resetRawMemoryHost();
      // 官方 game.js:504-512：require 每 tick 重绑四条件判定（缺失/timestamp
      // 变/main 非对象/main.loop 非函数 → 整体重绑）；moduleObject.user 注入
      ensureRequire();
      setCurrentUser(gw.viewerName);
      memory.installMemoryHost();
      gw.terrainByRoom = {};
      // 新 tick 开始时丢弃上一 tick 的绘图缓冲区。
      if (gw.visualReset) gw.visualReset();
      // 地形表一律经 defineProperty 填自有属性：房名形如 "__proto__" 时
      // 普通赋值会改写原型而不是建键（两条通道同一保护；引擎房名不可
      // 产生该形态，防御面针对手搓世界注入）。
      const setTerrain = (room, bytes) => {
        __Object.defineProperty(gw.terrainByRoom, room, {
          value: bytes,
          writable: true,
          enumerable: true,
          configurable: true,
        });
      };
      if (terrainViews !== null) {
        // KKPV 通道：公开束的零拷贝视图直接作为地形源——无 b64 解码、
        // 无逐 tick 重分配（视图锚定共享内存，旧代际随视图失活回收）。
        // rev 缓存路径不参与：束每代际整份更换，不存在同 rev 复用问题。
        for (const room in terrainViews) {
          setTerrain(room, terrainViews[room]);
        }
      } else {
        const terrainEntries = __objectEntries(gw.world.rooms || {});
        for (let i = 0; i < terrainEntries.length; i++) {
          const roomName = terrainEntries[i][0];
          const t = terrainEntries[i][1].terrain;
          if (t && typeof t === 'object' && typeof t.payload === 'string') {
            // 紧凑协议（P3 真路径）：rev 命中跨 tick 复用解码产物（地形首玩家
            // tick 后不可变，稳态零解码）；未命中解码 b64 → 2500 字节索引兼容
            const cached = gw.terrainCache[roomName];
            if (!cached || cached.rev !== t.rev) {
              gw.terrainCache[roomName] = { rev: t.rev, bytes: gw.b64ToBytes(t.payload) };
            }
            setTerrain(roomName, gw.terrainCache[roomName].bytes);
          } else if (__Array.isArray(t)) {
            // 旧数组形态（宿主手搓注入的夹具路径——bot_check 桩世界等；真
            // 路径已不产出），原样接收
            setTerrain(roomName, t);
          }
        }
      }
      // 调用各 API 注册的 tick 重置函数，例如本 tick 已使用的 creep 名。
      // 使用索引遍历，避免动态调用玩家可替换的数组迭代器。
      if (gw.perTickReset) {
        const fns = gw.perTickReset;
        for (let i = 0; i < fns.length; i++) fns[i]();
      }
      refreshGame();
    }
  }
  globalThis.__kkp.createWorldTick = createWorldTick;
})();
