# C.4 玩家 API 安装（2026-10-03）

[计划](../../plans/mod-plane-batches.md) C 批第四片。范围：Mod 对象进入
玩家投影（可见性裁切、双通道）、FIND/LOOK 查询常量、player.js 装载进
每个玩家 VM 与安装 API（类绑定/全局定义/字段 getter/emit 预检/查询
安装）、KKPV 块头换真实计划指纹（B.5 残留收口）。**不含**：既有结构
wrap/replace 续行（C.5）、Mod 对象授予房间视野、Rust 查询面（admin
find）的 Mod 常量、玩家面 SDK 的更多预检（见残留表）。

## 交付物

- **玩家面目录**（kk-mod-api `player_surface.rs`）：`PlayerSurface::
  from_plan` 从冻结计划确定性导出——FIND 常量自 `MOD_FIND_BASE=1000`
  按玩家面类型键序分配（ServerOnly 类型不进目录、不占常量；同一计划
  恒同值，跨计划不承诺稳定）；导出时拒绝：字段裸名占用记录保留键
  （type/modType/id/room/pos/ext）、非位置字段占用 x/y、条件可见性
  （对象级与字段级）、非法/重复玩家类名。
- **玩家模块导出**（kk-mod-host `realm.rs`）：`Registered.player_modules`
  ——声明 `player` 面的包依拓扑序携带（入口 + 整包 UTF-8 文件），
  字节与装载期内存快照同源。夹具 demo-counter/demo-base 均实现真实
  player.js（类/getter/increment/emit/查询常量全局）。
- **沙箱装载**（kk-runner `mod_api.rs` + `sandbox.rs` + prelude
  `41_mod_player.js`）：`SandboxConfig.mod_api`（默认 None——vanilla
  分片全局面与运行时选项零变化）。存在时：装配 `kkmodplayer://` 内存
  模块加载器（静态导入图唯一形态，动态导入/外部导入/跨包拒绝）→
  清单注入 → 41 段安装运行时 → 逐包 `load_side_es_module` +
  default(install_api) 调用（同步纪律：返回 Promise 即拒）→ finalize
  校验必需绑定。安装 API：`api.builtins`（内建类安装期捕获并冻结）、
  `api.objects.bind(typeKey, Class)`、`api.globals.define(name, value)`
  （标识符合法、不得覆盖既有全局、non-writable/non-configurable）、
  `api.view.field(obj, name)`（经 `gw.rawData` 读当轮已授权投影）、
  `api.intents.emit`（键在目录 + 目标含 id 预检 → C.3 原语；错误码
  返回）、`api.queries.install(typeKey)` → `{find, look}`。
- **对象装配与查询**（prelude `_registry.js`/`room.js`）：wrapObject
  对 `type:'mod'` 记录用 `gw.modApi.types[modType].cls` 连原型（探测
  标签=类型键）；room.find 增 `type >= 1000` 分支（findByConst 映射）；
  look 分类进类型键桶（动态初始化，键含冒号不与内建 look 键冲突）；
  lookAt/lookAtArea 在内建固定序之后按键序追加 Mod 类型；lookForAt
  经 lookSpatial 键集自然支持。
- **投影扩展**（kk-world `world_json.rs`/`kkpv.rs`）：`mod_record_json`
  ——对象级可见性（RoomVisible 走既有房间视野门；OwnerCollection=
  归属者集合语义，与己方旗同形不受房间视野门、他人不可见）+ 字段级
  裁切（Public 恒可见 / Owner=查看者是归属者 / ServerOnly 不出），
  信封键最后写入（声明位置字段同名时被等值覆写——写入口径保证同步）。
  JSON 与 KKPV 两通道同口径（KKPV 桶键=类型键）；`world_json()` 内部
  全量形态与查询快照仍走 object_snapshot 专用读面（C.3 口径不变）。
  WorldSource 两方法签名扩展 `surface` 参数。
