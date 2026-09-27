# M1 引擎骨架开发方案：kk-core / kk-schema / 单房间结算 / 棘轮

> 状态：**待确认**（本方案经确认后动手；架构级结论定型时升格为 ADR-011，实现现状回写架构文档）。
> 依据：[roadmap M1](../roadmap.md)、[game-loop.md](../architecture/game-loop.md)、[overall.md](../architecture/overall.md)、
> ADR-004 / ADR-008（第 5/6/7/8 条）、[M0 评审台账](../../compat/reports/m0-code-review.md)。
> 官方源码引用均为本地 `~/Desktop/projects/sources/screeps/`（官方 engine/common 仓库）的 file:line。

## 1. 范围与验收

M1 交付四件事 + 欠账清偿：

1. `kk-core`：常量表生成、房间名编解码、地形编解码、带种子随机数；
2. `kk-schema`：房间快照的二进制编码格式（声明宏 + 版本升级 + 读写器）；
3. `kk-engine`：单房间结算原型——intent 模型、官方结算顺序对照、最小闭环
   （移动/采集/建造/升级/孵化），固定指令脚本驱动，Sandbox trait 假实现（ADR-008 第 5 条）；
4. screeps-ok 通过率棘轮进 CI（地板 237，回归即红）。

**验收关卡**（roadmap）：基线确立并在 CI 棘轮运行。
**明确不在 M1**：战斗结算全家（attack/heal/ranged*）、power、市场、跨房间阶段（creep 出房间落地）、
NPC（keeper/invader）、真沙箱接引擎（M2-2A）、玩家 API 层语义对齐（M2-2A）、存储引擎（M2-2B，ADR-008 第 7 条）、
官方地形生成算法（M4，ADR-008 第 6 条）。

## 2. 关键决策（需确认的放在最前）

### D1（最重要，需确认）：embed 路径切换到 typed world + 真结算

`EmbedEngine`（screeps-ok 适配器与 sim 共用的嵌入式 API，ADR-008 第 8 条）的内部世界从
`kk-world::DumbWorld`（M0 哑世界，JSON Value 存储）切换为新的 typed world（kk-world 升级，见 §5）：

- `createShard / placeObject / setTerrain` → 归一化为 typed 对象（**这就是台账"typed placement 精确初态"
  欠账的清偿位置**：每类型一张缺省表，对齐官方对象初态）；
- `tick(n)` / `runPlayer(s)` 的 +1 tick → 跑**真实结算**，intents 为空（玩家 intent 收集通道属 M2-2A，
  M1 的 prelude 仍是无 intent 的桩环境）；
- `runPlayer` 的世界投影保持 M0 的"世界 JSON"形状不变（prelude 契约零改动）。

理由：

- 台账 M1 项本身就预设了这个形态——正确性-7/8（terrain 校验"按 `terrain[y*50+x]` 读取时"）、
  第四轮 typed placement（"属快照语义，与 M1 引擎结算同批做"）都只在结算读世界时才成立；
- 不切换的话，M1 引擎结算与任何用户可见路径完全断开，棘轮数字仍是哑世界数字，M2 接入时还要
  重做一遍放置归一化；
- 棘轮地板 237 兜底行为漂移（回归即红、修复后重跑更新地板），全量基线重跑归因是既有纪律。

备选（不推荐）：embed 保持 DumbWorld，引擎结算只由单测证明。省一批归因工作，但 M1 的"手工摆房 +
预生成地形"就名存实亡，且欠账正确性-7/8/typed-placement 无法真正清偿。

风险与对策见 §11。

### D2：kk-schema = 精简版声明宏 + 版本升级（xxscreeps 思路，Rust 编译期）

