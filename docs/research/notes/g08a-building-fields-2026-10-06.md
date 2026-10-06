# G08.a 交付笔记：建筑自定义字段与生命周期 tick（2026-10-06）

登记基线 `75b92d9`（后接工具链合并提交 `e2a5d59`：单一 `pnpm run check` 入口）。目标：建筑从「纯配方数据」升级为「配方 + 自定义字段 + 自主生命周期」——字段声明即获玩家面直供与事务化写面，`tick(ctx)` 槽与生产同访执行，全部经冻结计划进入身份。

## 交付内容

1. **声明面**（kk-mod-api/host）：`BuildingDescriptor` 增 `fields: Vec<FieldDescriptor>` 与 `lifecycle_slots: BTreeSet<Key>`。SDK `buildings.define` 新成员 `fields`（与 objects.define 同形 fieldOf：name 禁点号/冒号，schema/default/visibility）与 `tick(ctx)`（函数即声明；槽/绑定从建筑键派生 `{key}.tick`/`{key}.tick.process`，ObjectTick 阶段，手写其他值拒绝）。可见性首片只支持 `public`（SDK 与 convert 双侧拒绝）。冻结层校验字段键/绑定归属、schema 嵌套、默认值 schema；字段表按键稳定排序并参与 PlanHash（默认值/可见性/槽集变化均改身份）。目录门（`BuildingCatalog::from_plan`）：字段键与建筑键同命名空间、建筑内唯一、玩家类名合法且唯一。
2. **存储与写面**：字段存于 Generic.data，**存储键为 `{ns}:{name}`（含冒号），与原生记录键结构上不相交**——原生键（无冒号）整比对只读，任何漂移拒绝（耐久/库存/归属不经此写面）；自定义键按字段表整集校验（缺字段/域外值/未声明键拒绝，与 Mod 对象 payload 同一 `check_object_payload` 入口，以去命名空间短名构造校验载荷后按存储键回写）。写面首触进链日志（`ChainMutation::ModUpdated`），拒绝随整轮候选丢弃；无字段建筑写面只读。完工落 `Value` 默认（`Missing` 不写键，与对象域 prepare_fields 同口径）。
3. **分派**：room_loop Generic 分支新增 `mod-building` 类（目录命中且声明了生产或 tick）：**先生产后 tick**，tick 读到本轮生产后的候选值。tick 经 `contrib.run_ext` 走 Extension 槽（与对象 tick 同机制）；`Declined` 升格为故障（生命周期槽无正常拒绝面）。
4. **玩家面六层直供**：`SurfaceBuilding.fields`（存储键表）→ `ModPlayerApi.buildings` 四元组 → canary/真实玩家 manifest JSON → `41_mod_player.js` `gw.modApi.buildings[key].fields` → 结构装配器为字段键定义实例级 getter（与内建字段同一 `rawData` 回查机制，两投影通道同源）→ 玩家经 `gauge['bl:heat']` 读值，绑定类可加语义糖（`get heat()`）。记录缺键读到 `undefined`。
5. **门禁**：install.rs 三门（槽挂载、check_object_lifecycles、check_intent_centers）扩展到建筑目录；两条安装路径把字段绑定并入 `installed`。
6. **typegen 同步**：`mod types` 声明生成为有字段的建筑输出 `{构造器}Fields` 接口（存储键成员；`Missing` 默认为可选成员），构造器符号经 Symbols 防撞分配（typegen/tests.rs 夹具同步并断言）。
7. **停用语义定案**：停用包后 tick（Mod 槽）随停用掩码停止、字段停在前值成惰性数据；**生产是 plane 上的 host 规则，与资源/仓储/射程规则同口径存续**（停用不改写已冻结 plane），库存照常推进——运行时测试钉死。

## 验收（全部实际执行）

- **引擎单测**（`settle::buildings::tests`，3）：写面原生键漂移拒/缺字段拒/域外拒/合法写合并（原生键保持）；未声明键点名拒/link 非目录目标拒/无字段建筑只读；同建筑重复字段键目录门拒。
- **引擎集成**（`settle_tests/building_lifecycle.rs`，3）：完工写声明默认值（Missing 不落键）；先产后 tick（ExtSimple 观察序列 [5,10] / tick_only [0,0]）；生命周期门三形态（槽阶段错/绑定未挂/键不派生）拒绝且 plan_hash 不变。
- **冻结**（`freeze_cases/buildings.rs` 新增 2）：字段归属/默认值 schema/槽归属/嵌套限额；字段排列不改身份、默认值/可见性/槽集改身份。
- **typegen**（`kk-mod-api --lib typegen`）：建筑字段接口映射与确定性（`GateConstructorFields` + 存储键成员断言）。
- **运行时**（`mod_chain_tests/mod_building_fields.rs`，4 × 双通道）：tick 整读写 + 玩家 Memory 直供 + 先产后 tick 观察序；原生键漂移整轮失败（生产一起回滚）；decline 故障；停用后 tick 停/生产续（见交付内容第 6 条）。
- **sim**（`sim_tests/building_fields.rs`，2 测试，97.78s）：`bl`（gauge 建筑：字段+生产+tick）与 `bk`（beacon 建筑：无生产纯 tick）两独立包 + 组合 + 无 Mod 原生对照四场景；末轮玩家 memory（heat/cycles=3、energy=15、cls 类名、beacon 翻转）与世界终态（第 4 轮值、冒号键在 final objects 断言）按表成立；四场景 × JSON/KKPV final 等价、4 个 plan_hash 互异；第 2 tick 保存检查点（time≥3 记录门使断言未满足、退出码 2）→ 加载续跑与直跑等价（字段终值跨进程存续，走 Generic.data 快照，无检查点 schema 新键）。
- 完整 `pnpm run check` 见提交记录。

