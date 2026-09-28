# M2A-A3c per-viewer 装配（开发报告）

基线 main=74c16d5，分支 m2/a3c。方案条目：**A3c per-viewer 装配**（world_json 增
user/cpu 真值；Game.collections/my/FIND_MY_*/energyAvailable 按 viewer——清偿正确性-3
与 energyAvailable 灰区）+ 灰区表归 A3c 的装配裸全局族。

## 官方取证锚点

- FIND 过滤全表 `findCacheFn`：engine/src/game/game.js:17-28（FIND_CREEPS 族滤
  `!i.spawning`；MY_*= `my===true`；HOSTILE_*= `my===false`（STRUCTURES 另加
  `&& i.owner`——owner getter 对 null/缺键归 undefined，structures.js:136-138））
- energyAvailable/CapacityAvailable per-viewer：game.js:324-328（`!off &&
  (extension|spawn) && user==viewer` 才累计）；结算侧重算**不滤 owner**：
  engine/src/processor.js:326（全房聚合——两口径并存是官方事实）
- GCL 正变换：game.js:130-163（`level=floor(pow(gcl/1e6, 1/2.4))+1`、
  progress=点数−前级基数、progressTotal 浮点不取整；GCL_POW=2.4/GCL_MULTIPLY=1e6
  common/lib/constants.js:284-285）；点数来源 `runtimeData.user.gcl`
- Game.cpu：game.js:145-153（tickLimit=runtimeData.cpu、limit=user.cpu、
  bucket=cpuBucket）；窗口先算后装（driver make.js:85-86）
- 视角中立面契约：screeps-ok src/find.ts（SUPPORTED_FIND_CONSTANTS 仅 9 个中立
  常量，MY_*/HOSTILE_* 抛 Unsupported）；CreepSnapshot 带 spawning 字段
  （common.ts:27）——中立面 FIND_CREEPS 含孵化雏形，玩家面才滤

## 实现清单

1. **FIND 族 per-viewer（两层同步）**
   - kk-world `matches_find(o, find, viewer)`：Some(viewer)=玩家面官方全表
     （MY_*/HOSTILE_* 归属、FIND_CREEPS 族滤雏形）；None=中立面（MY_*/HOSTILE_*
     返回空——封死假绿面）。`find_in_room` 签名带 viewer；snapshot 查询面按契约恒 None
   - prelude room.find：101/102/103 补 `!spawning` 滤；新增 103/109/113/114/115
     （HOSTILE_CREEPS/STRUCTURES/SPAWNS、MY/HOSTILE_CONSTRUCTION_SITES）臂，
     敌对臂按官方 `my===false && owner` 只认字符串 owner
2. **energyAvailable per-viewer**：world_json 房间投影按 viewer 过滤自家非 off
   spawn/extension（game.js:324-328 同构）；`world_json()` 全量面保持聚合（测试面）
3. **Game.gcl 真值**：PlayerState.gcl → embed 传参 → world.gcl 点数 → prelude
   官方正变换（模块级捕获 mathPow/mathFloor/GCL 常量）；creep.js `gclPoints()`
   直读 world.gcl（claim 预检门），缺省世界逆变换兜底（=0 点，与旧硬编码等价）
4. **Game.cpu 真值**：sandbox `begin_tick` 注入前覆写 world.cpu（budget 真值：
   limit/tickLimit=本 tick 窗口/bucket=tick 初桶；默认配置下与旧静态桩逐值相等
   ——ratchet 无翻转风险）；gcl/cpu 覆写点注释互指
5. **装配裸全局清偿（灰区 105/113 条）**：buildGame/beginTick 的 new Map/Set、
   对象/terrain/perTickReset 循环索引化、room 回填 .find→rawById 直查（顺带
   O(N²)→O(N)）、wrapObject `globalThis[cls]`→惰性定格类表、makeRoom 的
   `globalThis.Room`/setPrototypeOf/defineProperty 捕获、rawData 的
   `Map.prototype.get` 捕获、cjs require 去 Set 化、harvest/build 扫描索引化

## 测试

- kk-world +3：find_per_viewer_semantics（双主人+雏形+无主 wall 全矩阵）、
  energy_per_viewer_projection（双主人+off spawn）、gcl_points_projection
- kk-runner +1：viewer_meta_semantics（gcl 公式 0/1e6/1.5e6 三档含浮点
  progressTotal、cpu 真值自定义配置 limit 10/桶 30→tickLimit 40）
