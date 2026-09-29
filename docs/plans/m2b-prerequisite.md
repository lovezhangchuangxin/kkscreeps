# M2B 前置批：seam 三件套 + 投影/编解码宏化方案

> 状态：已收口（2026-09-30 五片全部落地，逐片记录见 §7；基准 HEAD=ef43f4a）。
> 依据：[ADR-013](../decisions/ADR-013-architecture-review-coordinator.md)
> 决策 2（性能第二形态「seam 先行」）；切片纪律沿用 [b0-slices.md](b0-slices.md) §3
> 统一验收口径（B1 同款），本批增补 perf-probe 同口径对比（§5）。

## 0. 目标与总原则

- **seam 先行**：M2B 玩家 API 大扩面（~150→340 方法）**之前**落地三件——世界注入
  来源接口化、地形按世界版本缓存、不可变房间版本句柄；M2B 新命名空间的 getter
  全部写在 seam 抽象之上，不直连世界具体类型。
- **列式布局与零拷贝冻结不预设结论**：由端到端测量决定
  （[snapshot 契约](../architecture/snapshot-contract.md)「冻结格式前的验收」
  条款不变）。本批交付的是版本/缓存/句柄的**结构面**，不是字节格式。
- **传输层性能、语义零变化**：一切改动不以玩家可见行为换性能；玩家可见面由
  既有测试面锁定（workspace / 棘轮 / bot 三基线）。
- 宏化反「统一抽象」纪律：只宏化**机械齐一**的样板（臂臂同构、零语义分叉）；
  契约差异层保持显式 match + 测试锁定，不造跨契约 DSL。

## 1. 勘测结论（2026-09-30 对 HEAD=ef43f4a 实测）

### 1.1 投影/注入链（perf 主热点）

每 tick 逐玩家（embed.rs:477-509 循环）：`world_json_for_viewer` 全量重建
serde_json::Value（每房 **2500 元素地形 JSON 数字数组** + 全部可见对象投影）→
sandbox.rs:349 `to_string` 整体字符串化 → JS `__jsonParse`（game.js:501）→
game.js:543-548 每 tick 重建 `terrainByRoom`。perf-a6 基线（2 玩家）：
project p50 0.164ms / wall 0.813ms（投影 ≈ 墙钟 20%）；4 玩家 project 0.517ms
（随玩家线性放大——object_json 本身与 viewer 无关，重复投影 N 次）。

### 1.2 地形与版本现状

- 地形首玩家 tick 后不可变（world.rs:214 `set_terrain` 守卫 + embed 侧同守卫
  :370-375；结算零写点），但每 tick 每玩家每房重复：重建 2500 元素数组 +
  序列化 ~7.5KB + JS 解析 2500 个数字。
- `World`（world.rs:126-134）无任何版本/代际计数——仅 time/seed/player_ticks/
  rooms/next_obj_id。snapshot 契约的代际号在 typed 层尚无对应物。
- `Terrain = Box<[u8;2500]>`（kk-core terrain.rs:24）：每房每 tick 随
  `RoomState.clone()` 复制 2500 字节（settle/mod.rs:686 整房克隆的工作副本）。

### 1.3 新增对象类型同步点全表（附录 §8 存全表，此处结论）

原始编辑点 **30 处 / 7 层**；其中**机械齐一**（臂臂同构、宏化零语义风险）的
只有 kk-schema 枚举管道 8 块（enum 本体 / id / pos / set_pos / encode /
decode+兜底 / tags 模块 / tag()，各 16 臂逐臂同构，room.rs:389-625）+
world.rs `set_object_id`（§8 表 #10）。其余每处承载各自契约或官方
语义（归一化缺省面、两投影各自契约形状、FIND 语义、prelude 装配），不属
机械样板。

两投影文件的真重复（未单源）：`object_owner`（world_json.rs:194 投影私有 vs
settle/mod.rs:370 `object_owner_of` 结算侧第二份，注释互认同语义——跨 crate
双份拷贝）、`controller_progress_total`（world_json.rs:673 返 0 兜底 vs
snapshot.rs:167 返 Null 兜底，同构查询两份）、store 家族（world_json.rs:266
`store_json`/`store_with_extra_json` vs snapshot.rs:103 `snap_store`/
`snap_store_extra`，逻辑几乎逐行同、差异仅 energy 键恒投影）、controller
owner 三态（world_json.rs:393 vs snapshot.rs:244，两份手写、注释互指同步）。

### 1.4 两文件现状

world_json.rs 1617 = 码 ~770 + 测 850；snapshot.rs 1106 = 码 ~400 + 测 706。
B1 留给本批：宏化/单源后须双双 <1000。（P4 复核补录：spikes/ 实验目录
非产品源码，不在 <1000 验收口径——inspector-spike main.rs 1394 行为
HEAD 既有，B1 §1 的「其余全仓」表述未含此排除，系口径笔误。）

## 2. 范围定夺

**做**：
1. kk-schema `kk_room_object!` 枚举管道宏（8 块机械样板 → 1 处声明）+
   world.rs `set_object_id` 消灭（宏顺产 `RoomObject::set_id`）；