参照 xxscreeps `packages/xxscreeps/schema/`（声明式 format：struct/variant/optional/array/primitive →
编译期 layout → BufferView 读写），在 Rust 里用声明宏 `schema!` 实现，**M1 只实现 M1 对象集需要的子集**：
定长整数/bool/enum（u8 标签）/string（len+utf8）/optional（_presence flag + inline payload）/
variant（tag 前缀多态）/vector（len 前缀）。指针区、对齐技巧、kaitai 导出等 M2 需要时再加。
宏展开直接生成：typed struct 定义、`encode(&self, &mut Vec<u8>)`、`decode(&[u8]) -> Result<Self>`、
以及每字段 offset/大小的 `const`（编译期布局计算，兑现 overall.md 的承诺）。
版本：文件头 `{magic, version}` + 版本升级函数链（`v1_to_v2`），旧版本读入即升级。

### D3：kk-core 常量从官方 JSON 资产生成，不手抄

`crates/kk-runner/assets/constants.json` 已是官方 `common/lib/constants.js` 的全量 dump（423 键，M0 建立）。
新增 `scripts/gen-constants.mjs`：读该 JSON → 生成 `crates/kk-core/src/constants.rs`（提交入库，头部注明
生成命令；CI 新鲜度校验按 roadmap 排 M3）。生成覆盖全部键，M1 消费子集（TERRAIN_MASK_*、
HARVEST_POWER/BUILD_POWER/UPGRADE_CONTROLLER_POWER、CARRY_CAPACITY、CREEP_LIFE_TIME/CREEP_SPAWN_TIME/
MAX_CREEP_SIZE、ENERGY_REGEN_TIME、SOURCE_ENERGY_*、CONTROLLER_LEVELS/DOWNGRADE/DOWNGRADE_RESTORE/
MAX_UPGRADE_PER_TICK、CONSTRUCTION_COST、BODYPART_COST/BODYPARTS_ALL、OBSTACLE_OBJECT_TYPES、
ROAD_*、SPAWN_ENERGY_CAPACITY 等）。哑世界/embed 里手抄的常量（dumb.rs 的 find 表、embed.rs 的
CONTROLLER_HITS）随迁移改引 kk-core。

### D4：结算模型 = 官方 processor.js 顺序的忠实子集 + 确定性替换

单房间 `process_room` 按官方 `engine/src/processor.js:16-520` 的阶段顺序实现（详见 §6 对照表）。
与官方的三处**有意差异**（确定性/内存模型要求，逐条登记灰区）：

1. **对象迭代序**：官方遍历 `roomObjects`（JS 对象，取自 DB 的插入序 ≈ 创建序）；我们用
   `BTreeMap<ObjId, _>`，ObjId 由世界单调分配——创建序 = id 升序，与官方近似一致，且天然确定
   （game-loop.md 关键设计二）。若 screeps-ok 出现顺序敏感用例再对照取证。
2. **bulk 写入**：官方先攒 `bulk.update/insert/remove` 末尾统一执行（DB 批量写）；我们直接在
   工作副本上变更（内存模型无写放大问题），tick 末产出 eventLog。变更集 diff（网络推送用）M3 再加。
3. **失败回退**：官方出错打日志继续（processor.js:579）；我们失败即丢弃工作副本、房间保持上一 tick
   状态（ADR-008 第 3 条，可靠性升级，玩家不可感知）。

### D5：Sandbox trait 定型为"世界视图进、intent 批出"

```rust
pub trait Sandbox {
    /// 给定当前世界视图与玩家，收集本 tick 的 intent 批（已过 system.js 语义白名单清洗）。
    fn collect_intents(&mut self, view: &WorldView, player: PlayerId) -> IntentBatch;
}
```

M1 一个假实现 `ScriptedSandbox`（固定指令脚本：按 tick 序回放预置 intent）。真实现（prelude 收集
intent 后经 endTick 通道传出，二进制编码）是 M2-2A；trait 形状按它设计。tick 内多玩家 intent 的
结算顺序按玩家名排序（game-loop.md：指令结算顺序按固定规则保证确定性；官方 processor.js:229
`_.forEach(intents.users, ...)` 的顺序来自 DB 查询，我们用排序序替换，同 §D4-1）。

### D6：棘轮 = 独立 floor 文件 + CI 第四 job，push 即跑全量

