# 外审清偿：MiniMax-M3.1-Flash 复审 B2..C3（2026-10-03）

[计划](../../plans/archive/mod-plane-batches.md) C 批收口后的清偿片。范围：外部模型对
64f2d91..3140f63（B2..C3）的复审报告逐条对 HEAD 实测复核，证实项清偿、
已清偿项确认、非活漏洞项登记；**不含**处理器执行语义变更（分型/毒化按
用户拍板的方向登记排期，见文末）。

## 复核判定总表

| 报告发现 | 判定 | 处置 |
|---|---|---|
| P1 处理器异常炸全服 tick 且"每 tick 自动重试" | 链条证实；两处叙述修正：runtime 在恢复态**阻断**变更操作（宿主显式选择重试/放弃，非静默循环）；"重试同败"是 C.2 有测试钉住的有意语义 | 不改语义；运营危害与缓解排期登记（设计 §7 + 计划 D/E 批行） |
| P2 validate_* 按计划级布尔量 fail-open | 证实（`has_mod_types`/目录级 `extensions.is_empty()`） | 改按候选内容短路 |
| P2 object_json 为 Mod 造残影记录、2/5 调用点未过滤 | 证实；报告所列 shared_bundle.rs/snapshot.rs 两处违例已被 C.4 自然清偿 | object_json 对 Mod 返回 `None`，从类型上排除 |
| P2 ext 裁切 owner 双通道来源分叉 | 证实（JSON 读记录键、KKPV 读枚举派生） | 统一 `domain::object_owner` |
| P2 snapshot Mod 面零裁切 | 部分证实：C.4 已改写注释为管理面专用 | 设计契约句 + ServerOnly 钉住测试 |
| P3 create_shard 失败摧毁旧分片与注释不符 | 证实（C.4 canary 只前移失败点，未改销毁顺序） | 注释改真实语义 |
| P3 freeze 不拒 builtin 命名空间 | 证实但更窄：撞名拒绝已存在（m2b 有测试），缺口是保证依赖"调用方恰好同传 builtin::link()" | `freeze_with_builtin` 双入口，保留名保证收回 kk-mod-api |
| P3 move_obj 不维护 Mod 位置不变量 | 证实（当前仅 creep 路径调用，不可达） | 对 Mod 显式 Err |
| P3 注释规范违例（批次号入注释） | 证实；初盘 5 处，评审补获同范围漏网 2 处（kkpv_channel 的 B.6）与 C.4 引入的批次引用 3 处 | 共 10 处全部改写为陈述性文案 |
| §三 按槽判定两处 | 证实存在、零行为分歧（装配门已封 hijack） | 登记为后续结构化重构（SlotOwner 枚举），不属修复 |
| §四 A/B ≥5 样 | 非新发现（B.5/B.6 note 已登记"后置"） | 归 C.5 性能矩阵 |
| §四 4p kkpv C 批后无数据 | 属实（C 批 A/B 全为 8p 默认 JSON 通道，kkpv 双通道任何规模均无 C 批后数据；报告建议 8p 与 4p 各复测） | 归 C.5 性能矩阵 |
| §四 SharedKkpvBundle 靠不可达守护 | 属实（B.6 已做 HOSTILE 探针+双外泄修复） | 钉住测试登记 C.5 |
| §四 graph.rs expects 依赖前置校验 | 属实（pub API 手工构造会 panic） | Version newtype 登记装载器后续 |

报告交叉验证节的声称（workspace 510/棘轮零翻转/装配顺序/原语缺席）与
本仓记录一致；其自述的 ratchet 写盘事故经 `git status` 核实未留下改动。

## 清偿明细

- **object_json 残影记录（kk-world world_json/snapshot/kkpv）**：
  `object_json` 返回 `Option<Value>`，`RoomObject::Mod` 直接 `return None`，
  函数注释写明返回契约。五个调用点：world_json_impl/kkpv 两处在上文分流
  后 `expect`（防回归守卫）；`object_json_by_id` 直通 Option（原显式过滤
  删除）；`find_in_room`/`find_snapshots_in_room` 改 `filter_map` 结构性
  排除。原 791-793 的过时注释（"随投影扩展批次另行设计"）随契约改写消除。