2. `object_owner` 跨 crate 单源（kk-world pub 化，kk-engine 第二份删除）；
3. seam：`WorldSource` trait（kk-runtime，消费方持有接口——kk-engine Sandbox
   trait 先例）+ embed 全部世界读取改经 seam；
4. 世界版本面：`World.generation`（提交点递增）+ `terrain_epoch`（仅地形
   实际替换时递增）+ `RoomVersionHandle`；
5. 地形按版本缓存（紧凑载荷 + rev 戳 + Rust/JS 双侧缓存，§3.3）；
6. 投影 helper 单源化（progress_total / store 家族 / owner 三态）+ 两文件
   测试拆出 <1000；
7. 按代际的对象投影共享缓存（**实测定去留**，§3.5）；
8. `Terrain` 内表示 Box→Arc（克隆停拷 2500B，API 零变化）。

**不做**（显式出界，评审按此判）：
- **列式布局/零拷贝冻结**——snapshot 契约字节格式由端到端测量另批定夺；
- **settle 工作副本整房克隆的彻底消灭**——那是 CoW/持久化数据结构的重写，
  正是被「测量决定」条款押后的列式批决策；本批只以 Arc 化削其地形拷贝；
- **双房双玩家实弹验证**——另出方案（跨房 tick 合并规则）；
- **prelude JS 侧类型注册表/字段表的代码生成**、`matches_find`（world_json）
  与 room.js find 数字链的合并——JS 镜像面保持手写，层间一致性由既有对表
  测试锁定；生成 JS 是收益存疑、风险实在的跨语言 DSL（反「统一抽象」纪律）；
- **kk-engine `Sandbox::WorldView`（假沙箱路径）的 seam 化**——M2B getter
  不写在 kk-engine Sandbox trait 上，本批不动；登记。
- **投影 match 臂本身的宏化**——object_json/snapshot_json 两 match 是两个
  不同契约（prelude 形状 vs screeps-ok 查询形状）的显式表达，臂即契约；
  宏化收敛点=机械层（枚举管道）与真重复层（helper）。**此为方向性判断，
  评审可挑战**（挑战成立的标准：能指出某宏化形态对两契约各自语义零折损且
  样板收益实证）。

## 3. 设计

### 3.1 seam 接口形状：`WorldSource`（落 kk-runtime）

```rust
/// 世界注入来源（seam，ADR-013 决策 2）：沙箱所需的全部世界读取面经此
/// 抽象。M2B 新命名空间的 getter 一律加在本 trait 之上，不直连
/// kk_world::World——后续列式/零拷贝实现（snapshot 契约）替换后端时
/// getter 面零改写。接口按「沙箱需要什么」塑形，不按 World 恰好暴露什么。
pub trait WorldSource {
    /// 每 tick 世界注入（per-viewer 投影；含地形协议 §3.3）。
    fn world_json_for_viewer(&self, viewer: &str, gcl_points: i64) -> serde_json::Value;
    /// 单对象查询（screeps-ok 契约形状，snapshot.rs）。
    fn object_snapshot(&self, id: &str) -> Option<serde_json::Value>;
    /// 房间 FIND 查询（契约快照形状）。
    fn find_in_room(&self, room: &str, find_const: i64) -> Vec<serde_json::Value>;
    /// 房间 controller 坐标。
    fn controller_pos(&self, room: &str) -> Option<(u64, u64)>;
    /// 当前世界代际（提交点单调递增，§3.2）。
    fn generation(&self) -> u64;
    /// 地形纪元（仅地形实际替换时递增——地形缓存键，§3.3）。
    fn terrain_epoch(&self) -> u64;
    /// 房间版本句柄（tick + generation；「同一 tick 内所有 getter 绑定
    /// 同一版本」纪律的结构载体，snapshot 契约「代际」节的 typed 层起步）。
    fn room_version(&self, room: &str) -> Option<RoomVersionHandle>;
    fn game_time(&self) -> u64;
}
```

`impl WorldSource for kk_world::World` 同文件落（消费方 crate 持接口，同
B0 的 Sandbox trait 在 kk-engine 的先例）。EmbedEngine 的
execute_players/get_object/find_in_room/controller_pos/get_game_time 全部
改经 `&dyn WorldSource`；对外方法签名零变化（kk-backend sidecar 协议不动）。

### 3.2 世界版本与不可变句柄

- `World.generation: u64`（私有 + `pub fn generation()`）——**提交点**递增。
  实现机制（P2 实现期定夺，评审 1/3 核实健全——较本节原稿「engine 提交尾
  显式调 bump_generation」更优：消灭位序耦合，任何 advance_ticks 调用方
  天然覆盖）：bump 烧进 `World::insert` / `set_terrain` / `register_room`
  （新房）/ `remove` / **`advance_ticks`**（时钟折叠面是投影输入，tick 推进
  即投影变化；kk-engine 提交尾直接写回 `world.rooms` 后必经 advance_ticks
  收束——两调用点之间无投影机会）+ `room_mut`/`get_mut` 保守覆盖（可变
  访问即失效，过度失效方向安全；现生产调用面为零）。`pub fn
  bump_generation()` 保留为防御面 API（当前零调用——pub `rooms`/`time`
  字段直写的唯一阀门）。结算失败路径不 bump（世界未变）。
  评审专项（§6-4）已执行：grep 全部世界可变面，非测试直写 `rooms` 仅
  settle 提交尾一处（经 advance_ticks 覆盖）。