- **真实计划指纹**（kk-runtime/kk-engine）：KKPV 私有块、公开地形束
  与注入采样的 `plan_hash` 从占位常量换成 `Engine::plan_hash()`
  （`kk-mod-api` 冻结结果）——计划变化使旧块按指纹失效（B.5 消费者
  缺口收口）。
- **分片接线**（kk-runtime `embed.rs`）：`create_shard_inner` 在引擎
  装配前导出玩家面目录（失败即分片创建失败，不产出半装状态）；装配
  成功后置位 `player_surface`/`mod_player_api` 并注入每个玩家沙箱配置
  （测试配置不可改写）；reset/teardown 同步清理。

## 验证

- workspace **520 测全绿**（C.3 后 510 + 新增：kk-mod-api surface 族
  7、kk-mod-host player_modules 1、kk-runner 玩家面四场景（安装面/
  记录装配/模块门/全局面守卫）、kk-world 可见性矩阵与双通道对照 2、
  kk-runtime c4 e2e 五场景（全链/双通道一致/字段裁切/装配门与防篡改/
  server-only 免绑））；clippy `-D warnings`、fmt、doc-links 通过。
- **棘轮 1854/2914 零翻转**（转红 0、转绿 0，无新豁免）。
- **bot 三基线**：tooangel 2000t（2000/0/0）、overmind 2000t
  （2000/0/1——与 C.2/C.3 报告同形，既有形态）、wasm 10t（10/0/0）。
- **零 Mod 开销同机 A/B 双样（交错 h1/w1/h2/w2）**：HEAD（3140f63）与
  工作树各 2 样（8 玩家/300 tick/warmup 50，默认 JSON 通道）：
  project 0.554/0.555 vs 0.552/0.551、run 0.330/0.348 vs 0.343/0.339、
  settle 0.130/0.133 vs 0.130/0.129、wall 1.022/1.039 vs 1.026/1.029
  （ms，p50）——全相差 ±2% 内，**不可分辨**；≥5 样正式判读后置
  （与既往口径一致）。

## 评审与清偿（三路并行只读评审 + 清偿复审）

三路评审（R1 Rust 链路与信任边界 / R2 V8 沙箱与玩家面 / R3 投影双通
道与兼容面）合计 **4×P1 + 2×P2 + 9×P3 + 2 存疑**，处置：

- **P1 KKPV OwnerCollection 不可见房分叉（R1+R3）**：kkpv 侧 Mod 分支
  原在 `object_exported_for` 门后，不可见房的归属者集合对象在 KKPV
  通道被丢弃而 JSON 通道保留。清偿：Mod 分支移到门前（与
  world_json_impl 同序，`mod_record_json` 自带完整对象/字段级门），
  新增不可见房 OwnerCollection 的双通道逐键对照测试。
- **P1 跨包 kkmodplayer:// 导入（R1+R2+R3）**：resolve 绝对分支原不核
  对 referrer 包属，A 包 player.js 可静态导入他包任意文件。清偿：绝对
  分支按 referrer 命名空间核对（宿主入口装载的 referrer 非本方案，放
  行），同时拒绝查询串/锚点（防同文件双模块实例）；load_sync 路径
  百分号解码（URL 编码与包内文件键对称）；新增跨包/查询串拒绝测试。
- **P1 安装期无看门狗/近堆兜底（R2）**：player.js 首次执行原在
  Watchdog 与近堆回调注册之前——死循环楔死分片创建、无界分配进程
  中止。清偿：装配序改为「宿主句柄捕获 → Watchdog::spawn + 近堆回调
  + arm(安装墙钟期限，默认 10s、SandboxConfig.install_wall_limit 可
  覆盖) → 安装 → disarm + cancel_terminate」；测试覆盖死循环（500ms
  期限）与近堆（32MB 堆 + 无界分配）两形态。