- **ext 裁切 owner 同源（kk-world world_json）**：JSON 通道从投影记录读
  `owner` 键改为经权威枚举 `domain::object_owner`（与 KKPV 通道同一来源），
  注释写明两通道可见性判定必须同源的约束。
- **validate 改内容键控（kk-engine settle/mod）**：`validate_mod_objects`
  短路条件从计划布尔量 `has_mod_types` 改为候选内是否实际存在 Mod 对象；
  `validate_extensions` 从目录级 `extensions.is_empty()` 改为候选房间扩展
  条目全空。`has_mod_types` 字段及其计算移除。计划级布尔量不再是"封直改"
  防线的开关——快照恢复等新来源带入的计划外对象照常被校验拒绝。
- **move_obj 拒 Mod（kk-engine settle/mod + movement/spawn）**：签名改
  `Result<(), String>`，Mod 对象显式 Err（payload 字段与信封坐标的一致
  不变量由 update_mod_fields 单点维护，原始索引移动不再能破坏它后让整
  tick 在发布校验爆）。movement 侧 `map_err(SettleError)?` 传播；spawn 侧
  chick 按 Creep 变体筛出，`expect` 带不变量依据。
- **freeze 双入口（kk-mod-api freeze + 全调用方）**：`freeze` 对
  `builtin` 命名空间贡献显式 Err；内建贡献经 `freeze_with_builtin`
  独立参数进入（产出与混装完全一致——内部按 namespace 排序归一）。
  调用方迁移 26 处：realm、settle/mod.rs 的 builtin_plan、kk-world
  （kkpv×3/place）与 kk-runtime（world_source）测试夹具、kk-mod-api 自身
  7 处（player_surface×3、freeze_cases rules×3/catalog×1）、m2b/c3
  引擎测试族；freeze.rs 的 `plan` 夹具助手兼容混合 vec 形态（首个
  builtin 走独立参数、后续 builtin 留在 mods 侧按保留名拒绝——catalog
  断言从"重复贡献来源"改对新语义，通用重复来源与 freeze 入口拒绝由
  新增断言直钉）。
- **create_shard 注释（kk-runtime embed）**：改为真实语义——输入校验
  失败保留旧分片；mod 注册/玩家面/canary 失败发生在回收之后，旧分片已
  销毁、引擎已重置为空。可选增强（装配成功后再回收）登记不实施。
- **注释规范违例十处**：初盘五处——kkpv_store.rs 头注（B.4 语义→
  私有块形态）、room.js 捕获 set 注释（B.6→共享直读内存的安全要求）、
  settle/mod.rs plan_hash（B.5 残留→计划变化使旧块失效）、embed 测试
  文档注释（B.5 残留 5→行为描述）、world_json object_json（过时投影
  批次句，随 Option 契约改写）；评审补获五处——kkpv_channel.rs 两处
  （B.6 收紧对称面/B.6 的 §6 验收面，在被审范围内、初盘漏网）、
  c4_player_api_family 头注与归属门注释（C.4/C.3 引用）、
  c3_counter_chain_family（C.4 起句）。
- **管理读面契约与钉住（docs + kk-world 测试）**：设计 §8 增补"管理/
  诊断读面在权限契约之外"；新增测试
  `admin_snapshot_face_reads_server_only_player_projection_does_not`
  钉住分工两面：object_snapshot_by_id 可读 ServerOnly（操作员排障面），
  玩家投影永不携带。

## 登记与排期（用户拍板方向）

处理器异常的失败半径为全分片（整轮原子提交的直接结果），触发可达性为
玩家载荷（schema 合法输入仍可让缺陷 Mod 抛异常）。缓解按既定方向排期：

1. **处理器结果分型**（正常拒绝 Declined 与异常 Fault 分开；拒绝走
   丢弃+计费+可选事件，异常维持炸候选）——随 D 批 SDK 面冻结定论。
2. **运行期毒化**（同一 Mod 连续故障达阈值即停用其处理器与 hook、路由
   意图丢弃计费并响亮记录，世界继续推进；阈值入注册选项）——E 批持久化
   前必须落地。C.2 的 mod_states 候选域回滚保证毒化发生在干净边界。