- `World.terrain_epoch: u64`——仅 `set_terrain` / `register_room` **实际
  替换地形**时递增（`entry().or_insert_with` 对已存在房不替换则不 bump）。
- `RoomVersionHandle { pub room: String, pub tick: u64, pub generation: u64 }`
  （kk-world 定义，Copy 语义按 String 取常规）；`World::room_version(name)`。
- EmbedEngine 每 tick 循环开始固定当前 generation（结构上由「循环期间无
  &mut world」保证），注入与查询共用——「读取方先固定世界级版本」纪律的
  typed 层起步；列式时代的字节句柄（snapshot 契约 SnapshotHandle）在此外延。

### 3.3 地形按版本缓存（两级交付）

**Level 1（必做）**——紧凑载荷 + rev 戳 + 双侧缓存：

- 注入协议：`rooms[name].terrain` 值从 2500 元素数字数组 →
  `{"rev": <terrain_epoch>, "payload": "<base64(2500B)>"}`（键名实现期定，
  紧凑为要；7.5KB → ~3.3KB）。
- Rust 侧：`World` 内 `RefCell<BTreeMap<String, (u64, Arc<str>)>>` 缓存
  （room → (epoch, b64 串)），epoch 不匹配才重编码——2500 元素数组的每 tick
  重建彻底消失。b64 编码器内联 kk-world（无新依赖）。World 单线程持用
  （embed 主线程），RefCell 无 Sync 需求；Clone 随体（缓存克隆无害）。
- JS 侧：game.js beginTick 改「rev 命中复用、未命中解码」——
  `gw.terrainCache[room] = {rev, array}`，`terrainByRoom[room] = array`。
  解码产物为 `Uint8Array`，下游索引兼容（消费面全表见 §6 坑 1）。b64 解码器
  就地定义于 game.js（消费点同段，免跨段桥）。
- 玩家可见零变化：`Terrain.get` 位码数字、`getRawBuffer` 拷贝语义、
  `getTerrainAt`、look 的 terrain 词——行为面全部不变。

**Level 2（实测后定）**——稳态省略 payload：embed 记每玩家
`last_terrain_epoch_sent`，稳态注入只发 `{"rev": N}`（JS 缓存命中），
新纪元发全量。失效钩子：`create_shard` 重置；ThreadExecutor 无 respawn/
evict 面（勘测确认：沙箱与分片同寿命，evict_idle 在池上但 embed 路径不调），
故仅需分片建立时重置。若 Level 1 实测后地形序列化已不显著（注入占比小），
Level 2 不做并登记。

**并行线协调**：f2-renderer worktree 的只读 exporter 若消费
`rooms[].terrain` 形态，合并期对齐（本批在 main 改形态，F2 分支隔离不并线）。

### 3.4 宏化与单源化

**`kk_room_object!`**（kk-schema/src/lib.rs，与 kk_schema! 平行的
macro_rules）：

```rust
kk_room_object! {
    /// 房间内对象（variant 枚举：tag 前缀 + 载荷）……（保留原 enum doc）
    pub enum RoomObject {
        Creep(CreepObj) = CREEP,
        Source(SourceObj) = SOURCE,
        ...(16 变体按名引用 tags 常量；doc 属性逐变体保留)
    }
}
```

生成：enum 本体、`id()`、`pos()`、`set_pos()`、`set_id(&mut self, u64)`、
`Schema::encode_into`/`decode_from`（含 `InvalidVariantTag` 兜底）、`tag()`、
编译期 tag 值互异断言（重复值会让解码 match 静默走错臂——契约级错误）。
**tags 常量表保持手写**（实现期定夺：线上契约冻结表 + 逐条沿革注释的显式
数据面；宏按名引用 `tags::X`，漏配由编译器强制）。
变体声明支持 `$(#[doc = ...])*` 属性透传（Tombstone/Ruin 等 B3/B4 注释保留）。
world.rs `set_object_id`（16 臂）删除，`insert_with_id` 改调
`object.set_id(id)`。

**展开等价证明口径**：宏发射体按「与 HEAD 手写版逐臂同文本」构造；审计 =
`cargo expand`（可用则）或宏声明↔变体清点 + 全部行为面（schema 往返测试 /
workspace 268 / 棘轮 / 三基线）。rustfmt 对宏内代码不重排（发射体格式自持）。

**单源化**（逐处先读两侧契约，差异是刻意的则参数化，拿不准的不合并并登记）：
- `object_owner`：kk-world 侧 pub 化（`pub fn object_owner`；消费者走
  `kk_world::world_json::object_owner` 全路径——world_json 本是 pub mod，
  lib.rs re-export 只加导出面不增值，P2-1 清偿定夺不做），kk-engine
  `object_owner_of` 删除、调用点（notify_when_attacked / damage 废墟承接）
  改经 kk_world——跨 crate 双份同步点消灭；
- `controller_progress_total`：共核 `-> Option<i64>`，world_json 侧
  `unwrap_or(0)`、snapshot 侧 `map(json).unwrap_or(Null)` 各自折叠；
- store 家族：`store_json`/`snap_store` 共核，energy 键恒投影差异参数化
  （合并前逐字段 diff 两侧——勘测称「几乎逐行同」，以逐行对照为准）；
