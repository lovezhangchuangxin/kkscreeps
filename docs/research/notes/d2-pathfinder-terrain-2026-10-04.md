# D2 寻路地形直通：实现与验证记录（2026-10-04）

[设计要求](../../designs/pathfinder-terrain.md)｜[批次登记](../../plans/mod-plane-batches.md)。
本文记录实现落点、性能归因（含一次 TLS 物理内存回归的定位与修复）与验收证据。

## 实现落点

- `crates/kk-runner/src/pathfinder.rs`（新）：`TerrainPack`（行序 2500 →
  kk-pathfinder 列序 625，经 `pack_terrain` 单点转换）、JSON 通道
  `from_world_json`（`{rev,payload}` b64 与宿主夹具裸数组双形态）、KKPV
  通道 `from_public_bundle`（束框架自走 + `KkpvView` 逐块校验）、进程级
  `LOADED` 内容比对（差异重载，覆盖 set_terrain 与跨分片陈旧）、全墙环
  （注册房 8 邻域减注册房）、`op_kk_pathfinder_search`（#[serde] 参数、
  错误透传官方文案）。
- `sandbox.rs`：`begin_tick_plan` 注入 `TickTerrain`（JSON 通道按 terrain
  rev 缓存打包产物稳态零解码；KKPV 通道持束句柄首次搜索懒解码）；SkipPenalty
  分支不注入（无玩家代码执行，下一 Run tick 整体替换）。
- prelude：`path-finder.js` 真搜索（driver 逐字清洗/坐标转换/错误文案；
  roomCallback 忽略归 D3）；`room.js` findPath = `_findPath2` 移植（range 1、
  跨房 maxOps 20000、plainCost 2/swampCost 10、终点回填、房界截断）；
  `room-position.js` findPathTo/findClosestByPath（`_findClosestByPath2`
  range 1 多目标 + maxRooms 1 + isNearTo 反查）；`creep.js` moveTo 全流程
  （`_move` 记忆缓存消费/写回、noPathFinding 出口、visual.poly、首步 move）。

## TLS 物理内存回归的定位与修复（本轮主要性能工作）

**症状**：perf A/B 双样显示 D2 二进制 RSS +40%（101MB→142MB），各时间相
全同。逐层二分（op 注册/体内逐行/执行与否）后定位：**只要链接入
`kk_pathfinder::search` 的代码路径（未执行亦可），进程 RSS 即 +35MB**。

**取证**（macOS `MallocStackLogging=1` + `malloc_history`，V8 平台工作
线程的分配调用树）：

```
3 (13.0M) v8::platform::DefaultWorkerThreadsTaskRunner::WorkerThread::Run
  → absl::NumCPUs() → CallOnceImpl → CurrentThreadIdentityIfPresent
  → _tlv_get_addr → dyld::instantiateVariable → malloc 13.0M
```

**根因**：`kk_ffi.cc` 的 `acquire_path_finder` 用值形态
`static thread_local std::array<path_finder_t, 2> pool`（实测
sizeof(path_finder_t)=2.16MB、池 4.32MB）。值形态 TLS 由 dyld 按线程实例化，
**进程内所有线程**（含从不执行搜索的 3 个 V8 平台工作线程）在线程启动路径
上触碰 TLS 时把整块记入物理内存（malloc_history 实测每线程 13MB 级单次
分配）。链接与否决定该 TLS 变量是否存在于模块 TLS 段——所以"只链接不执行"
即可复现。

**修复**：池改指针化 `thread_local std::unique_ptr<...>`（首次真正搜索的
线程才分配）。修复后 embed 单隔离 createShard RSS 27.8MB（HEAD 27.5MB），
真实搜索一次 33.9MB（含惰性池 4.3MB 中被触碰部分），进程常驻回归消除。

**防错坑（新登记）**：跨语言 FFI 库中约 MB 级的 `thread_local` 值对象，
在 macOS dyld 的 TLS 实例化模型下由**所有线程**支付物理内存，与是否执行
无关；一律指针化惰性分配。定位手段：MallocStackLogging + malloc_history
调用树按线程聚合，"链接不执行也涨 RSS"即指向 TLS/静态初始化段。

## 验收证据（修复后全链）

- workspace `cargo test`：616 绿 0 红（含 kk-runner 11 项 pathfinder 单测、
  sandbox 集成 search/findPath/moveTo/两通道等价族、kk-runtime embed e2e）。
- 棘轮（release 重建后）：**1859→1879**（地板随批上调入库）；相对 HEAD~1
  转绿 27 条：PATHFINDER-007~011/013~015/020/022 共 11 例 + LEGACY-PATH-005
  + 涟漪 15 例；转红 2 条经 ratchet-allow 显式豁免（ROAD-TRAVERSAL-002
  墙上路可走、ROOMPOS-FIND-007 costCallback 墙格——均属代价矩阵通道，
  归因见 ratchet-allow note，D3 接线后应转绿并清理条目）。回调族
  PATHFINDER-004/005/012/023 与 ROOMPOS-FIND-002/009 保持红（基线即红，
  归 D3）。
- bot 三基线：tooangel 2000t（0 硬中断、0 console 错）、overmind-dist
  2000t（1 错既有，形态不变）、wasm 10t 通过。**基线形态归因**：
  `PathFinder.search`（tooangel 10000 次/overmind 8010 次）与
  `RoomPosition.findClosestByPath`（overmind 99 次）退出桩计数；tooangel
  因真路径新触达 `room.find:102/108/111/117` 四个既有桩面（采集/结构族
  FIND，既有缺口显形，bot 仍 0 错）——报告随批归档。
- perf A/B（perf-probe 2p×1000t，双样交错）：project/run/settle/wall 各相
  p50 全同、total 与 CPU、RSS 采样区间重叠（A 101.3~102.0MB / B 99.3~101.7MB）
  ——不可分辨。
- doc-links：230 文件通过。

## 残留与登记

- roomCallback/costCallback 通道（含 `getPathfindingGrid2` 结构代价、
  道路 cost 1、creep 避让）归 D3；两例豁免与回调族红用例随 D3 清理/转绿。
- 同进程多引擎并发搜索会串 C++ 进程级地形表（embed CLI/ratchet/bot 均单
  引擎单进程；多引擎仅串行单测出现）——按设计文档边界维持登记。
- JSON 通道每 tick 每玩家按 rev 缓存命中零成本；KKPV 通道首搜索懒解码一次
  （µs 级，1-2 房世界）；百房级世界的打包/比对成本未测，归 D7 测量收口。
- `search` 结果经 op 的 serde_v8 数值/数组往返：路径数百步量级未见开销；
  上万次搜索的 bot 基线 CPU 无变化（上表）。