## 实现期修正（自纠）

- **命名空间键自纠**：初版完工与写面经 `field_name()`（去命名空间短名）落键，与「冒号键不相交」设计自相矛盾（短名可与 `store` 等原生键相撞）；修正为存储键恒 `{ns}:{name}`，写面在目录校验（短名载荷）与存储（命名空间键）之间显式转换。
- **目录校验键序**：冻结排序后 `bl:cycles` 先于 `bl:heat`，缺字段断言先于域外值触发——测试按整集提交推演。
- **sim 夹具命名**：Mod id 即命名空间前缀，包目录由 gauge/beacon 更名 bl/bk 后通过。
- **plain 场景记录门**：无 time 门的对照场景在 2 tick 即满足断言（退出码 0），冷恢复保存轮失去「断言未满足」语义——main.js 统一 time≥3 记录。

## 复审记录

三路后台只读评审（引擎与冻结正确性、玩家面管道、夹具/sim/文档），无 P0，P1×3 + P2×十余条，全部清偿：

- **R1-P1-1（冻结层漏跑完整 schema）**：空枚举字段（`{type:'enum', values:[]}`）可冻结，运行期该字段任何写入恒拒 → 房间钉死在永久故障。修：freeze 第二遍建筑目录循环对每个字段跑完整 `validate::schema`（枚举非空、ref 指向已知类型等）；补空枚举/未知 ref 反例。
- **R1-P1-2 = R2-P1-1（可见性不变量只在转换层强制）**：直构 Rust 贡献声明 Owner/ServerOnly 字段可冻结并直供全体玩家。修：freeze 建筑字段循环拒绝非 public（Rust 直构与 JS 装载同门）；身份测试的 Owner 变体改为 schema 变体。
- **R1-P1-3 = R2-P2-2（proposed 新增无冒号键静默丢弃）**：与「任何漂移都拒绝」自述及 link/Mod payload 两个姊妹写面不一致。修：proposed 中不属于 current 原生键集的无冒号键点名拒绝；补单测反例。
- **R1-P2-1/2（freeze 权威层缺位）**：重复字段键、生命周期槽声明/派生/阶段上收到 freeze 第二遍（install 门保留为纵深防御）；字段绑定须在贡献 bindings 集（对象字段同口径）——该门立即抓出 typegen/集成测试夹具三处漏登记绑定，佐证有效。门分两侧后集成测试重构为 freeze 层三例（槽键派生/阶段/未声明槽）+ install 层一例（槽中心）。
- **R1-P2-3（完工默认值未规范化）**：Float 域整数默认在记录里保持整数 backing 直到首次 tick 写面。修：freeze 期对 Value 默认跑 `normalize_value`（对象域 prepare_fields 运行期做、建筑完工直接落键，冻结期一次做齐）；身份测试断言 `json!(1)` 冻结为 `json!(1.0)`。
- **R1-P2-4 = R2-P2-1（SurfaceBuilding/player_surface 死代码）**：清单构造在 setup.rs 直构，两处平行映射会漂移。修：删除 `SurfaceBuilding` 与 `player_surface()`（连带自身单测）。
- **R2-P2-3（运行时测试四缺口）**：补 toJSON 面（`JSON.stringify(g)` 含 `bl:heat`）、缺键读 `undefined`、停用后玩家读冻结前值、完工默认→玩家读成链（新增无 tick 的 `bl:meter` 对照建筑走建造流）。期间自纠两处：runtime 夹具 tick 体改防御性自增并整集补键（bare 建筑首写 NaN/缺 label 均会整轮失败——与 sim 夹具 `(x||0)` 写法对齐）。
- **R1-P2-5 / R3-P2（测试与文档小项）**：install 缺槽分支补例；恒真断言折叠；sim 注释两处口径修正（退出码 2 的真实依据是 time≥3 记录门使 memory 为空）；plain 场景补 `/objects/relay/store/energy` 断言；夹具 README 标注默认值落键由引擎测试钉死；架构文档 tick 示例补 `(x || 0)` 起步口径。
- **R3-P1-1（台账登记段未收尾）**：见下方收口段——按先例翻为已交付条目。
- **R3-P1-2（typegen 变动范围确认）**：`mod types` 建筑字段接口属本批玩家面范围（kk-mod-api 19 项全过，完整 check 覆盖新鲜度门）。

行数门：`working.rs`（1014）经建筑字段写面移入 `settle/buildings.rs` 域模块、`structures_spawn.js`（1012）经墓碑/废墟段拆出 `tombstones.js`（新 prelude 段，段计数 18→19）双双回落；`sdk.js` 981/1000 贴线，下批动 SDK 前先拆。

评审人另核通过：写面无原生键绕过路径、短名↔存储键单射、链日志时序与 `update_mod_fields` 同构、分派条件与先产后 tick、KKPV 通道 rawData 同源、canary 与真实玩家同源注入、六层键一致性、场景数值推演与实跑一致（评审机四场景实跑 + 检查点字节级取证）。

## 已知边界

- 可见性 owner/server-only 裁切未开放（freeze/SDK 双侧拒绝）；字段无玩家面写路径（写入只能来自服务端 tick）。
- 生命周期槽无正常拒绝面（decline 即故障）；无逐建筑相位状态（tick 无跳过/休眠语义）。
- 停用后生产存续是 plane 语义的既定口径（见交付内容第 6 条）；重启用/清理面不在本批。
- G08 剩余子片：逐建筑 RCL 配额、逐资源容量（`storeCapacityResource`）、建筑专属 FIND 常量、生产事件。
