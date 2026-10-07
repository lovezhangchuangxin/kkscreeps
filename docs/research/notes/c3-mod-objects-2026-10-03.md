# C.3 counter 对象与动作全链（2026-10-03）

[计划](../../plans/archive/mod-plane-batches.md) C 批第三片。范围：Mod 声明类型
的对象进入世界存储、扩展意图从玩家面到结算的收集/校验/执行全链、
Mod 自有槽动作处理绑定的执行接线（前批被装配门显式拒的面）、
管理命令创建 counter 对象。**不含**：玩家 API 安装与投影扩展
（player.js/FIND/LOOK/玩家可见 counter——C.4）、wrap 续行（C.5）、
对象域外动作与队列合并等形态（装配期显式拒，见残留表）。

## 交付物

- **ModObj 世界存储**（kk-schema room.rs）：`RoomObject::Mod(ModObj
  {id, x, y, type_key, data})`，新标签 16（"新增类型使用未占用值"纪律，
  旧快照字节不受影响）。`data` 是已按类型描述符校验的完整字段 JSON
  （含挂载字段——Mod 类型的字段全部住 payload，`room.extensions` 只服务
  内建类型的追加字段，`ExtensionCatalog::from_plan` 相应收窄为 builtin
  类型）。信封 x/y 与声明的位置字段由写入口同步。二进制快照往返有测试
  锁定。
- **玩家面原语**（kk-runner prelude 40_ext_intents.js + game.js
  `setExt`）：`__kkEmitExtensionIntent(intentKey, targetId, args)` 把
  扩展意图记入 intentsList 的对象容器（`list[objId][intentKey]=args`），
  计费与对象动作同轨（0.2/键、同名覆盖只计一次）。**按沙箱配置门控
  装配**（`SandboxConfig.extension_intents`，默认 false）——vanilla 分片
  全局面零变化（全局面白名单守卫测试不改即过）；EmbedEngine 按计划
  是否含 Mod 意图置位，测试配置不可改写。载荷原样保留（含 null——
  nullable 参数 schema 的合法形态），原型不剥：stringify 只序列化自有
  属性，结构校验在结算侧。
- **收集/清洗**（kk-engine intent.rs）：`UserIntents.ext:
  BTreeMap<ObjId, BTreeMap<Key, Value>>`。`from_json` 对对象动作名先走
  vanilla 白名单，冒号形状的合法键进 ext 容器并绑定调用方玩家身份；
  键形非法丢弃。同对象同键后发覆盖（与对象动作一致）；`extend_from`
  同语义合并。
- **权威校验与派发**（settle/mod_intents.rs）：结算侧逐意图独立校验
  ——意图必须在冻结目录、参数满足描述符 schema（严格转换）、目标对象
  类型匹配接收者、玩家经归属字段授权（类型未声明 owner_field 时对玩家
  fail-closed）。任何一项不满足即丢弃且本轮照常结算（与未知 vanilla
  动作同待遇——玩家侧错误不炸世界）；全部通过才进槽处理链，链内失败
  （含 JS 异常）使候选失败。派发点在既有对象动作之后、按玩家与对象 ID
  排序。
- **自有槽执行链**（settle/contrib.rs）：第三执行器族
  `ExtSimple = Fn(&mut Working, ObjId, &Value) -> Result`；`ExtSlot`
  为 Process（仅动作处理中心）或 Chained（before → 中心 → after，
  中心=动作处理实现或 replace 处理器；wrap 非空即拒——同 link 槽纪律）。
  `ContributionDispatch::assemble_with_ext` 消费计划中全部非 link 槽；
  冻结后的 SlotPlan 不保留 native 绑定，中心按 Native 绑定/Chained
  中心推导；被 replace 占位而已声明的动作处理实现允许残余（区分装配
  键名错误）。
- **装配门**（settle/mod.rs `check_executable_intents`）：Mod 意图只放
  行对象域 + 末次覆盖合并 + 严格转换 + 玩家授权 + 无压制关系 + 槽中心
  是自身 process（显式入口或被占位即拒）；其余形态装配期显式拒绝并
  指明原因。`install_plan_with_ext` 就地装配（`install_plan` 保持三参
  形态转发）；Engine 常驻冻结计划（默认只含 builtin:link，OnceLock），
  Plane 增 `plan` 成员，意图描述符与投影类型经它解析。
- **桥与写回**（kk-runtime embed.rs + kk-mod-host sdk.js）：`dispatch`
  返回值增 `target`（ctx.target.fields 的终值；undefined/null 归一为
  null=不改动）；ext 桥闭包 ctx 携带
  `{tick, room, state, target:{id, fields}, args}`，结果整值写回
  state 与 target——target 为对象时经 `Working::update_mod_fields`
  按类型描述符校验（键集合与字段 schema 完全一致，**部分字段返回被
  拒**）并同步位置字段到信封坐标与位置索引。桥粒度沿用 JSON 整上下文
  （设计 §6.3 的选型决定见下"登记的简化"）。