- controller owner 三态：共函数（两份注释互指「同步」即单源意图的自认）。

**两文件 <1000**：测试拆 `world_json/tests.rs`、`snapshot/tests.rs`
（lib 非根模块同名子目录解析——B1-2 place.rs 先例），源文件留码面。

### 3.5 按代际的对象投影共享缓存（P5，实测定去留）

事实：`object_json` 与 viewer 无关（viewer 只影响可见房集合/房间聚合/自家
工地破例——world_json_impl :61-105 的过滤全部发生在投影**之外**）。故按
`(generation, room)` 缓存 `Vec<(ObjId, Value)>`（World 内 RefCell，与地形
缓存同键族（代际/纪元）但失效粒度不同（对象缓存单键整代作废、地形缓存按房含纪元）），per-viewer 组装时过滤+clone。N 玩家：投影计算 N 次 →
1 次 + (N−1) 次 Value 克隆。

**去留判据（本批核心验收的实测面）**：`--players 2` 下 project p50 改善
≥10% 且无其他相 p50 劣化 >20% 则保留；否则整体回退并在 §7 登记（Value 克隆
与重建同阶则该缓存无益——这正是「列式由测量决定」的又一数据点）。
`--players 4` 补测一次作缩放证据（非门禁）。

**实测结论（2026-09-30）：回退**——project 相 **+106%**（0.102→0.210，
三跑稳定；4 玩家 0.215→0.590 为 2.7 倍劣化）。归因=读侧局部性与表示
成本（clone 追逐分散堆上 Value 树+工作集翻倍，vs json! 从连续 typed
载荷直构；两者堆分配同阶——收口评审订正，详见 §7 P5），Value 级共享
结构性不回本；共享收益须换表示（列式/rev 键控载荷旁路/引用可共享
结构）方可兑现——归测量批（与 §3.3 Level 2 登记同向）。

## 4. 切片

| 片 | 内容 | 主落点 |
|---|---|---|
| P1 | `kk_room_object!` 宏（8 块 + set_object_id 消灭）+ `object_owner` 跨 crate 单源 | kk-schema/room.rs+lib.rs、kk-world/world.rs+world_json.rs、settle/mod.rs+damage.rs（object_owner_of 第二调用点的必然波及） |
| P2 | seam：`WorldSource` trait + embed 全读取改经 seam + generation/terrain_epoch/RoomVersionHandle + Terrain Arc 化 | kk-runtime/embed.rs+新 trait 模块、kk-world/world.rs、kk-core/terrain.rs |
| P3 | 地形 Level 1 缓存（b64+rev+双侧缓存；锁步面：prelude game.js / bot_check 夹具 / 形态断言）[+Level 2 若测得必要] | world_json.rs、game.js、bot_check.rs |
| P4 | 投影 helper 单源化（progress_total/store/owner 三态）+ 两文件测试拆出 <1000 | world_json.rs、snapshot.rs |
| P5 | 按代际对象投影缓存（实测定去留，判据 §3.5） | world_json.rs（World 内缓存） |

切片依赖：P2 供 P3/P5 的版本键；P1 独立；P4 依赖 P1 的 owner 单源先落
（同文件免二次冲突）。方案文档随 P1 入库（B1 先例）。

每片批次循环：`cargo test --workspace` 全绿 → 显式 cd 主仓
`cargo build --release` → `node compat/scripts/ratchet.mjs` 零未豁免转红
（kkscreeps.json 时间戳 diff 以 `git checkout --` 还原）→ bot 三基线
（tooangel 2000t console 零错人工确认 / overmind-dist 2000t / wasm-bot 10t）
→ **perf-probe 同口径**（`perf-probe --ticks 1000 --warmup 100 --players 2`，
基线 compat/reports/perf-a6.md；各相 p50 变化 >20% 须归因——本批核心验收）
→ ≥3 路 subagent 并行评审 → 逐条清偿+清偿复审（复审复核评审共识本身）
→ `pnpm run check` → 单一归因提交（**subject ≤350 字符**）并 push。

## 5. perf 验收口径

- 基线：perf-a6.md（2 玩家 project/run/settle/wall p50 = 0.164/0.520/0.008/0.813ms）。
  **P1 实测登记（2026-09-30）**：perf-a6 绝对值已整机漂移——同机器态下
  HEAD=ef43f4a 原树实测 project/run/settle/wall = 0.176/0.255/0.010/0.592ms
  （run −51%/wall −27% 对 perf-a6，A/B 消融证明与改动无关——A6 报告自述
  「日常开发机（绝对值仅供参考）」）。**本批工作基线 = 上述 HEAD 实测**；
  各片以「对 HEAD~1 的 A/B 消融」为主要判据（P1 先例：P1 vs HEAD 差 ≈ 0），
  perf-a6 作历史参照。
- 预期方向：P1/P2 ≈ 零变化（Arc 化或令 settle 相微降，噪声内）；
  P3 project 相下降（地形重建+序列化消失——归因写入报告：地形字节占比前后
  对照）；run 相可能受益（JS parse 变小——方向有利仍须记录幅度）；P5 按判据。
