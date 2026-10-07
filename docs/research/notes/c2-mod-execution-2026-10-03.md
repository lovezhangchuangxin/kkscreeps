# C.2 处理器执行与 EmbedEngine 接线（2026-10-03）

[计划](../../plans/archive/mod-plane-batches.md) C 批第二片。范围：JS 处理器进入
结算——执行器可失败化、Mod 状态候选域、realm 派发、`create_shard_with_mods`
装配与零 Mod 开销验证。**不含**：wrap 的 next 续行接线、Mod 自有槽
（动作处理）执行、对象/意图/挂载绑定的装配（C.3/C.4/C.5）。

## 交付物

- **执行器可失败化**（kk-engine settle/contrib.rs）：before/after/replace
  处理器签名从不可失败改为 `Result<(), String>`——JS 处理器的异常必须
  能使本轮候选失败，不能因执行器形状退化为 panic 或静默。before 失败
  即中止（不进 wrap/center/after）；after 仍只在链成功后执行且失败
  同样传播。wrap/Next 语义不变（B.2 已定）。
- **Mod 状态候选域**（settle/txn.rs + settle/mod.rs）：`SharedState` 增
  `mod_states: BTreeMap<ns, Value>`；`TickState::with_mod_states` 构造、
  `mod_state/set_mod_state` 读写（候选内可见）、`into_parts` 三元返回
  （`PublishedCore` 别名）；`PreparedTick` 携带、`publish_tick` 整体换回
  `Engine.mod_states`（已提交态，每 tick 克隆为候选）。服务端私有，
  不进世界快照与玩家投影；失败轮候选丢弃即回滚。
- **realm 派发**（kk-mod-host）：sdk.js 增 `dispatch(key, ctx)` 导出——
  绑定表查函数、同步调用、thenable 结果拒绝、返回 `{state, result}`；
  `ModRealm::invoke(binding, ctx)` 以 v8 调用走同一条纪律（异常传播、
  Promise 拒绝）。处理器契约：`function(ctx)`，ctx 携带 tick/room/state，
  终值 state 由调用方写回候选。
- **EmbedEngine 接线**（kk-runtime embed.rs）：`Engine::install_plan`
  （with_plan 拆出的就地装配——世界/玩家/沙箱保留，与 with_plan 同一
  校验）；`EmbedEngine::create_shard_with_mods(spec, packages, options)`
  = create_shard + 注册 + `mod_handlers` 装配 + realm 持有
  （`Rc<RefCell<ModRealm>>` 字段，EmbedEngine 因此不再跨线程移动——
  结算与 realm 同线程）。`mod_handlers` 把计划里 link 两槽的 Mod
  before/after/replace 绑定装成桥闭包（ctx = tick/room/候选状态，
  结果 state 写回）；**wrap 非空即装配拒绝**（执行接线后置，不静默
  截断链）；Mod 自有槽绑定经 `check_installed_bindings` 显式报错
  （C.3 接线）。create_shard/teardown 重置 realm。

## 验证

- kk-mod-host 23 测（+1：invoke 往返——状态续传/thenable 拒/异常传播/
  未知绑定拒）；kk-engine 257 测（Simple 签名迁移后全绿）；embed 链新增
  4 场景：状态跨 tick 累积提交（3+2 tick 计数续传）、处理器异常使候选
  失败（世界/Mod 状态/代际全部保持，重试同败）、replace 中心执行 +
  wrap 装配即拒（错误指向后续批）、动作处理绑定无执行器装配显式拒。
  workspace 492 全绿；clippy/fmt/doc-links 通过。
- **棘轮 1854/2914 零翻转**；**bot 三基线通过**（tooangel/overmind
  各 2000t、wasm 10t，验收判定全过、既有形态不变）。
- **零 Mod 开销同机 A/B 双样**：HEAD（662fc32）与工作树各 2 样
  （8 玩家/300 tick/warmup 50，默认 JSON 通道）：project 0.551/0.560 vs
  0.550/0.555、run 0.354/0.340 vs 0.347/0.348、settle 0.129/0.130 vs
  0.127/0.126、wall 1.038/1.032 vs 1.024/1.027（ms，p50）——全相
  **不可分辨**（工作树点值略优）。对照 B.2 记录的 8p wall 0.774 是
  机器态漂移（相隔数日），非本片回归；≥5 样正式判读后置。

