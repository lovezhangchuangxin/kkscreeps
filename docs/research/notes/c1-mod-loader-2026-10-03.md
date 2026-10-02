# C.1 Mod 装载器与可信注册（2026-10-03）

[计划](../../plans/mod-plane-batches.md) C 批首片。范围：从磁盘上的 JS Mod 包到
冻结 `ContributionPlan` 的启动路径——包发现、manifest 校验、依赖拓扑、指纹、
可信注册 realm（deno_core）、注册期 SDK 与贡献组装。**不含**：处理器在结算期
的执行、玩家 API 安装、生产引擎（EmbedEngine）接线——默认世界不引用新 crate，
零 Mod 路径零变化（同 B.1 纯数据层先例）。

依据：[mod-system 设计](../../designs/mod-system.md) §1（共同可信宿主/ESM）、
§2（包结构/装载顺序/指纹）、§6.1（kk-mod-host 定位）、§13。

## 交付物

新 crate `kk-mod-host`（约 1.1k 行 + 测试/夹具）：

- **manifest.rs**：mod.json 严格子集解析（`deny_unknown_fields`：未实现的
  services/client 等字段按未知字段拒绝，能力补齐时显式放开）。id 必须是
  合法命名空间且不得为 `builtin`；版本 `x.y.z` 三段数字（无 prerelease）；
  范围 `*`/精确/`^`/`>=`；`apiVersion` 须与宿主 SDK 1.0.0 兼容；entry/player
  路径限定包内相对 `.js/.mjs`。
- **package.rs**：整包字节装载进内存（≤512 文件 / 8 MiB），拒绝符号链接；
  摘要与模块装载共享同一份字节（注册期不触碰磁盘，无装载/摘要竞态）。
  code_digest = 除 player 外全部文件（排序混入路径+长度+字节）；
  player_api_digest = player 字节（未声明为零摘要）；config_digest =
  规范化 JSON（对象键排序，与 serde_json preserve_order 特性无关）。
- **graph.rs**：依赖拓扑序（Kahn，BTreeMap 确定性迭代）；缺依赖/版本不满足/
  依赖环均为装载错误。
- **realm.rs**：可信注册 realm。tokio current-thread 执行器 + deno_core
  JsRuntime + 自定义模块加载器：只允许包内相对导入与 `@kkscreeps/mod`
  虚拟模块（`kkmods:sdk`）；裸导入与非 file scheme 拒绝；绝对 file URL
  （宿主装载入口的形态）经 load 侧的**包根包含约束**与**跨包导入拒绝**
  兜底。注册循环：拓扑序逐包 `load_side_es_module` → `mod_evaluate` →
  `run_event_loop` → 调 default(api)；register 必须**同步返回**（Promise
  视为注册错误）。全部注册完成后调 SDK `seal()` 取回绑定函数键清单，
  Rust 侧同步置密封位。
- **sdk.js**：注册期 SDK（虚拟模块，单实例、无自身状态）。`createApi(namespace, config)`
  产出 `api.{config,objects,intents,handlers,rules,mounts}`；处理器函数
  **留在 VM 内的绑定表**（Rust 只记录键）；`seal()` 后一切注册面调用抛错。
  形状是设计 §9 拟议 SDK 的最小可用子集，不视为已冻结。
- **convert.rs**：声明 JSON → kk-mod-api 描述符的**权威**转换。键约定与
  builtin 同构：字段键 `{ns}:{name}`（类型内作用域）、字段绑定
  `{ns}:{类型名}.field.{name}`、动作处理绑定 `{intent}.process` + 自动入口
  `{intent}.slot`（以处理绑定为 Native 中心）、处理器绑定与键同名、挂载
  绑定 `{ns}:mount.{name}`。命名空间归属在 Rust 侧定论（SDK 预检只是快速
  失败）；声明引用的处理器/动作处理绑定必须在密封函数表内——**声明不算
  实现**。

产出 `Registered { plan, contributions(拓扑序), executables }`；plan 由
`freeze([builtin::link()] + mods)` 得出，含跨包挂载、既有入口 wrap
（`builtin:link.tick` 变 Chained）与规则覆盖（唯一候选自动选中）。

## 信任模型与注册期安全面

- 共同可信宿主（设计 §1）：Mod 是服主安装的服务器扩展，Mod 之间不构成
  隔离边界；注册期 SDK 无任何世界读写 op——"注册期修改游戏世界"结构性
  不可能，而非靠检查。
- 密封是双门：JS 侧 `sealed` 标志（api 方法快速失败）+ Rust 侧
  `Registry.sealed`（op 层拒绝）。泄漏的 api 对象与直连
  `Deno.core.ops.op_mod_declare` 在密封后都抛错（测试锁定）。
- 宿主打标：声明经 op 进入时由 Rust 侧 `Registry.current` 打命名空间标签，
  SDK/Mod 不能自报身份；跨命名空间键在转换层拒绝。