- `compat/reports/ratchet.json`：`{"floor": 237}`——唯一真相源，基线报告 JSON 只是产物；
- `compat/scripts/ratchet.mjs`：调 `run-suite.sh --quiet` 跑全量 → 解析 `kkscreeps.json` →
  `passed < floor` ⇒ exit 1（打印差值与最近变红用例）；`passed > floor` ⇒ exit 0 并提示上调地板；
  `numTotalTests` 漂移（上游 pin 固定，理论不可能）打印警告；
- CI 新 job `ratchet`（ubuntu，node24 + pnpm + rust 1.98.1）：`cargo build --release -p kk-cli` →
  `scripts/setup-screeps-ok.sh` → compat build → ratchet.mjs。timeout 40 分钟，push/PR 都跑。
  本地等价：`node compat/scripts/ratchet.mjs`（macOS 无 timeout 命令，脚本内部不用它）。

RPC 超时（工具链-4）作为前置同时修：sidecar 挂死时 CI 由卡死变为超时红。

## 3. kk-core 模块划分

```
kk-core/src/
├── lib.rs
├── constants.rs      # 生成物：官方 constants.js 全量（scripts/gen-constants.mjs）
├── room_name.rs      # RoomName 新类型：解析/生成/xx-yy 编解码
├── terrain.rs        # Terrain：2500 字节平面 + TERRAIN_MASK_* 位语义
└── rng.rs            # SeededRng（PCG32）；seed 存世界数据，M1 结算暂不消费
```

- `room_name.rs`：移植官方 `utils.js:358-390`（`roomNameToXY`/`getRoomNameFromXY`），含官方解析细节：
  `parseInt(name.substr(1))`、`xx>=100 → verticalPos=4 / >=10 → 3`、W/N 取负减一、大小写混认。
  `RoomName` 构造即校验（非法名 Err），编解码往返单测锁官方行为（含 `E10S0`、`w2n3` 小写、
  三位数房间名）。
- `terrain.rs`：
  - `Terrain([u8; 2500])`，`get(x,y) -> u8`、`is_wall/is_swamp`（对照 `utils.js:333-337 checkTerrain`
    的 `code & mask > 0` 语义）；
  - 解码两种输入形态：官方数组形态（embed setTerrain 的 `number[]`）与字符串形态
    （`checkTerrain` 的 `terrain.charAt` 分支；官方 DB 里地形历史上存数字串）；
  - **构造即校验**（清偿正确性-8）：长度必须 2500，元素非整数/越界（`& !0b1111 != 0`，合法位仅
    wall=1/swamp=2/lava=4 及其组合）拒绝而非静默截短；
  - **四角墙强制**（清偿正确性-7，官方地图生成器不变量）：`normalize()` 把四角写成 wall，
    createShard/setTerrain 入库前统一过它。
- `rng.rs`：PCG32（约 20 行）+ `WorldSeed`。M1 世界持有 seed（快照含之），结算路径暂无随机规则
  （掉落散布、NPC、stronghold 都在后续里程碑），先落地原语避免将来换 RNG 破坏回放。

## 4. kk-schema 模块划分

```
kk-schema/src/
├── lib.rs           # schema! 宏 + 编解码 trait
├── reader.rs        # Cursor 读取原语（u8/u16/u32/u64/i64/bool/string/len）
├── writer.rs        # 写入原语
└── room.rs          # 房间快照的 schema 声明（M1 对象集）
```

- `schema!` 语法（M1 子集，示例而非终态）：

```rust
schema! {
    version: 1,
    RoomSnapshot {
        time: u64,
        seed: u64,
        next_obj_id: u64,
        terrain: [u8; 2500],
        objects: Vec<RoomObject>,
    }
    variant RoomObject {
        0 => Creep { name: String, owner: String, x: u8, y: u8,
                     body: Vec<BodyPart>, fatigue: u32, age_time: u64,
                     store: Store, hits: u32 },
        1 => Source { energy: u32, energy_capacity: u32,
                      next_regeneration_time: Option<u64> },
        2 => Controller { level: u8, progress: u64, owner: Option<String>,
                          downgrade_time: Option<u64>, .. },
        3 => Spawn { .. }, 4 => Extension { .. }, 5 => ConstructedWall { .. },
        6 => ConstructionSite { .. }, 7 => Energy { .. }, 8 => Flag { .. },
    }
}
```