- **管理命令**（embed.rs `place_mod_object`）：按已装配计划创建 Mod
  对象——类型在计划中、提供除带默认值外全部声明字段、未知字段拒绝、
  整体 schema 校验后写入；位置取 payload 声明字段。受 no_open_round
  门（失败轮/恢复态拒改世界）。诊断读面：`object_snapshot` 对 Mod 输出
  `{kind:"mod", typeKey, pos, fields}`（embed get_object 可断言）。
- **发布前校验**（`validate_mod_objects`）：候选房间内全部 Mod 对象
  逐个复核——类型在计划、payload 整体过 schema、声明位置字段与信封
  坐标同步（封堵绕过写入口的直改与失同步）；仅当计划声明 Mod 类型时
  启用（零 Mod 路径不进入该循环）。

## 验证

- workspace **509 测全绿**（B/C 批后 493 + 新增：kk-engine ext 收集
  单测 1 + 引擎级族 12 + kk-world 快照往返与投影排除 2 + kk-runner
  原语 1 + kk-mod-host target 往返 1 + embed counter 族 7 场景；含
  清偿复审后的扩面）；clippy `-D warnings`、fmt、doc-links 通过。
- **棘轮 1854/2914 零翻转**（转红 0、转绿 0，无新豁免）。
- **bot 三基线**：tooangel 2000t（2000 完成/0 中断/0 console-error tick）、
  overmind 2000t（2000/0/1——与 C.2 报告同形，既有形态）、wasm 10t，
  验收判定全过。
- **零 Mod 开销同机 A/B 双样（交错 h1/w1/h2/w2）**：HEAD（9aaa2df）与
  工作树各 2 样（8 玩家/300 tick/warmup 50，默认 JSON 通道）：
  project 0.550/0.554 vs 0.562/0.549、run 0.352/0.345 vs 0.345/0.339、
  settle 0.129/0.130 vs 0.129/0.127、wall 1.030/1.042 vs 1.052/1.024
  （ms，p50）——全相差 ±2% 内（含双向），**不可分辨**；≥5 样正式判读
  后置（与既往口径一致）。

## 与 C.2 残留项的对账

- 自有槽/动作处理绑定无执行器（C.2 残留 2）→ 本片接线（对象域）。
- 桥跨越 JSON 粒度 / ctx.objects（残留 3、7）→ 本片把 target 对象
  读写并入同一 JSON 整上下文通道（单目标）；多对象 ctx.objects 与
  fast op 通道仍后置（见下）。
- before-Err/replace-Err 引擎级专测缺口（残留 8）→ 补齐（引擎级族
  `link_slot_before_and_replace_failures_propagate`：before 中止中心、
  replace 传播失败，均断言已提交状态不动）。

## 评审与清偿（三路并行只读评审 + 清偿复审）

三路评审（Rust 引擎链路 / V8 沙箱与宿主桥 / 存储投影与兼容面）合计
**0×P0 + 1×P1 + 7×P2 + 若干 P3**，处置：

- **P1 装配门按槽命名空间豁免（引擎链路）**：Mod 意图挂 builtin 槽
  （freeze 允许、门跳过、ext 表无槽）→ 玩家可重复触发整轮失败。
  清偿：豁免改为按**意图键**命名空间（builtin 意图仍走 native），非
  builtin 意图全量过门，builtin 槽劫持在中心检查显式拒绝；结算侧另加
  builtin 槽防御丢弃（手搓批兜底）。
- **P2 mod_object_type 未滤 builtin**：`place_mod_object` 可造
  `type_key=builtin:link` 的 Mod 对象（P1 的零 Mod 放大器）。清偿：
  builtin 命名空间返回 None（builtin 类型有 Rust 权威表示，双表示
  本就非法），测试锁定。
- **P2 门严于执行器（replace 占位中心被拒而执行器/桥已支持）**：
  清偿：门放开"中心=自身 process **或**调用方提供的 Replace 处理器"，
  跨 Mod 替换动作处理可用（引擎级测试锁定 Replace 中心接管、原
  process 不跑且允许残余）；own-process 中心下多意图共享入口仍拒
  （Replace 中心下共享入口的各意图派发同一条已定义链，属有意放行）。
- **P2 长意图键使 install panic**：`{intent}.process` 派生键超 128
  字节上限走 `expect`。清偿：显式 Err；门矩阵测试锁定。
- **P2 动作处理实现残余校验过宽**：按"计划全部声明绑定"放行（字段/
  声明性绑定键下的杂项实现静默通过）。清偿：允许集收窄为"被 Replace
  占位的意图自身 process"，其余残余一律拒。
- **P2 Mod payload 裸名跨命名空间碰撞**（挂载引入，全键不同裸名相同 →
  同一 payload 键被两个 schema 约束）。清偿：freeze 在挂载并入后对
  非 builtin 类型做裸名唯一性检查。
- **P2 无意图槽的静默死执行面**：非 link 槽一律要求 process 但可能无
  派发点。清偿：门新增"槽必须被意图挂载"与"槽阶段须为对象动作阶段"
  两项（生命周期/其他阶段槽显式拒并指向后续批次）。
- **P2/3 桥装配时序**：ext_intents 标志移到 install 成功后置位（失败
  路径不再残留 true）。