- 任何相 p50 变化 >20%：先做 HEAD A/B 消融区分「改动效应 vs 机器态漂移」，
  归因落档；不可归因的回退。
- perf-probe 产物入 compat/reports/ 的按既有惯例处理（报告文件 mtime 类
  diff 还原纪律同 kkscreeps.json）。

## 6. 风险与已知坑（本批专项）

1. **地形协议改动的锁步面**（P3，改前逐处 grep）：prelude game.js:543-548
   （重建点）、room.js:43-57 Terrain（get/getRawBuffer——Uint8Array 索引
   兼容已核）、game.js:392-402 getTerrainAt、room.js:91-98 terrainWordAt/
   look 面、bot_check.rs:246-258 home_world 夹具、world_json.rs 测试
   「地形面恒在」断言（:1019 一带）、sandbox/embed 测试中读 terrain 数组
   形态的断言（实现期 grep `terrain` 全测试面）。**P3 实现期定夺**：夹具
   不改——prelude 双形态收（对象=真路径 / 数组=宿主手搓夹具路径，
   game.js 两分支均有测试锁定），bot_check 与 sandbox 数组夹具原样保留。
2. **prelude 消费面 grep 纪律**：改字段名/形态先 grep prelude 全消费面
   （防错坑清单既有条目，本批 P3 是最高频触点）。
3. **宏化反「统一抽象」**：只收机械齐一样板；单源化逐处先读两侧契约；
   「两投影臂不宏化」是方案判断（§2），评审可挑战但须按 §2 标准举证。
4. **generation bump 遗漏**：engine 直接写 `world.rooms` 的提交尾必须显式
   bump（P2 评审专项：grep 全部 `&mut world` / `world.rooms` 可变面）；
   失败路径不 bump。
5. **RefCell 缓存**：World 的 Debug/Clone 派生随体；单线程持用前提在注释
   钉死；缓存键 = (epoch|generation) 失效正确性由 P3/P5 各自测试锁
   （改地形后注入必须变——P3 有正向测试钉；place 后对象缓存必须失效——
   P5 原有正向测试已随回退消失，未来重启对象缓存时须重写）。
6. **JS 预检门/层间测试**：P3 的 JS 侧改动须有 Rust 孪生重验（world_json
   形态测试 + sandbox_tests 断言同步），不得只改 JS。
7. **线上契约冻结**：tags 常量名+值逐字保持（宏化不改任何 tag 值——
   schema v1 冻结纪律）；snapshot.rs 契约形状（screeps-ok）零变化。
8. 工具链坑（B1 沉淀全表适用）：Bash cwd 跨调用持久（跑套件前显式 cd 主仓）、
   zsh `===X===` 分隔符当 glob 报错、ratchet 前显式 release 重建（旧二进制
   假零翻转）、跑完还原 kkscreeps.json、commit subject ≤350（先短主题+详
   body）、V8 测试并入既有串行链（本批新 V8 场景若有）。
9. **并行线**：f2-renderer worktree 隔离不并线；world_json/snapshot 形态
   改动（P3）在 main 落地后 F2 合并期对齐 exporter 消费。

## 7. 切片执行记录

> 段序=收口整理序（新片在上）；执行与提交序为 P1→P2→P3→P4→P5
> （P5 实测回退无代码入库）。

### P1（kk_room_object! 枚举管道宏 + object_owner 单源）——已收口

- 实测：room.rs 1116→929 行（8 块手写管道 → 一次声明：16 变体逐变体
  `= tags::X` 全路径 `$tag:path` 引用 + 编译期互异断言）；lib.rs 新宏
  kk_room_object!（含 set_id 新方法）；world.rs set_object_id（16 臂）消灭；
  world_json.rs object_owner pub 化（消费走全路径，re-export 定夺不做——
  P2-1 清偿）；settle/mod.rs object_owner_of 删除，notify_when_attacked 与
  damage.rs 废墟承接改经 kk_world::world_json::object_owner。
- 评审清偿：三路评审全 PASS 零 P0/P1。P2-1（评审 1：宏内裸 `tags::` 按
  调用点解析是隐式约定）清偿为**逐变体全路径声明**——评审建议的模块参数
  形态不可行（模块路径不能进 `<mod>::X` 限定路径——尖括号内是类型位，
  E0573；`$frag::ident` 在表达式位又非法），全路径 `$tag:path` 在表达式/
  match 模式/数组三位全部合法且契约逐变体锁定；P2-2/P2-3（评审 3：方案
  示例语法、§8 表格与实现不一致）已就实订正；F-1 即本段。
- 验证：workspace 268 全绿 / clippy 零告警 / release+棘轮 1405 零转红
  （清偿前后各一跑）/ 三基线通过（tooangel console error tick=0，清偿后
  复跑同样为零）；评审 2 独立探针实证互异断言编译期 E0080 生效、
  deterministic_bytes（kk-storage-bench 两次构建逐字节相等）过宏生成
  decode 路径。
- perf（A/B 消融）：P1 三跑 project 0.174-0.176 / run 0.252-0.256 /
  settle 0.010 / wall 0.587-0.593 ms，与 HEAD 原树同机器态实测
  （0.176/0.255/0.010/0.592）全部噪声内——**P1 perf 差 ≈ 0**。