- **P1 Float 域列型混派（R3）**：f64 字段的整数 backing（json!(2)）与
  浮点 backing 在 KKPV 列式编码分派不同列型，同桶混合使整轮投影失
  败（JSON 通道无此约束——失败形态不对称）。清偿：写入口统一规范化
  （`validate::normalize_object_payload`——Float 域统一浮点 backing，
  嵌套 schema 递归；`update_mod_fields` 取 owned 副本后规范化，
  `place_mod_object` 同口径）；单测（顶层/嵌套）+ e2e（同房同类型
  2 与 2.5 混合，双通道读值一致）。
- **P2 spawn 阶段失败留中间态（R1）**：坏 player.js 原在引擎装配与标
  志置位之后、逐玩家 spawn 时才失败。清偿：`create_shard_inner` 在引
  擎装配前用一次性 canary 沙箱完整预检安装（清单→装载→install→
  finalize）；ext_intents 复用同一 `has_mod_intents`；文档注释改为如
  实描述失败语义（调用方须 teardown/重建收尾）。
- **P2 每玩家深拷贝 Mod 源码（R1+R2）**：`ModPlayerApi.modules` 改
  `Arc<Vec<ModPlayerModule>>`，装载器持 Arc 零复制。
- **P3 批清偿**：41 段 emit 目录按安装包命名空间裁剪（跨包意图键不
  经本包 API 发出）；安装运行时两入口（createInstallApi/finalize）
  捕获为 Rust 持有的 v8 句柄且**模块执行前删除 `__kkp`**——Mod 代码
  与玩家同权限面；`findBase` 经清单注入（room.js 无 1000 字面量，
  Rust 侧唯一权威）；Mod look 桶在 makeRoom 按玩家面目录预初始化
  （空房 lookForAt 与内建同语义返 []，`__proto__` 形态结构性不可
  达）；room.find 的常量映射提出对象循环；game.js 回退视野推断排除
  mod 记录（Rust 权威口径一致，测试夹具改带 visibleRooms）；
  `mod_record_json` payload 损坏返回 None（与文档一致）；注册期校验
  玩家入口 UTF-8 且在导出集内（不再静默滤除后以误导文案推迟到 spawn
  失败）；snapshot.rs 过时注释、c3 夹具双写 mod.json、保留键惰性依据
  注释三处清理。
- **登记不修（残留）**：同包 `import './server.js'` 可装载执行（包是
  共同可信单元的面划分决策，随 SDK 冻结 revisit）；装载器源码常驻
  （Arc 共享消除逐玩家复制，模块进 V8 缓存后释放装载器属后续优化）；
  Unsigned 域同桶大小跨 i64::MAX 混派（触发极冷僻、失败 loud）；
  kkpv.js 沙箱解码器不校验 plan_hash（宿主路由信任，设计取舍）；
  公开束库 acquire 键不含 plan_hash（结构性不可达——计划唯一装配路径
  必然重建库，热换计划属 E 批设计）；B.3 扩展字段写入门的 Float 规范
  化（同病根，无现实触发，随扩展面复用一并收口）。

**清偿复审**（只读复审 agent）逐条复核：4×P1 与 2×P2 全部真闭合（含
判别力推演——门序回退会使不可见房测试红、Err 路径看门狗经 Drop
shutdown 干净回收、deno_core 近堆回调为单槽替换无双重注册、canary 与
spawn 共享同一 Arc、入口装载 referrer="." 放行可达）；P3 批 9 子项
8 项闭合，唯一例外是 payload 损坏的 RoomVisible 分支仍产骨架记录
（文档口径级）——已随后收口（`from_str().ok()?` 对两种可见性统一
整体不导出）。结论可提交。