- 生成的 struct 即 kk-world 的内存类型来源（kk-world 依赖 kk-schema，方向合规：world → schema → core）；
  字段布局常量（`Creep::OFFSET_x` 等）编译期算出，供 M2 共享内存直读使用；
- 版本升级：`decode` 读到旧版本 → 依次过升级函数 → 内存为当前版本；测试锁"v1 旧字节 → 升级 →
  再编码"的往返；
- M1 的消费方式：engine 的 tick 快照导出（`kkscreeps export` 类文件）+ 单测往返。结算本身操作
  内存 struct，不经过二进制层。

**为什么 M1 就做 schema 而不是等 M2**：任务定的优先级（类型是结算的地基）——typed world 的字段
集先在 schema 里声明，world/engine 按它实现，M2 的存储与共享内存直读不再重排字段。

## 5. kk-world 升级：typed world + place 归一化

```
kk-world/src/
├── lib.rs
├── snapshot.rs      # RoomSnapshot（来自 schema! 的类型）+ ObjId 分配
├── index.rs         # 50×50 格子索引 + 按类型分桶（ADR-004 第 2 条）
├── place.rs         # place spec（screeps-ok JSON）→ typed 对象的归一化表
└── world_json.rs    # typed world → prelude "世界 JSON" 投影（M0 契约形状不变）
```

- `place.rs` 归一化表（清偿"typed placement 精确初态与快照规范化"欠账），初态对齐官方对象全字段：
  - `source`：`energy = energy_capacity =` 依 controller 状态 3000/1500（官方 sources/tick.js:38-46
    每 tick 归一，放错首 tick 自愈，但放置时就给对）；
  - `constructionSite`：`progress_total = CONSTRUCTION_COST[structureType]`；
  - `constructedWall`：`hits` 缺省 `WALL_HITS`（官方常量 1？——注意 `WALL_HITS=1` 是旧常量，
    新墙血量官方为 `WALL_HITS_MAX` 语义，实现时以 constants.json 实值 + build.js 新建分支取证）；
  - `creep`：`age_time = time + (ticksToLive ?? CREEP_LIFE_TIME)`——**跟 vanilla
    放置语义**（claim 区分寿命是引擎孵化的 ageTime 缺失分支，放置路径不适用；
    三方评审裁决，2026-09-28）、body 元素 `{type, hits:100}`；
  - `boosts` 等 spec 私有字段不泄入快照（第四轮登记项）。
- `world_json.rs`：投影保持 M0 形状（`{time, cpu, rooms, objects}`，对象字段名=prelude 约定）——
  runPlayer 路径零改动；
- `DumbWorld` 删除，其 2 个测试随迁 typed world（place/find、terrain guard）。
  dumb.rs 的 find 常量表由 kk-core 常量取代。

## 6. kk-engine：结算模型（官方顺序对照）

### 6.1 intent 模型

- `intent.rs`：`Intent` enum，M1 最小集：

  | intent | 官方处理器 | 官方白名单字段（system.js） |
  |---|---|---|
  | `move` | creeps/move.js | `id, direction` |
  | `harvest` | creeps/harvest.js | `id` |
  | `build` | creeps/build.js | `id, x, y` |
  | `upgradeController` | creeps/upgradeController.js | `id` |
  | `transfer` | creeps/transfer.js | `id, amount, resourceType` |
  | `withdraw` | creeps/withdraw.js | `id, amount, resourceType` |
  | `drop` | creeps/drop.js | `amount, resourceType` |
  | `pickup` | creeps/pickup.js | `id` |
  | `createCreep` | spawns/create-creep.js | `name, body, energyStructures, directions` |
  | `createConstructionSite` | room/intents.js | `roomName, x, y, structureType, name` |
  | `createFlag` | room/intents.js | `roomName, x, y, name, color, secondaryColor` |