- **P3 批清偿**：`LINK_SLOT_KEYS` 收敛为 engine 侧 pub 常量（embed 复
  用）；`setExt` 计费改按键存在性（null/假值载荷不重复计）+ 注释改为
  真实边界（继承 toJSON 可改写出站载荷，完整性由结算侧校验兜底）；
  `__kkEmitExtensionIntent` 的 undefined 参数归一 null（不再出站静默
  丢）；宿主 `count_billed` 对冒号键（扩展意图）按每键一次计（数组
  载荷不逐元素计）；`object_json_by_id` 对 Mod 返回 None（空对象 +
  造 pos 的残影记录消除）；`tags::MOD=16` 进 schema 标签钉死测试；
  kk-world 层投影排除直接测试（world_json 内部/玩家/FIND 全常量/
  KKPV 块四处 + matches_find 空记录兜底）+ 快照测试改用插入返回 id。
- **测试扩面（判别力缺口清偿）**：owner_field 缺省 fail-closed、双
  Mod 类型接收者不匹配、process 改位置字段的信封同步、扩展链
  before-Err 中止中心与 after、装配门矩阵（非严格转换/非玩家授权/
  压制关系/builtin 槽劫持/长键/阶段不符/无意图槽七类）、place_mod_object
  的 builtin 键与失败轮门、无 Mod 意图分片的原语缺席、原语防篡改
  （赋值/删除/描述符）。

**清偿复审**（只读复审 agent）12 项逐条复核：11 项闭合、无 P1/P2
级未闭合或新引入问题，结论可提交；三条 P3 建议已随后落实——freeze
裸名唯一性的锁定测试（挂载同名碰撞拒绝 + builtin 类型对照）、
`replaced_processes` 循环跳过 builtin 意图（与中心检查对齐）、本节
"多意图共享入口"口径修正。清偿后全量复跑：workspace 510 绿、
棘轮 1854 零翻转、bot 三基线同形（tooangel 2000/0/0、overmind
2000/0/1 既有、wasm 过）。

## 登记的简化与残留（后续承接）

1. **对象域外动作**（房间/玩家/世界域）、队列合并、非严格转换、非玩家
   授权、压制关系、显式入口：装配门显式拒；执行通道归后续片（SDK 面
   冻结时统一定论，同装载器残留 15 的口径）。
2. **ctx.objects 只有当前目标**：多对象读写/创建/删除、按字段 patch、
   fast op 通道未开——设计 §6.3 的实测选型沿用 JSON 整上下文（单目标
   够 counter 型负载），真实多对象 Mod 出现时按测量再选（C.5 性能
   矩阵一并看）。
3. **归属策略内建为 owner_field 相等**：描述符尚未声明授权策略字段
   （装载器残留 15 同源）；显式策略声明随 SDK 面冻结。
4. **Mod 对象不进玩家投影/FIND/LOOK**：world_json（内部全量与玩家
   视图）与 KKPV 私有块同口径跳过；查询快照走 object_snapshot 专用
   读面。玩家可见面（可见性裁切/FIND/玩家类）归 C.4 投影扩展。
5. **意图计费**：扩展意图按对象动作同轨 0.2；每 tick 意图计费上限/
   专属限额未实现（与 vanilla 通道同暴露，intentsList 50k 对象闸门
   兜底）。
6. **生命周期槽/能力面**（obstacle/damage/store 等）未接：Mod 对象
   不参与阻挡、寻路、耐久与 FIND；能力目录声明与消费归 D 批 slots。
7. **KKPV 计划指纹占位**（B.5 残留沿袭）：块仍绑
   KKPV_PROTOTYPE_PLAN_HASH 常量，未绑真实 PlanHash；Mod 计划变化
   不失效旧块——C.4 投影扩展接入真实指纹时一并收口。
8. **绑定键跨种类单射性**：`{intent}.process` 与
   `{ns}:{type}.field.{name}` 在特定点号命名下理论可撞（convert 层
   遗留）；本片中心分流按"replace 处理器优先于动作处理实现"消歧，
   撞键形态留给 SDK 面冻结时定论。
9. **place_mod_object 位置无位置字段类型落 (0,0)**：无声明位置字段的
   Mod 类型信封坐标固定 0,0（不进阻挡判定，无功能影响）；位置能力
   声明接入时定论。
10. **Mod 负载的每 tick 重解析成本未度量**：`validate_mod_objects`
    每 tick 对全部 Mod 对象重解析 payload 并全字段校验；`ext_bridge`
    每处理器调用重读一次 payload（一次 before/center/after 链 ≥3 次
    解析）。counter 规模无虞（A/B 零 Mod 不可分辨）；Mod 对象量级出现
    后实测，候选手段=写入口校验 + touched 集合复核（封堵面收窄须明示）
    或缓存解析结果。C.5 性能矩阵一并观测。
11. **扩展意图无视野/房间可见性门**：授权只看 owner_field（与 vanilla
    对象意图同口径——本引擎 vanilla 也无视野门；Mod 对象当前不进玩家
    投影，玩家正常拿不到 ID）。完整授权策略随 C.4 投影扩展定论。