- 登记：A6 基线整机漂移（run −51%/wall −27% 对 perf-a6.md，消融证明与
  改动无关——perf-a6 自述「绝对值仅供参考」；本批工作基线改今日 HEAD
  实测，见 §5）；`[`RoomObject::tag`]` doc 链接指向私有方法系 HEAD 既有
  状况（rustdoc 警告集前后逐条相同）；ratchet「豁免条目未命中」噪音为
  先前批次遗留（归棘轮清理项）。

### P5（按代际的对象投影共享缓存）——实测回退（按预登记判据）

- 实现（已完整落地并跑通全测）：World.object_projection_cache
  （RefCell<(generation, BTreeMap<room, Vec<(ObjId, Value)>>)>，单键代际
  作废）；world_json_impl 逐房 get-or-build，per-viewer 组装 zip 对齐
  clone；失效测试（place 后新对象必现 / tick 后时间折叠面重折）过。
- **实测（判据触发回退）**：2 玩家 project 0.102→0.210/0.209/0.211
  （**+105~107%**）、wall +25%；4 玩家 project 0.215→0.590（2.7 倍）。
  run/settle 相零变化（确认效应隔离在投影内）。三跑稳定非机器态
  （金丝雀 settle 相三跑同为 0.010）。
- 归因（收口评审 1 订正：原记「分配成本≫」在 serde_json 实现层面不成立
  ——json! 构造与 Value::clone 堆分配同阶，见 macros.rs 展开）：真实机制
  是**读侧局部性与表示成本**——clone 追逐缓存驻留的分散堆上 Value 树
  （指针追逐/cache miss）且工作集翻倍，而 json! 从连续 typed 载荷直构；
  外加组装双重结构（缓存形状→最终形状）。Value 级共享结构性不回本；
  兑现共享收益须换共享友好表示（列式布局=连续布局恰好同时治局部性/
  rev 键控载荷旁路/引用可共享结构）——「列式由测量决定」的直接数据点
  （注意：勿据此走 arena-Value 等仍含树遍历克隆的路线）。
- 处置：整体回退（git checkout 三文件，44 测试回基线集；回退后 perf
  精确复原 0.103/0.154/0.010/0.413）。无代码入库；本段与 §3.5 结论
  即该片的交付（判据预登记在先，负结果照登）。

### P4（投影 helper 单源化 + 两文件测试拆出 <1000）——已收口

- 实测：三对真重复单源化——store 家族（store_positive_entries 正量键集
  + apply_store_extra 共核，world_json 侧包装层显式补 energy 恒投影键；
  snap_store/snap_store_extra 改消费）、controller_progress_total_opt
  （Option 共核，快照 Null/世界 0 各自折叠）、controller_owner_value
  （三态 Some/Null/None，两侧手写 if/else-if 与互指注释消灭）。测试拆出子模块
  （源文件 847/392）；world_json 侧拆出后又经收口终审再拆——P4 清偿
  补入的锁定测试使单文件长到 1030 行（>1000，且 §7 原记 920 为拆分
  时点数字未随清偿回更——终审 P1 抓到），再拆为 tests.rs（46，共享
  helper/uses）+ tests/terrain_and_projection_forms.rs（293）+
  tests/viewer_semantics_and_families.rs（705）；snapshot/tests.rs
  821（清偿测试补入后仍 <1000）。B1 留置的两源文件 <1000 达成；测试
  文件同口径无 >1000。
- 评审三路全 PASS 零 P0/P1：等价性逐输入类推演（store 四输入类键集值
  逐一相同 + serde_json 无 preserve_order 键序恒字典序、progress 的
  level=0 表无 "0" 键疑点排除、owner (Some,true) 态生产不可达且优先序
  一致）；测试拆分 token 级 SHA256 双双一致。
- 评审清偿：结算侧 controller_levels 两份查表拷贝收编（评审 3 勘测盲区
  ——economy.rs/creep controller.rs 与共核逐字符同构，CONTROLLER_LEVELS
  表仅含 "1".."7" 故无守卫旧版与 opt 折叠全域等价；pub 化共核消费）；
  三组输入类锁定测试（(E=0,G>0) 组装序重组面/零量 extra 栈排除/
  progress 0-2-8 级两投影折叠——评审 1 P2 ×3）；tests.rs 头注释口径
  订正（token 流一致 + rustfmt 缩进/合行机械重排——非逐字节，评审 2
  P1-1）；本段与 §1.4 补录。
- 验证：workspace 276 全绿 / clippy 零告警 / release+棘轮 1405 零转红 /
  三基线通过（tooangel console error tick=0）。
- perf：与 P3 基准持平（0.103/0.153/0.010/0.416 vs 0.101/0.152/0.010/
  0.413——纯函数边界重划，评审逻辑核验通过）。
- 登记（评审 3 确认的刻意分歧，非漏网）：snap_pos vs world_json 尾部
  pos 组装（输入域/覆盖面不同）、remaining 家族 vs 内联时间折叠（两契约
  折叠形态系统性不同，各臂注释钉官方出处）；source/mineral 与 tombstone/
  ruin 的同构内联（3 行 ×2）抽象成本高于收益；kk-runtime embed 测试一处
  注释与断言不符（:605 声称 8 级 progressTotal 读 null 但无断言——归文档
  卫生）；B1 §1 的 room.rs 行数陈旧（1116→现 929）系后续批演化的文档
  漂移，备查。