- 函数不跨 isolate：绑定表只在 VM 内；丢弃 realm 即丢弃函数表。
  C.2 的调用机制（execute_script 派发 vs reentrant op，见
  spikes/deno-spike js_mod_host 实验）届时定。
- realm 一次性：seal 后 `register` 拒绝再入；升级/换包 = 新 realm。
  V8 线程亲和：创建线程使用（文档锚定）。

## 评审与清偿（三路并行只读评审，清偿后全量复跑）

三路评审（Rust 装载链路 / deno_core 与 V8 语义 / SDK↔描述符忠实性）
合计 **0×P0 + 6×P1 + 14×P2 + 22×P3**，逐条处置：

**P1 全部代码清偿**：

1. **装载字节上限在读完后才判定**（含 mod.json 无界读）——改为
   metadata 预检 + `take` 硬顶的双层有界读，读满预算即拒；超限大文件
   不再进入内存（oversize 测试锁定）。
2. **非常规文件未白名单**（FIFO 可挂死 boot）——`is_file()` 白名单，
   其余报"非常规文件"。
3. **discover 顶层遍历静默吞错**——错误传播，包无声消失改为 boot 失败。
4. **动态 import 根装载绕过"跨包导入拒绝"**（deno_core 根装载的 load
   无 referrer，跨包一致性检查结构性缺席）——resolve 层直接拒绝
   `ResolutionKind::DynamicImport`（顺带封 `import.meta.resolve` 的
   宽容面），静态导入图是唯一装载形态（TLD await import 测试锁定）。
5. **f64 默认数值域 ±∞ 与 freeze 有限端点校验死冲突**——convert 层
   要求 f64 显式双侧 min/max，报错指明原因（正负测试锁定）。
6. **SDK 把 ref 目标强制本命名空间、convert 层却允许跨包**——SDK 改为
   键形状预检（归属仍由 Rust 权威校验），跨包 ref 可达（双包夹具锁定）。

**P2 代码清偿**：注册失败毒化 realm（sealed=true + current 清空，失败
重入必拒）；包根符号链接拒绝 + 根路径规范化；player==entry 拒绝（code_digest
盲区）；目录深度上限 32；重复包 id 报错带 id 与两个目录；拓扑序反向夹具
（依赖方向 ≠ 字典序）；entry/player 路径负形 ×7（绝对/盘符/反斜杠/CurDir/
player 上跳/后缀/同文件）；字段名禁点号（绑定单射性）；min/max/maxLen/
maxKeyBytes/queueMax 形状非法即报错（不再静默落默认）；playerClass 缺失
给精确报错；handler id 在 SDK 层预检字符集；queueMax 透传、撤除 convert
不读的 stage 字段；config_digest 注释收窄（数值字面量形态不归一）。

**P3 代码清偿**：环报错文案、`^0.0.x` 语义注释、player 摘要 expect、
符号链接测试整测 cfg 门控、tokio 上下文禁忌入文档。

**清偿复审（只读复审 agent）结论"基本闭合、一处须修"已收口**：

- 规则声明的 min/max 仍走宽松读取（字符串/浮点形状静默落默认域）——
  number_domain 收拢为唯一边界入口：f64 端点按浮点严格读（小数端点
  合法，清偿了"f64 边界只能取整数"的未登记缺口），整型按严格 i64 读，
  schema 与规则两路共用（正负测试锁定：规则 min:'5' 拒、f64 max:0.75 过）。
- mod.json 保留在快照与 code_digest 内（解析用克隆字节）——指纹按设计
  覆盖包的全部实际字节，"只改 manifest 不升版本则 PlanHash 不变"的
  缺口随之关闭（测试锁定 manifest 字节变化改变摘要）。
- 复审判定"超限测试无法区分读前拒/读后拒"属实：测试锁定超限必拒，
  读前拒由 read_bounded 的结构保证（metadata 预检 + take 硬顶）。
- 计划文档测试计数同步（12/481 → 22/491）。

**P2/P3 登记不修**（见下残留表）：nullable 显式 null 默认折叠、override
数值形态分流、i32/u32 名义宽度不钳制、依赖重复键 last-wins、URL 别名
双实例、不 settle 的 TLD 挂起、微任务 declare 归因、configs 指向不存在
包、孤儿 process 绑定、限额默认值锚定。

## 验证