- **sanitize 层**（对照 system.js transforms，:7-15）：`number → parseInt` 截断、`bodypart[]` 过滤
  非法部件、`userString` 截 100、未知 intent 名/未知字段丢弃——沙箱传来的数据一律视为不可信
  （game-loop.md"玩家指令的流转"）。M1 的 ScriptedSandbox 产出已合法的 intent，sanitize 由
  入口统一跑（单测锁 transforms 行为）。
- **同对象冲突裁决**：对照 creeps/intents.js:3-17 的 priorities 图——`checkPriorities`：动作执行当且仅当
  其 intent 存在且**无更高优先级 intent 存在**（如 harvest 被 attack/build/repair/dismantle/
  attackController/rangedHeal/heal 任一压制；被压制的动作不执行，API 早已返回成功——官方语义）。
  执行顺序按 creepActions 数组序（intents.js:15-17）。M1 无战斗 intent，但 priorities 图按官方
  全图建表（压制判断需要认识全部动作名），M2 补执行器时零改动。
- **幂等/覆盖**：同一对象同名 intent 后者覆盖前者（game-loop.md：同一种指令只记最后一次）。

### 6.2 process_room 阶段序（M1 子集 ↔ 官方 processor.js）

| 阶段 | 官方 | M1 |
|---|---|---|
| 1. 预扫描：actionLog 重置、TTL 计算、分桶收集 | processor.js:43-171 | 做（actionLog 只留 M1 动作；keepers/invaders/nukes 桶跳过） |
| 2. NPC pretick（keeper/invader/invaderCore/nuke） | :176-219 | 不做（无 NPC） |
| 3. `_calc_spawns`（energyAvailable 初算） | :221-223 + intents/_calc_spawns.js | 做 |
| 4. `movement.init` | :225 | 做 |
| 5. 玩家 intents：按用户→对象遍历；`room` 级 intents（createConstructionSite/createFlag）先于对象级；所有权检查 | :227-322（:241 room、:248 `object.user != userId → skip`） | 做（用户序=玩家名排序，D5） |
| 6. `movement.check`（仲裁 + 级联移除） | :324 + movement.js:104-187 | 做 |
| 7. energyAvailable 重算 | :326 | 做 |
| 8. 逐对象 tick（energy→source→creep→spawn→…固定序） | :342-483 | 做 M1 类型：energy（衰减）、source（重生+容量归一 sources/tick.js）、creep（移动执行+TTL 死亡 creeps/tick.js）、spawn（孵化推进 spawns/tick.js + 出生 _born-creep.js）、constructionSite（超时）、controller（降级计时 controllers/tick.js） |
| 9. 落盘（bulk/eventLog/history/mapView） | :489-518 | 内存模型：直接变更；eventLog 产出留存；history/mapView 不做 |

### 6.3 movement 仲裁（movement.js 全套移植）

- `move` intent：方向→偏移（`getOffsetsByDirection`）、出界拒、目标格静态障碍预检
  （OBSTACLE_OBJECT_TYPES、非公开 rampart，move.js:33-37——注意：**creep 不在预检里**，creep 冲突
  由仲裁阶段处理，这是官方"挤格子"语义的根基）→ `movement.add` 入矩阵（movement.js:84-98，
  坐标钳 0..49）；
- `check`（movement.js:104-187）：目标格多 creep 竞争时按四键排序裁决
  （`rate1`=本格被多少 creep 盯上/被对穿者盯上=100、`rate2`=被拉、`rate3`=在拉、`rate4`=move/weight
  效率比）；胜者入 `objects`，败者出矩阵；随后逆序清链——`canMove` 不过（无 MOVE 部件或疲劳非零，
  movement.js:11-14 `_oldFatigue` 语义）或目标格静态障碍（含 safeMode 敌我判定，:16-39）则
  `removeFromMatrix` 级联清掉"要进我这格"的移动；