### P3（地形按版本缓存：紧凑载荷 + rev 戳 + 双侧缓存）——已收口

- 实测：注入协议 `rooms[name].terrain` 从 2500 元素数字数组 →
  `{"rev": <terrain_epoch>, "payload": "<base64 2500B>"}`（~5.0KB →
  ~3.3KB）。Rust 侧 World.terrain_cache（RefCell<BTreeMap<String,
  (u64, Arc<str>)>>，键含纪元）缓存 b64——纪元内零重编码；自包含
  base64_encode（无新依赖）。JS 侧 game.js 加载期 b64 反查表 + b64ToBytes
  （非法字符 fail-fast，评审清偿）+ gw.terrainCache 跨 tick 复用解码产物
  （rev 命中零解码）；beginTick 双形态收（对象=真路径 / 数组=宿主手搓
  夹具路径——bot_check/sandbox 数组夹具原样保留，实现期定夺见 §6-1）。
- 玩家可见零变化：官方引擎内部地形本就是 Uint8Array 形态（utils.js:333
  checkTerrain 的 instanceof 分支）——解码产物与官方同型；Terrain.get/
  getRawBuffer/getTerrainAt/lookFor(LOOK_TERRAIN) 逐项官方对照一致
  （评审 3 三处源码抽查）。
- 验证：workspace 274 全绿 / clippy 零告警 / release+棘轮 1405 零转红
  （清偿前后各一跑）/ 三基线通过（tooangel console error tick=0 两跑）。
- 评审三路全 PASS 零代码缺陷。清偿：b64ToBytes 非法字符 throw；embed 级
  端到端值断言（terrain_compact_protocol_e2e——getTerrainAt/get/
  getRawBuffer 全链闭环，补 producer/consumer 之外的组合链缺口）；探针
  尾部多余 end_tick 删除；归因数字订正（~5.0KB 非 7.5KB——2500 单数字
  +2499 逗号；「Value::Number 分配」实为 ~80KB Vec 缓冲）；room_mut 文档
  钉子（禁写 terrain——纪元不随可变访问递增）；本段与 §6-1 订正。
- perf（A/B 消融，对 ef43f4a 工作基线）：project 0.176→0.102（**−42%**）、
  run 0.255→0.152（**−40%**）、settle 0.010（−2%，未动结算=噪声）、wall
  0.592→0.412（**−30%**）；两跑稳定；对父树 4f59c8b 实测口径为
  −43%/−42%/−32%（结论不变）。归因：project=地形 Value 数组重建消失
  （~80KB Vec 缓冲/房/玩家/tick）；run=JSON.parse 地形部分 2500 数字
  token → 1 字符串 token + 稳态零解码；三相咬合（节省 0.177ms vs wall
  节省 0.180ms，差 2%）。4 玩家缩放同向（project 0.215 vs perf-a6 0.517）。
  测量学注记：清偿后曾出现一次全相劣化离群跑（settle +28%——P3 未动
  结算，该相即机器负载金丝雀），复跑两遍精确复现正测数字；单跑离群
  以「未动相劣化」为号甄别。
- **Level 2 判定：不做，登记**。Level 1 后地形残成本 = 每 tick 每玩家
  每房 3.3KB 字符串三段传输拷贝（Value::String 构造→to_string→JS parse），
  相对 run 相主导（玩家代码执行）已非主项；Level 2 的 per-player last_sent
  跨侧状态复杂度暂不回本。——与 P5 段「兑现共享须换表示」登记同向归测量批。
- 登记：JS 侧 terrainCache 防跨世界 (room, rev) 撞键 = create_shard 同时
  重建沙箱+世界（约定性而非结构性——与 P2 的 Rust 侧「缓存驻 World」
  钉子对称；evict_idle 无生产调用面）；terrain_epoch 对 settle 直写回/
  room_mut 无绊线（当前结构性不可达——settle 对地形全只读、Arc fail-fast
  兜底）；terrainCache 普通对象的原型链病态键（房名宿主可控，与既有
  terrainByRoom 同款模式非回归）；b64 畸形输入两侧语义未锁定（Rust 孪生
  panic vs JS throw 后的形态差异——不可达路径）。

### P2（seam：WorldSource + 世界版本面 + Terrain Arc 化）——已收口

- 实测：kk-runtime 新模块 world_source.rs（`WorldSource` trait 八方法 +
  `impl WorldSource for kk_world::World`，UFCS 消歧固有同名方法）；
  embed.rs 五读取面改经 seam（execute_players 投影 + 四 getter）+ 玩家
  循环代际绊线（debug 断言经 `assert_bound` 闭包覆盖正常尾与两条轮中止
  路径——注入失败 `?` / InfraFailure bail）；World 增 generation/
  terrain_epoch（bump 机制见 §3.2 订正）+ RoomVersionHandle + room_version
  （map key 权威——快照解码产出 name="" 的房，评审 F-1）；Terrain 内表示
  Box→Arc（normalize/as_mut_slice 经 Arc::get_mut，共享态 fail-fast）；
  snapshot-contract.md 头部加现状标注（typed 层代际已起步、字节形态仍是
  目标设计——复审持续纪律的目标/已实现分层）。
