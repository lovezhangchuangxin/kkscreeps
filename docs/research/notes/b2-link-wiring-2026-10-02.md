# B.2 link 默认贡献接线

本片在 `64f2d91`（B.1）基础上把 `kk-mod-api` 的 link 定义接入生产引擎：
有效规则贯穿放置、建造、结算、配额、伤害与玩家常量，槽执行器落地
Native/Chained 分派。依据[领域与投影设计](../../designs/domain-projection.md) §1–§4、§8 的 B.2 行。

## 实现

- `kk_mod_api::builtin::LinkRules`：从冻结计划提取 link 全部规则的
  拷贝快照（装配时解析句柄一次，热路径按字段读取）。
  `kk_world::place::builtin_link_rules()` 以 OnceLock 冻结内置计划，
  作为无自定义计划调用方的默认。
- 放置与建造：`PlaceCtx.links` 携带规则；
  `generic_structure_hits` / `built_store_capacity` /
  `built_store_capacity_resource` 的 link 臂、`normalize_site` 的
  link 建造成本改读规则。`settle/creep/build.rs` 建造落成经
  `Working.links` 传同一份。显式放置字段仍优先（覆盖不吞测试输入）。
- 结算：`Working.links` + `Working.contrib`（`&'w ContributionDispatch`）。
  `link_transfer` 的损耗率与冷却系数、`static_hits_max` 的 link 上限、
  `check_structure_against_controller` 的 link 配额（`allowed_for`，
  RCL 0..8 走 `links.rcl`）改读规则，不再直读 `kk_core::constants`。
- 槽执行器 `settle/contrib.rs`：`ContributionDispatch::assemble` 按
  冻结计划的 `Dispatch` 装配 before/wrap/center/after；wrap 经 `Next`
  续行——至多调用一次，重复调用使本轮候选失败（`SettleError`，
  `settle_room` 返回 Err，候选丢弃）。wrap 不调用 next 即截断后续，
  链仍成功；after 只在链成功后执行，冻结时已逆序，执行器顺序调用。
  `Engine::with_plan` 装配自定义计划并核对声明绑定（白名单对应
  B.2 已接线消费者，未安装的绑定拒绝装配）。
- 玩家工地创建：`settle/room.rs` 的 `create_construction_site` 中
  link 建造成本与 RCL 配额同样读 `Working.links`，与转移门同源
  （发现过程见本批评审记录：初版只迁了转移门，此入口为清偿补齐）。
- 玩家常量：`SandboxConfig.constant_overrides` 在常量平铺前叠加
  覆盖（键不在常量表时初始化失败）；`EmbedEngine::create_shard`
  按引擎计划注入 `LINK_CAPACITY`。预检继续读权威字段（其数值源头
  已是规则）；JS `isActive` 的 CONTROLLER_STRUCTURES 表与配额覆盖的
  同步登记为限制（见下）。

## 验证

- 工作区测试 428 通过、0 失败、5 忽略（B.1 后 412，+16：
  kk-mod-api 2、settle_tests 14、kk-runner 与 kk-runtime 串行链场景计入既有单测计数）。
  clippy 全工作区全目标 `-D warnings` 通过；fmt 与文档链接检查通过。
- 新测试族 `m2b_contrib_link_family`（14 项：双层
  wrap 嵌套、tick 槽 Chained 链、玩家工地 buildCost/配额覆盖、
  覆盖下自转移、double_next 的引擎级回滚断言）：规则覆盖贯穿放置默认值
  （容量/hits/工地总进度+显式字段优先）、转移结算（损耗/冷却/满钳/
  同轮冷却先加后减）、RCL 配额门、hitsMax 钳 tower 修复、装配拒绝
  （缺处理器实现、builtin 命名空间占用在 freeze 层先行拒绝）、
  处理链执行序（before→wrap→native→after，after 依赖拓扑逆序）、
  wrap 截断（native 不执行、冷却从未写入）、replace 替换中心、
  双 next 候选失败且世界保持上一 tick。
- 变异击杀 ×6（改后红、还原后绿）：转移损耗绕回 `c::LINK_LOSS_RATIO`；
  放置容量绕回 `c::LINK_CAPACITY`；配额绕回官方表（须先构造分歧窗口
  ——官方 rcl5=2 非零，覆盖为 0 才有判别力；rcl3 官方本就为 0，
  该值下变异不可检出）；wrap 截断失效（native 仍执行——须 RCL5 夹具
  使 native 可真实执行，rcl3 下配额门本身就会拒绝、断言无判别力）；
  玩家工地总进度绕回官方表；玩家工地配额绕回官方表。