- kk-engine embed：oracle 面 MY_* 改锁空（旧断言即登记的假绿）；玩家面新增
  MY/HOSTILE/FIND 锁值 + **gcl 跨层行为探针**（settle_external 直注 upgrade
  意图→PlayerState.gcl→world.gcl→Game.gcl.progress，纪律②）
- 全套：workspace 26 套件全绿 + fmt + clippy 零警告 + embed 冒烟
  （双主人房 FIND=2/MY=1/HOSTILE=1）

## 发现与登记（灰区表已回写）

- **upgradeController prelude API 面仍是桩**（探针发现）：执行器完善但 API 未
  换真——A4-iii 只换真 controller 族五方法，upgrade 漏网。玩家调它得假 OK。
  归 A5/API 补漏批（新登记）
- Game.resources 形态分歧：我方收掉落资源对象 vs 官方账户资源对象（新登记，
  M2B 用户资源面）
- world.user 字符串形状 vs 官方 user 对象（新登记，M2B 收拢）
- createShard 种子钟 +1 失配（inspection controller relativity 两用例）：**本批
  未做**——归 createShard 语义轨道单独清偿（vanilla index.ts:968-975 warm-up
  tick，涉及时序面，与 per-viewer 正交）

## 风险自评

- prelude 索引化改写面较大（8 文件 16 处）——评审重点：语义等价性（尤其
  buildGame 回填序、classCtor 惰性定格时点先于玩家代码）
- room.find 玩家面新臂的 owner 三态（字符串/null/缺键）与 Rust matches_find
  的 `owner_str` 判定需逐臂对表
- cpu 覆写在 begin_tick（skip 分支不覆写——停跑 tick 玩家根本不执行，无泄漏面）

## 三路评审清偿记录（508a9c1 评审后）

三路结论：评审 1（官方语义）可合入 4×P3；评审 2（防篡改/装配）需修复后合入
2×P2+2×P3（含两组探针实证）；评审 3（Rust/测试）可合入 1×P2+3×P3。全部清偿：

- **P2-1（评审 2）类表惰性定格被证伪**：game.js 加载期 refreshGame() 在空世界下
  零 wrapObject → 空世界首 tick（新玩家）玩家代码可先于首次 wrapObject 执行并
  污染定格表（探针实证装配对象永久携带攻击者原型）。清偿：类表改**加载期
  立即定格**（_registry 安装循环后）；类全局同时改官方同形非可写
  defineProperty（creeps.js:1123/store.js:99——P3-3 一并根治，绑定交换结构性
  免疫）；Store 移出桩表由 structures 段官方同形安装（非可写会挡掉唯一重绑）
- **P2-2（评审 2）装配清偿覆盖不全**：补 Map.prototype.set、Set.prototype.add/
  .has、Object.entries、Array.isArray（game.js——__Array 已捕获未用）、
  Map.prototype.get（room.js makeRoom/find/getObjectById）、Object.keys/
  defineProperties（defineGameObjectProperties 每 tick 装配路径）全部捕获
- **P3-4（评审 2）+ P3-3（评审 3）**：敌对臂 103/113/115 改 `typeof owner ===
  'string'`（与 Rust/109 同口径）；hostile_of 注释补官方 103/113/115 无 owner
  检查的分歧说明与不可达依据；cleared controller 的 109 臂锁值断言补齐
- **P3-1（评审 3）**：world_json.rs cpu 注释改指 sandbox.rs begin_tick（原指
  proto.rs 失准）
- **P3-2（评审 3）**：cpu 覆写改 beginTick 第二参数 JS 侧合并——消灭每 tick
  整世界深拷贝（含每房 2500 元素地形数组）
- **P2-1（评审 3）**：claim 门 gclPoints 直读行为级锁（claim_gate_gcl_behavior：
  gcl 0+已占 1 房 → -15 / gcl 1e6 → 0）
- **F-1/F-3（评审 1）**：prelude 裸 run 中立面语义收口（MY_*/HOSTILE_* 空、101
  含雏形——与 Rust None 臂同构）；灰区表口径精化（viewer 相关 10 臂、120/121
  随 powerCreep 批、真值/严格 false 混合）
- **F-2（评审 1）**：JS find 臂止于 116 的覆盖缺口登记（归 B3 接线批）
- **F-4（评审 1）**：hostile_of 两臂差异注释（防 schema 演化静默沿分歧）