- `execute`（movement.js:189-252，creep tick 阶段调用）：落格、fatigue 结算
  （plain=2/swamp=10/road=1 × 非 move/carry 体重 + 资源重量 calcResourcesWeight:41-59）、
  道路磨损、进格踩掉敌方工地、edge 特例（从非边缘进边缘格 fatigue=0——跨房间预备，
  M1 单房间无 exit 落地，跨房间阶段 M2+）；
- **fatigue 恢复**：creep tick 末 `-2 × 活跃 MOVE 部件`（creeps/tick.js:105-108），
  `_oldFatigue` 用于本 tick canMove 判定（move 时记录，movement.js `!object._oldFatigue`）。

### 6.4 资源与经济闭环

- `harvest`（source 分支，harvest.js:29-70）：邻接 1 格；`harvestAmount = WORK 数 × HARVEST_POWER(2)`
  （无 boost，M1）；controller 非本人/非本预约拒；`amount = min(target.energy, harvestAmount)`；
  超容自动 drop；actionLog + eventLog；
- `source tick`（sources/tick.js）：`energy < capacity` 时挂 300 tick 重生钟（首个 tick 记
  `next_regeneration_time`，到点回满清钟）；`energy_capacity` 按 controller 归一
  （owned/reserved→3000，neutral→1500，无 controller→4000，:38-46）；
- `build`（build.js）：3 格范围；目标格静态障碍（含 safeMode 敌 creep 判定）拒；
  `build_effect = min(WORK×5, 剩余工程, 携带 energy)`；完成→工地移除+新结构落成（初态表对照
  build.js 的新建分支逐类型）；M1 落成类型限 spawn/extension/constructedWall/road/storage/
  constructionSite 全表照抄常量，其余类型落成字段 M2 补；
- `upgradeController`（upgradeController.js）：3 格；level 0/非本人拒；8 级每 tick 上限 15；
  升级条件含 downgradeTime 窗口（:63-66）；progress 进位、level+1、`downgrade_time = now +
  DOWNGRADE[new_level]/2`、gcl 计入玩家（M1 玩家档案存内存，persist M2）；
- `controller tick`（controllers/tick.js）：降级计时——超窗 level-1（progress 清零、通知），
  M1 做基础版（升级/降级计时/归零），safeMode 细节 M2；
- `transfer/withdraw/drop/pickup`：邻接/同格语义、capacity 双形态
  （`storeCapacityResource` 单资源槽 vs `storeCapacity` 总量，transfer.js:30-40 官方口径）、
  掉落 energy 对象合并（官方 drop → 同格 energy 叠加）；
- `createCreep`（create-creep.js）：body 裁 MAX_CREEP_SIZE、cost=BODYPART_COST 和、
  energyAvailable+spawn 能量双源扣费（_charge-energy）、`spawning{need_time=3×body.len, spawn_time}`
  状态、出生（spawns/tick.js:14-27 到点 → _born-creep.js 八方向找空位 + spawnstomp 敌 creep）、
  孵化中 creep 以 `spawning` 状态存在于 spawn 格。

### 6.5 双缓冲与失败回退

`settle_room(current: &RoomSnapshot, intents, view) -> Result<RoomSnapshot>`：clone 当前 → 结算 →
Ok 交换 / Err 丢弃（ADR-008 第 3 条）。M1 单房间对象量小（≤几十），clone 成本可忽略；
性能优化（Arena 复用）M2 基准时再看。

## 7. embed 路径切换（D1 的落地）

- `EmbedEngine.world: DumbWorld → kk_world::typed::World`；`createShard/placeObject/setTerrain`
  走 place.rs 归一化；`tick(n)`/`runPlayer` 的 +1 走 `settle_room(intents=空)`；
- `get_object/find_in_room/get_controller_pos`：typed → JSON 快照投影（保持 M0 字段名约定，
  snapshot_of 的 pos 组装逻辑随迁）；
- `createShard` 校验区清偿正确性-5：`controller:false` 且显式 `rcl/owner/safeMode` ⇒ 报错且不改状态
  （screeps-ok 契约）；