- **kk-mod-host 22 测**：demo 夹具全链（拓扑序/类型与字段/动作与自动入口/
  wrap 后 Chained/跨包挂载/规则默认与覆盖/可执行绑定表/指纹来源）；
  确定性（两次注册同 PlanHash/贡献/绑定表）；配置进入注册与摘要
  （start→字段默认、config_digest、PlanHash 变化）；manifest 失败 ×8
  （未知字段/坏 id/builtin id/apiVersion 不兼容/两段版本/上跳 entry/
  非 js 后缀/声明的模块缺失）；路径负形 ×7；依赖失败 ×3（缺依赖/范围
  不满足/环）+ 拓扑序反向夹具；注册失败 ×10（外命名空间键/builtin 抢占/
  缺 process 函数/重复绑定/未知声明种类（直连 op）/规则缺消费者/
  async register/注册期抛错/上跳导入/裸导入）+ 动态 import 拒绝 +
  f64 双侧界正负 + 点号字段名/字符串数值 + 跨包静态导入拒绝 + 跨命名
  空间 ref 可达 + queue 透传；密封后泄漏 api 与直连 op 双抛错；二次注册
  拒绝 + 失败毒化；重复包 id（含目录定位）；包内/包目录两级符号链接
  拒绝；超限大文件预检；摘要分区（服务端文件变化改 code_digest、
  player 变化只改 player_api_digest）。
- workspace 491 测全绿（B.6 后 468 + 22 + doctest 1）；clippy 全工作区
  全目标 `-D warnings` 通过；fmt 通过；doc-links 通过。
- **棘轮 1854/2914 零翻转（转红 0、转绿 0）**：新 crate 不被服务端二进制
  引用，默认路径零变化。
- bot 三夹具未跑：本片不触碰 kk-runner/kk-runtime/kk-engine/cli 的任何
  既有代码（唯一既有文件改动是根 Cargo.toml 的 workspace.dependencies
  增行）；计划 §4 的"触碰 runner/runtime 的片必跑"条件不满足，同 B.1
  先例。perf 未跑：无生产路径消费，零 Mod 开销对照在 C.2 接线片补基线。

## 登记的简化与残留（后续承接）

1. **处理器执行（C.2）**：绑定函数表无调用路径；Mod 状态候选域、tick 钩子
   接线、失败/恢复语义与执行性能全部后置。
2. **玩家 API 安装（C.4）**：player.js 只进指纹；装载器、玩家 VM 注入与
   FIND/LOOK 注册未实现。
3. **SDK 面子集**：capabilities/lifecycle_slots/查询、Conditional 可见性、
   `suppressed_by`、处理器 `depends_on`、规则约束未开放——描述符层已支持，
   SDK 形状随消费者出现再定（设计 §9"不视为已冻结"）。
4. **player/world 域意图的阶段映射粗粒度**：统一归 Market 段；真实包出现
   再细化。
5. **manifest 子集**：services/client/permissions/types/migrations/
   configSchema 拒收（fail-loud）；放开时机=对应能力批。
6. **注册期无资源预算**：boot 一次的信任面（服主代码），无 CPU/时间限额；
   纯 JS 死循环靠进程级手段，不在此层。
7. **动态 import 与 import.meta.resolve 在 resolve 层拒绝**（评审清偿
   后的行为）：静态导入图是唯一装载形态。
8. **discover 只认一级子目录**；包版本无 prerelease；范围四种写法
   （`^0.0.x` 按次版本收口，npm 会收口到 patch——以本实现为准）；
   `evaluate_probe` 是可信诊断面（测试/排障），非玩家可达路径。
9. **EmbedEngine 接线（C.2）**：`Engine::with_plan` 的绑定核对目前只有
   Rust 实现白名单；JS 绑定的装配路径、`ContributionDispatch` 的宿主桥
   trait 与结算期调度均未动。
10. **nullable 字段的显式 null 默认不可表达**：转换层把 null 默认折叠为
    Missing（"默认 null"与"无默认"在描述符层是两种语义）；SDK 形状冻结
    前决定是否开显式通道。
11. **规则覆盖值按 JSON 整数性分流 Signed/Float**：浮点规则的整数字面量
    覆盖（`override(rule, 0)` 覆盖 Float 域）会报类型不匹配且不指明原因；
    u64 上半段不可表达。C.2 接线时按规则域上下文回放修正。
12. **i32/u32 种类名不钳制名义宽度**（显式 min/max 可超 i32/i64 范围，
    仅域校验兜底）；u64 的 (i64::MAX, u64::MAX) 开区间域不可表达；
    file_type→open 之间换入 FIFO 的 TOCTOU 可使 open 阻塞（take 顶只管
    字节数，信任面内）。
13. **manifest 依赖对象重复键静默 last-wins**（serde_json 对象解析本性）；
    configs 中指向不存在包的配置静默忽略。
14. **URL 别名可致同文件双实例**（`label%6c.js` 与 `label.js` 是不同
    module map 键）；永不 settle 的顶层 await 会挂起 boot（进程级手段
    兜底）；微任务里延迟 declare 会归因到错误的包（fail-closed 但排障
    体验差）。三者都属信任面内自伤。
15. **显式 slot 的动作 process 绑定是孤儿**（无 slot 引用它）；意图
    authority 恒 Player、conversion 恒 Strict、queueMax 默认 8、
    array/map 限额默认 16/128——SDK 面冻结前统一定论。
16. **player/world 域意图阶段映射无测试锁定**（粗粒度归 Market 段）。