## 评审与清偿（三路并行只读评审）

合计 **0×P0 + 2×P1 + 若干 P2/P3**，处置：

- **P1-1 装配顺序破坏常量同步**（spawn 先于 install_plan，Mod 覆写
  builtin:link.capacity 时玩家常量读默认值而结算按新值——静默跨平面
  分叉）：`create_shard` 拆出 `create_shard_inner(spec, mod_install)`，
  注册与计划装配移到引擎重置后、玩家 spawn 前（常量与结算同源）。
- **P1-2 dispatch 对 `return null` 抛 TypeError**（`null !== undefined`
  后读 `null.then`）：补 `result !== null` 守卫 + null 往返测试。
- **P2 状态清空不可表达**：桥改为**整值无条件写回**——未触碰=恒等写、
  写 null=清空（与缺失同形，读取侧都得 null）、原地改写被捕获；
  sdk 注释锚定契约。
- **P2 批次号入注释/错误串（C02）**：三处改陈述性文案（"wrap 处理器
  不支持执行装配"），同步测试断言；`place_object` 被劫持的两行 doc
  归位；`create_shard_with_mods` doc 补"失败时 vanilla 分片已建立"。
- **P3 清偿**：invoke 错误前缀 `处理器 {key} @ {room}`；注册失败测试
  收紧断言（去掉弱化 OR）并补 `e.tick(1)` vanilla 可跑断言；replace/
  注册失败两处测试注释名实对齐。
- **登记不修**（见下）：before-Err 中止与 replace-Err 传播的引擎级
  专测缺口（实现与 after-Err 对称、e2e 已锁 after 路径）；密封后
  微任务队列永不冲洗（确定性成立，但 fire-and-forget 异步是无声
  残留）；dispatch/seal 是 Mod 可导入导出（信任面内自伤）；**处理器
  模块全局不回滚——同输入重试确定性依赖"状态只走 ctx.state"纪律**
  （已入 sdk 契约注释）；kk-mod-host 经 kk-runtime 进入服务端二进制
  （C 批接线预期，链接面变化在此留痕）。

## 登记的简化与残留（后续承接）

1. **wrap 的 next 续行接线归 C.5**：JS wrap 需经再入 op 同步续行
  （spikes/deno-spike js_mod_host 已验证机制）；本批装配即拒。
2. **Mod 自有槽/动作处理绑定无执行器**（C.3）：`check_installed_bindings`
   显式报错；当前可装配面=link 两槽的 before/after/replace + 规则。
3. **桥跨越粒度为 JSON 整上下文**：每次调用序列化 ctx/反序列化结果；
   读写候选对象（ctx.objects）与 fast op 通道归 C.3 按实测选型
  （设计 §6.3）。
4. **mod_states 无限额与 schema**：状态值任意 JSON、无大小上限
  （信任面，boot 期注册的 Mod）；持久化随 E 批（当前仅内存态）。
5. **EmbedEngine 因 realm 不再可跨线程移动**：单机嵌入式用法无影响；
   需要移动时再评估专用玩法线程 + 通道桥（设计 §6.2 的 worker 形态）。
6. **每 tick 空候选克隆**：`Engine.mod_states` 每tick 克隆进候选——
   空 Map 成本为噪声（A/B 已证）；有状态负载后按代际写时复制优化。
7. **处理器 ctx 无对象/房间数据**：只有 tick/room 名/state；世界读视图
   与 txn 写通道归 C.3。
8. **before-Err 中止与 replace-Err 传播无引擎级专测**：实现与 after-Err
   对称（`?` 传播），e2e 锁定 after 路径；补测归 C.3 测试扩面。
9. **密封后 realm 微任务队列永不冲洗**：结算确定性成立（无逃逸），
   但处理器排下的 fire-and-forget 异步反应无界残留且拒绝不可见。