- 评审三路全 PASS 零 P0/P1（seam 形状与 bump 完备性 / Arc panic 可达性
  穷举证明生产不可达 / embed 行为等价逐点）。清偿：断言闭包覆盖两条中止
  路径；world 测试补 set_terrain 成功/remove/get_mut/room_mut 逐面 bump
  钉；engine 级「一次成功 tick ⇒ generation 恰 +1、失败 +0」测试；
  seam 委托等价测试（&dyn vs 固有逐值）；terrain 共享态 #[should_panic]
  契约测试 ×2 + Weak 注释；room_version 用 map key；模块 doc「全部读取
  面」收窄为「沙箱/玩家面」（宿主建片/放置写面不在范畴）；契约标注措辞
  精确化；§3.2 机制订正（本段随附）。
- 验证：workspace 273 全绿 / clippy 零告警 / release+棘轮 1405 零转红 /
  三基线通过（tooangel console error tick=0）。
- perf（A/B）：P2 vs 工作基线 project 0.178(+1%) / run 0.264(+3%) /
  settle 0.010(+4%) / wall 0.604(+2%)——全部噪声内（预期零变化达成；
  Arc 化的结算克隆节省在 settle 相 <1% 量级内不可分辨）。
- 登记：`World::reset` 归零计数器的跨分片撞键防御 = 「版本键缓存必须驻
  World 内」钉子（reset doc + 本段；P3/P5 实现以此为前提）；pub
  `rooms`/`time` 字段直写是结构性敞口（bench fixtures 直写 time 构造期
  无害），bump_generation 是唯一阀门；未来结算内若引入地形变异将命中
  Arc 共享态 fail-fast（设计意图，评审 2 P2-1 纪律登记）；embed 建片/
  放置路径直读 world.time 与 seam 双轨并存（非玩家面，收窄后 doc 已如实）。




## 8. 附录：新增对象类型同步点全表（2026-09-30 勘测，M2B 扩面登记资产）

| # | 层 | 位置 | 形态 | 本批处置 |
|---|---|---|---|---|
| 1 | schema | room.rs kk_schema! 结构声明 | 数据表 | 保留（声明面本就单点） |
| 2-9 | schema | room.rs:389-625 枚举管道 8 块（enum/id/pos/set_pos/enc/dec/tags/tag） | 16 臂机械 match | **P1 宏化**（7 块宏生成；tags 表保持手写显式数据面——§3.4 定夺） |
| 10 | world | world.rs:103 set_object_id | 16 臂机械 match | **P1 消灭**（宏顺产 set_id） |
| 11 | place | place.rs:36-60 kind 直通道 match | 语义臂（缺省面折算） | 保留 |
| 12 | place | place_structure.rs:18-247 structureType match | 语义臂 | 保留 |
| 13 | place | place_structure.rs:259 generic_structure_hits 表 | 数据表（已与 settle 单源） | 保留 |
| 14 | 投影 | world_json.rs:306-641 object_json 臂 | 契约臂（prelude 形状） | 保留（§2 判断） |
| 15 | 投影 | world_json.rs:700-765 matches_find | 守卫 if 链 | 保留（JS 镜像由层间测试锁） |
| 16 | 投影 | snapshot.rs:180-398 snapshot_json 臂 | 契约臂（screeps-ok 形状） | 保留 |
| 17 | 投影 | object_owner（world_json:194 + settle/mod:370 双份） | 同语义双拷贝 | **P1 单源** |
| 18 | world | world_json.rs:241-254 room_energy 聚合 | 语义 | 保留 |
| 19 | 常量 | kk-core constants.rs FIND/LOOK 表 | 数据表 | 保留 |
| 20 | 常量 | assets/constants.json（JS 面板） | 数据表（官方面板） | 保留 |
| 21 | prelude | _registry.js:129-266 三张类表+classGlobalFor | 数据表+switch | 保留（手写+测试锁） |
| 22 | prelude | assembler 注册（散布 6 文件） | 注册表 | 保留 |
| 23 | prelude | 原型字段表挂载（defineGameObjectProperties） | 对象字面量 | 保留 |
| 24 | prelude | room.js:264-303 find 数字 else-if 链 | #15 的 JS 镜像 | 保留（层间对表测试） |
| 25 | prelude | room.js:136-171 look 寄存器/归类 | if 链 | 保留 |
| 26 | prelude | game.js:249-285 buildGame 集合装配 | else-if 链 | 保留 |
| 27 | engine | movement.rs:399 type_name_of | 17 臂语义 match | 保留 |
| 28 | engine | settle/mod.rs:534-549 phase 8 分派 | 语义 match | 保留 |
| 29 | engine | creep/build.rs:140-246 complete_construction | #12 的镜像（字段构造未单源） | 保留（登记：与 place 镜像） |
| 30 | engine | damage.rs hits 三组 match + notify_when_attacked 写回臂 | 机械但承载 hits 语义 | 保留 |

宏化后新增类型在 Rust 侧的同步点：schema 声明（1，宏产 9 块）+ place 归一
臂 + 两投影各一臂 + matches_find 条目 + settle 各面——机械层收敛为声明一次，
契约层保持显式（编译器 exhaustive match 强制 + 测试锁定）。