3. **房间隔离不做**：动 tick 契约根基（部分推进世界使跨房不变量、
   恢复协议、检查点全部复杂化），且不叠加毒化就不解决活性问题。

设计文档 §7 已补已知边界段；计划文档 D/E 批行已登记。

## 三路评审与清偿

三路并行只读评审（正确性/freeze 迁移/文档与注释规范）：

- **评审 A（正确性与不变量）**：可提交，无 P1/P2。P3×2 已清偿——find
  两面补 Mod 排除与查询去向的文档句；内容键控校验补回归钉
  `engine_validate_guards_plan_foreign_mod_objects_and_extensions`
  （默认引擎 + 直插 Mod 对象→"不在计划中"；默认引擎 + 直插扩展条目→
  "未接入扩展平面的对象"——旧计划级短路下两者都会静默发布）。
- **评审 B（freeze 双入口）**：可提交，无 P1/P2。P3×3 已清偿——
  freeze_with_builtin 文档补 builtin 参数"宿主可信构造、只校验命名空间"
  的信任假设；catalog 补通用重复来源断言（`module("dup")`×2）与
  freeze 入口对 `module("builtin")` 的直接拒绝断言（外审指出的原始
  漏洞形态——无真 builtin 在场时旧口径会放过）。
- **评审 C（文档与注释规范）**：须修后提交，1×P2 已清偿——
  kkpv_channel.rs 两处 B.6 批次号注释（在被审范围内、初盘漏网）改写，
  note 盘点声明同步修正（见判定总表与清偿明细）。P3 已清偿——C.4 引入
  的三处批次引用注释、freeze.rs/catalog.rs 两处考古口吻措辞、"报告
  8 项发现"计数对账（见下）、find_snapshots 文档句的方向纠正（Mod 不在
  find 面，管理查询走 object_snapshot_by_id）、计划文档双空行。更早
  批次的模型名/评审轮残留（A3 观察，约 10 处，先于被审范围）登记为
  后续注释清偿候选。

**清偿复审（收口）**：代码与测试清偿逐项核实通过（回归钉推演成立、
26 处迁移完整、find 面文档方向正确、全 diff 无夹带、crates 批次号引用
清零）；三处一行级 P3（计划块测试计数 524→525、计划文档双空行、
catalog 新注释考古口吻）已修，修后提交。

**计数口径（对账）**：报告分级发现共 9 项（P1×1、P2×4、P3×4）——
8 项证实（含 P1 的链条证实与两处叙述修正、freeze 项的"证实但缺口
更窄"）、1 项（snapshot 管理面）已由 C.4 部分清偿；另按槽判定扫描
1 项定性非活漏洞、存疑 4 项分别归置（性能矩阵/加固项/装载器后续）。

## 验证

- **workspace 525 绿**（+2：管理面/玩家面 ServerOnly 钉住测试、内容
  键控校验回归钉；freeze 两断言并入既有测试）；fmt/clippy 全过。
- **棘轮 1854/2914 零翻转**（floor 1854，相对 HEAD~1 基线转红 0）。
- **bot 三基线过**：tooangel 2000t/overmind 2000t/wasm 10t 验收判定全过；
  tooangel 的 `offsetPosition.lookFor is not a function` console 输出经
  HEAD（15d90af）独立 worktree 20t 对照确认**两侧同为 4 次、既有形态**
  （非本片引入）。
- **零 Mod 开销同机 A/B 双样（交错 h1/w1/h2/w2）**：HEAD（15d90af）与
  工作树各 2 样（8 玩家/300 tick/warmup 50，默认 JSON 通道）p50（ms）：
  project 0.555/0.563 vs 0.562/0.557、run 0.366/0.371 vs 0.365/0.375、
  settle 0.131/0.131 vs 0.130/0.130、wall 1.049/1.067 vs 1.048/1.057——
  project/run/wall 点估计 ±0.5% 内、区间重叠；settle 点估计 −1.2%
  （反向，量级在既往 ~5% settle 噪声带内）。内容键控校验扫描的零 Mod
  开销**不可分辨**；≥5 样正式判读与 C.5 性能矩阵一并。
- **doc-links**：201 个 Markdown、1013 条本地链接通过。