**清偿后全量复跑**：workspace **523 测全绿**（清偿新增 13：kk-mod-api
规范化 2、kk-runner 跨包/查询串/死循环/近堆/空桶 5、kk-world 不可见房
OwnerCollection 1、kk-runtime f64 双通道 1 + 既有族扩面）、clippy/
fmt/doc-links 通过；**棘轮 1854/2914 零翻转**；**bot 三基线同形**
（tooangel 2000/0/0、overmind 2000/0/1 既有、wasm 10/0/0）；**零 Mod
开销 A/B**：project −1.0%、settle −0.8%、wall +0.6%、RSS −0.5% 不可
分辨；run 相点估计 +3.5%（HEAD 3 样 0.339~0.383、工作树 6 样
0.348~0.396——区间完全重叠、样间散布 ±6%，按机器瞬态归因；正式判读
后置并对 run 相标注复核）。

## 登记的简化与残留（后续承接）

1. **Mod 对象不授予房间视野**：`room_visible_to` 的 object_owner 不认
   Mod 对象——仅有 Mod 对象的房对任何玩家不可见；RoomVisible 对象
   依赖其他视野源。授予视野的声明面（能力目录）归 D 批 slots。
2. **Rust 查询面不认 Mod 常量**：`find_in_room`/`matches_find`（admin
   查询面）对 FIND ≥1000 无分支；玩家 FIND 在沙箱侧 room.find（权威
   面）。admin 面 Mod 查询归后续管理面扩展。
3. **emit 预检最小面**：键在目录 + 目标含 id；参数 schema/接收者/
   归属由宿主结算侧权威校验（C.3）。更富预检（错误码细分、schema
   前置）随 SDK 面冻结。
4. **玩家模块导入面**：仅包内相对导入（`@kkscreeps/mod` 玩家侧变体、
   公开内建模块化）未提供；整包文件进玩家 VM（含 server.js 字节），
   玩家模块 import './server.js' 会装载执行——登记为暴露面决策：
   包是共同可信单元（服主安装），跨面复用不构成越权，后续可按需收窄
   文件集。
5. **每玩家重建成本未单独度量**：41 段 + 模块装载只发生在沙箱构造
   （每玩家一次，非每 tick）；B.5 每玩家固定开销结论未受影响（A/B
   不可分辨），玩家重建频率模型出现后实测。
6. **OwnerCollection 对象不进 Game 专属集合**（无 `Game.counters`
   类面）；玩家经 getObjectById/FIND/LOOK 访问。集合面随 SDK 冻结。
7. **C.3 残留对账**：残留 4（玩家可见面）✅ 本片；残留 7（KKPV 计划
   指纹占位）✅ 本片；残留 11（视野/授权策略与投影绑定）部分——授权
   仍只看 owner_field，视野门沿用房间视野（Mod 对象不授予）；完整
   策略归 D 批。其余 C.3 残留（对象域外动作/ctx.objects 多目标/
   计费上限/能力面/裸名碰撞 SDK 定论/位置能力）不变。

## 承接到 C.4.5 的登记（2026-10-03，外审三路 + 外部 AI 复核）

四路评审（R1 Rust 链路 / R2 V8 重入 / R3 兼容面 / 外部 AI）对 C.4.5 前置
重构的发现与清偿，见本文件末尾「C.4.5 登记」节。两条**本次未引入**的既有
测试缺口经 HEAD 对照确认后登记在此，供后续批次认领：

1. **link 归属校验无判别力**：`process_structure_intents` 的 link 分支跳过
   归属校验，变异后全部测试仍绿；在 HEAD 快照做同一变异同样零红——属既有
   缺口，非 C.4.5 引入。
2. **controller 无主判定无判别力**：`o.owner.as_deref().is_none_or(...)`
   改坏后零红；HEAD 对照同样零红——同为既有缺口。

两条都在本切片「归属 → 类型分派」的改写路径上，改写本身经变异验证等价；
但既然落在这条路径上，补判别力用例的成本很低，建议随下一个触碰
`structures.rs` / `controller` 的批次一并清偿。