- prelude 侧配套修复（M0 登记的 M1 项）：`room.controller` null→undefined（正确性-6）、
  structureType→类映射补 `constructedWall`（JS语义-17）、`Room.getTerrain()/Terrain 对象/
  lookFor(LOOK_TERRAIN)`（第五轮地形保真度——tooangel 布局阶段的真实依赖）、跨房间
  getDirectionTo（官方 rooms.js 出口换算语义，实现时取证）、console 格式化对照官方 console.js
  逐参 toString（第三轮登记）。
- **prelude.js 拆分先行**（第七轮结构建议）：动 prelude 之前先做零行为变更的脚本分段提交
  （buffer-shim / cjs-loader / memory+world-projection / host-channel 四段独立 IIFE），
  之后的功能修复落在分段后的文件上。

## 8. 棘轮与 CI（D6 落地）

- CI job `ratchet` 步序：checkout → pnpm/node24/rust 1.98.1 → `cargo build --release -p kk-cli`
  → `bash scripts/setup-screeps-ok.sh`（pin 上游 commit，C++ 工具链 ubuntu 自带）
  → `pnpm --filter @kkscreeps/compat run build` → `node compat/scripts/ratchet.mjs`；
- 行为变化收尾纪律（既有，重申）：全量重跑 → 更新 `kkscreeps.json` 入库 → floor 只升不降 →
  README/roadmap/基线报告数字同步。

## 9. M1 欠账处置表（台账同步方案）

台账：`compat/reports/m0-code-review.md`"留待后续"表 + 各轮追记登记项。处置（修=本轮；
重登记=标明去向里程碑）：

| # | 登记项（出处） | 处置 |
|---|---|---|
| 1 | 并发-4 runExpr 无看门狗（留待表） | **修**：runExpr 通道按 run_player 同款 begin→arm→end 编排（引擎接入后为常驻通道） |
| 2 | 正确性-5 createShard 互斥校验（留待表） | **修**（§7） |
| 3 | 正确性-6 controller null→undefined（留待表） | **修**（§7 prelude） |
| 4 | 正确性-7 terrain 四角墙（留待表） | **修**（§3 Terrain::normalize） |
| 5 | 正确性-8 terrain 长度/整数校验（留待表） | **修**（§3 构造即校验） |
| 6 | JS语义-17 constructedWall 映射（留待表） | **修**（§7 prelude） |
| 7 | 工具链-4 RPC 无超时（留待表） | **修**：默认 60s 超时 + pending 拒绝（CI 棘轮前置） |
| 8 | 工具链-9 tick AbortSignal（留待表） | **修**：embed tick 有真实循环后每 tick 前检查 signal |
| 9 | 测试-4 bot-check 退出码（留待表） | **修**：阈值化（中断/缺失超限 exit 非零），为"每里程碑跑一次"铺路 |
| 10 | console 格式化（第三轮追记） | **修**（§7，对照官方 console.js） |
| 11 | has-trap 否定探测噪声（第三轮追记） | **修**：探测记录带"访问后否定"标记不计缺失（仪表增强，小改） |
| 12 | typed placement 初态/boosts 泄入（第四轮追记） | **修**（§5 place.rs 归一化表） |
| 13 | LIFO 进程级所有者与 evict_idle 逐出（第四轮追记） | **重登记 M2-2A**：需与多实例架构一起设计；M1 仍单实例 + 测试串行，现状（告警+断言零违规）够用 |
| 14 | bot 仪表重复行折叠 + 跨房间 getDirectionTo（第四轮追记） | **修**（bot_check 折叠统计；prelude 跨房间方向换算） |
| 15 | room.getTerrain()/Terrain/lookFor(LOOK_TERRAIN)（第五轮追记） | **修**（§7，M1 核心交付物之一） |
| 16 | serializePath/deserializePath、findPath 步进对象（第五轮追记） | **重登记 M2-2A**：属真寻路（kk-pathfinder 接入）与玩家 API 层范围 |
| 17 | prelude.js 拆分（第七轮结构建议） | **修**：M1 首个实现提交（零行为变更、独立提交，§7） |