- 完整 screeps-ok（重建 release 后）：1854 通过、153 既有失败、
  907 跳过；相对 HEAD 报告逐用例转红/转跳过/缺失均为 0，转绿 0——
  默认行为逐用例一致。
- bot 三夹具：tooangel/overmind 各 2000 tick、wasm 10 tick 全部完成
  （计数读自 compat/reports/bot-*.json）；
  硬中断与停跑均为 0；console error 计数与 A3/A4/P1 基线一致
  （tooangel 0、overmind 1 轮 tick 1 既有形态、wasm 0）。
- 性能：8 玩家档 HEAD×工作树各 5 次独立运行对照（3000 tick、预热 500），
  数字见下节。

## 性能对照

1/4/8/32 玩家各 5 次独立运行中位数（B.1 基线 → B.2 工作树，机器 Mac16,12/M4/10C，
预热 500、采集 3000 tick，二进制 SHA-256 记录于报告 JSON，
原始数据见 [B.1 基线](b2-perf-before-2026-10-02.json)与[B.2 工作树](b2-perf-after-2026-10-02.json)）：

| 玩家 | wall P50 ms | settle P50 ms | CPU core-ms/t | RSS P50 MiB |
|---|---|---|---|---|
| 1 | 0.128 → 0.132 | 0.0126 → 0.0128 | 0.201 → 0.207 | 69.8 → 69.0 |
| 4 | 0.351 → 0.370 | 0.0424 → 0.0437 | 0.874 → 0.913 | 184.1 → 184.3 |
| 8 | 0.756 → 0.774 | 0.0806 → 0.0819 | 1.981 → 2.055 | 331.5 → 331.0 |
| 32 | 5.194 → 5.386 | 0.3488 → 0.3568 | 12.80 → 13.39 | 1259.0 → 1258.6 |

wall P50 涨幅 +2.4%~+5.6%。消融判读：本批未触碰的投影相同步上涨
（32 档 project P50 2.659→2.821，+6.1%），说明存在构建布局与当日
机器态的全局漂移成分；settle 相 +1.6%~+2.3% 与未动相涨幅同量级，
不能把全部涨幅归因于本批改动，也无法排除 Working 规则快照拷贝与
槽分派引入的 ~1% 真实开销。本负载无 link 对象，transfer/tick 槽的
动态分派未被触发；该开销在 link 实际存在时另行测量。32 档 P99
10.69→12.27 由 after 侧单次 5.79 离群驱动（其余四次 5.29–5.52），
不作显著结论。负载自检各侧一致（控制器进度增量 5994）。

## 限制与后续

- `Engine::with_plan` 是 Rust 测试与装配入口；EmbedEngine 尚无自定义
  计划注入通道（workers 已随 create_shard 建立，规则变更须重建），
  真实 mod 装配归 C 批。
- JS 预检的 `isActive` 读 CONTROLLER_STRUCTURES 常量表；link 配额
  被覆盖时玩家侧判定与 Rust 结算可能分歧。官方常量表是镜像，
  B.2 未提供对象级配额的玩家覆盖通道——登记为 C 批 JS 装配时的
  同步项。
- `creep_resources.js` 的容量兜底 `C.LINK_CAPACITY`（权威字段缺失时
  的 fallback）与 globalThis 常量同源（嵌入表+覆盖通道），随常量
  覆盖生效；权威路径已由放置/建造落规则字段。
- 处理器为 Rust `Box<dyn Fn>`；JS 处理器、VM 状态回滚、候选补丁
  校验归 C。`Next` 重复调用的"候选失败"覆盖到 `settle_room` Err；
  panic 不捕获（同引擎既有边界）。
- B.1 登记的"link 工厂摘要仅覆盖描述工厂"维持：native 实现摘要随
  引擎版本（装配在同一进程），外部贡献摘要待真实 mod 加载器。
- Chained 槽每次执行为每层 wrap 装箱一个 Box（Native 默认路径直调零
  额外分配）；mod 链路的分配成本在 C 批真实 JS 处理器接入时测量。
- `Engine::with_plan` 用后不保留计划指纹；作业/视图边界的指纹核对
  （mod-system 要求"世界记录激活计划指纹"）随 B.4 共享句柄一并接线。