M2 表 7 项维持原归属不动。

## 10. 测试计划

- kk-core：房间名编解码往返（官方行为用例）、Terrain 校验/四角墙/两形态解码、常量生成物抽查
  （对 constants.json 逐键断言一致性——生成脚本自带）；
- kk-schema：roundtrip（每类型随机样本）、版本升级链、布局常量与编码偏移一致性；
- kk-world：place 归一化表逐类型初态断言（对照官方 build.js 新建分支字段）、world_json 投影与
  M0 形状回归（用现有 prelude 契约测试）；
- kk-engine（重点，全部对照官方语义写断言）：
  - 移动：单 creep 平原/沼泽/道路 fatigue 数值、无 MOVE 不动、疲劳卡死、两 creep 抢格仲裁四键
    排序、对穿（互相走向对方原格=双方成功）、级联清链、edge 特例；
  - 采集：harvest 数值（WORK×2）、source 300 tick 重生、容量归一、超容 drop；
  - 建造：进度数值（WORK×5）、完成落成字段、障碍格拒建、能量不足减半；
  - 升级：progress 进位、8 级上限 15、降级计时触发 level-1；
  - 孵化：spawnTime=3×body、能量双源扣费、出生找位、spawnstomp；
  - intent 层：sanitize transforms 逐条、priorities 压制图、同对象覆盖；
  - ScriptedSandbox 驱动的多 tick 场景测试：挖矿→运回→升级控制器的完整闭环（每 tick 断言世界状态）；
- embed：现有测试全保留（形状契约不变）；新增 typed world 往返 + createShard 校验用例；
- 棘轮：本地跑一次全量出报告、floor 与基线一致后入库。

## 11. 风险与灰区预登记

| 风险 | 对策 |
|---|---|
| embed 切 typed world 引起基线漂移（投影字段缺漏→用例转红） | 棘轮地板 237 兜底；逐用例归因（沿用 worktree 对照法）；只接受净上升的合入 |
| 官方对象迭代序 = DB 插入序，我们 = id 序（placed 对象按 place 顺序分配 id，一致；官方结算中新建对象插入 roomObjects 尾部，同 tick 后续阶段可见——build.js 的 `roomObjects['createdStructure'+n]` 即此） | 结算中新对象也走"分配 id + 立即可见"语义；若有顺序敏感差异，取证后登记灰区 |
| `WALL_HITS=1` 等旧常量语义混淆 | 实现时以 build.js 新建分支 + constants.json 实值双重取证，不臆断 |
| schema! 宏复杂度膨胀 | 严守 M1 子集；缺能力时手写 impl 过渡，不扩宏 |
| CI 时长（ratchet job ≈ build 5min + suite 3min + setup 2min） | 可接受；若超 20min 再改 PR 触发为手动 |
| 灰区（预登记）：结算顺序差异（D4-1/2）、玩家序替换（D5）、failure 回退差异（D4-3，玩家不可感知方向） | 登记 compatibility.md 灰区表，screeps-ok 用例裁决 |

## 12. 实施顺序（提交切分）

1. prelude.js 拆分（零行为变更，独立提交）；
2. kk-core（常量生成 + 房间名 + 地形 + rng）+ kk-schema（宏 + M1 schema）；
3. kk-world typed world + place 归一化 + world_json 投影（DumbWorld 退役）；
4. kk-engine 结算（intent 层 → movement → 资源经济 → 孵化 → controller）+ ScriptedSandbox 测试；
5. embed 切换 + prelude 配套修复（欠账 #2/3/6/10/14/15）+ createShard 校验；
6. runExpr 看门狗 + RPC 超时 + AbortSignal + bot-check 退出码 + 仪表（欠账 #1/7/8/9/11）；
7. 棘轮脚本 + CI job + 基线全量重跑 + floor/文档同步；
8. ADR-011（本设计的架构级结论）+ 台账更新 + roadmap M1 收口。
