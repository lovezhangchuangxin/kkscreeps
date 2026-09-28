# 设计笔记：玩家开发套件与本地集成测试（Dev Kit）

> 2026-09-27。目标：玩家本地编写代码后，**不依赖重型私服**就能快速且真实地做集成测试。
> 前提架构事实：本引擎是单二进制、进程内仿真、确定性 tick、房间快照 + 指令日志（两者合起来构成完整的事件溯源）——
> 这四点使"官方级真实模拟"在本地以毫秒级 tick 运行成为架构的自然副产品，而不是额外基建。

## 0. 社区现状与缺口（对照）

| 现有方案 | 形态 | 缺陷 |
|---|---|---|
| `screeps-test-utils`（jest + mock Game）【社区知识，未逐一核实版本】 | 快（毫秒级） | **不真实**：mock 的 Game 不是官方引擎语义，intent 结算/冲突/寻路行为全靠模拟 |
| 本地私服（launcher / screeps-server-mockup） | 真实 | **重且慢**：Node 多进程 + 存储中间件，tick ~1s+，1000 ticks ≈ 17 分钟，改一次代码要重新部署 |
| tooangel 等 bot 作者的 pytest + 私服流水线【社区知识】 | 真实 | 同上，且搭建复杂 |

缺口 = "快"与"真实"不可兼得。我们的答案：**测试环境就是生产环境的同一个二进制**（同一 deno_core 沙箱、同一 API、同一结算逻辑），只是以 headless 仿真模式运行——快与真实不再互斥。

## 1. CLI 形态

```bash
kkscreeps sim init          # 生成 sim/ 工作区：scenario.toml + bot 代码目录
kkscreeps sim watch         # 热重载循环：保存即重置世界跑 N ticks，console 实时流
kkscreeps sim run --ticks 1000 --seed 42 --scenario pvp1.toml
kkscreeps test              # 断言测试套件；退出码供 CI 使用；--report json
kkscreeps sim replay --from snapshot-4242 --ticks 100   # 时间旅行：从任意快照续跑
kkscreeps deploy            # 测试通过后经官方 API (/api/user/code) 一键上服
```

## 2. 场景即代码（scenario）

- TOML/JSON 声明：地形（生成器 / import 官方地形 / 手绘 ASCII）、房间布局、初始 creep/建筑/资源/RCL（Room Controller Level，房间控制等级，决定可解锁的建筑）、NPC 入侵脚本；
- **多 bot 场景**：两份玩家代码同图对抗（PvP、防守、入侵响应测试）；
- **快照 fixture**：任意 tick 的世界存档可直接作为测试起点（断点续测）。

## 3. 断言模型（关键决策）

- 主推**沙箱内 `test` 全局**：测试模块与 bot 同一沙箱运行（`sim/tests/*.js`），
  API 仿社区熟悉的风格：`test.tick(n, fn)`、`test.assert(cond, msg)`、`test.world` 检查器
  （`test.world.room('W1N1').find(FIND_CREEPS)` 等，只读快照）；
- 另提供 **jest adapter**（`@kkscreeps/test`，Node 侧经 Chrome DevTools 调试协议（inspect）驱动同一二进制），
  兼容存量 jest 工作流；
- **红线合规**：`test` 全局仅在 `--sim` 显式开启时注入，正常服务器模式不存在——
  遵守 [compatibility.md](compatibility.md) 的"附加能力默认关闭"规则。

## 4. 确定性与可复现

- 固定种子 PRNG（内部结算）+ 固定房间处理序：同 seed + 同 scenario + 同 bot 代码 ⇒ 逐 tick 相同的世界状态；
- 测试失败即自带最小复现：`--seed` + scenario 文件 + 代码版本，一条命令重放；
- 对照：官方服务器本就不可复现（`Math.random`、时序），我们的 sim 比官方更可测——
  但玩家可见随机性（`Math.random` 在玩家代码中）仍保持官方行为。

## 5. 调试体验

- **任意 tick 回放**：房间快照双缓冲历史 + 指令日志 = 完整事件溯源（03 报告架构的顺带收益）；
  "我的 creep 为什么往左走" → 取该对象任意 tick 的状态、收到的 intents、结算依据；
- **断点调试（M2.5 显式子项）**：`kkscreeps sim --debug` 起本地 CDP 服务（deno_inspector_server，
  随机端口并打印 ws URL），每个玩家/bot 一个独立 DevTools target——Chrome `chrome://inspect`
  添加 `localhost:<port>`，或直开 `/json/list` 返回的 devtoolsFrontendUrl；VS Code js-debug 按
  ws URL attach。多 bot 场景各 isolate 独立 target、独立会话，暂停一个 bot 不影响世界推进。
  **debug 档语义**：该玩家当 tick 豁免看门狗 arm——实测开着看门狗打断点必被掐，且 terminate
  要到暂停被唤醒后才展开（[inspector-spike 实验 N](../research/notes/spike-deno-inspector.md)）。
  默认关闭，`--debug` 显式开启（兼容红线"附加能力默认关闭"，同 §3 test 全局先例）；公网服不
  开放（鉴权与资源占用归 M5 安全加固）。
  前置依赖（M2-2A 沙箱接入时预留，均为 spike 实证约束，见笔记 §6/§7）：`SandboxConfig` 加
  debug 档（`RuntimeOptions.inspector: true`）；玩家模块脚本名**绝对路径化**（断点按 URL 绑定，
  deno_core 只把绝对路径资源名转成 file:// URL，裸名断点永远不命中）；泵事件循环固定
  `wait_for_inspector: false`（true 会在空闲循环 + WS 会话下销毁主上下文，此后调试全失效）；
  inspector 开关不改变遮蔽后全局面（实验 O），白名单测试无需豁免；
- **CPU/内存报告**：每 tick `getUsed()` 曲线、bucket 演化（bucket_sim 同款机制）、
  每 intent 的 0.2 计费明细、内存峰值——帮玩家在本地把 CPU 优化做完再上服；
- **可选本地 viewer**：`kkscreeps sim watch --port 21026` 起 ws，官方 renderer 连接实时观战
  （复用兼容层，零新前端）；
- 覆盖率类工具（可选后期）：哪些分支/creep 行为在测试中被执行过。

## 6. 速度预期与依据

- headless 小世界 tick 成本 ≈ 玩家代码执行（毫秒级）+ 房间结算（目标亚毫秒，无 IPC/存储中间件）；
- **1000 ticks 集成测试秒级完成**（对照：本地私服 17 分钟）；
- 依据：单二进制进程内仿真（xxscreeps `simulate()` 先例：单进程内跑完整 tick 管线）、
  快照恢复 <1ms（snapshot 实测）、blob 读写即内存操作；
- 这些是【推断+架构推论】，M2 单机可玩后以基准实测为准。

## 7. 编译型玩家工作流

- 内置 esbuild/swc：TS/JSX 等在本地 sim 与 deploy 走**同一条转译管线**（所见即所得），
  上传产物与本地测试产物一致；纯 JS 玩家零感知。
- 官方语义保持：服务器仍接受多模块 CJS（兼容红线），编译只发生在本地工具链。

## 8. 实现成本评估

低——大部分能力是既有架构的换皮：sim 模式 = 同一 engine crate 以库形式嵌入 CLI +
场景加载器 + test 全局注入 + 报告输出。不需要新进程、不需要网络（viewer 可选）、
不需要存储引擎（内存中的房间快照即可）。建议排在 M2（单机可玩）之后、M3（backend）之前，
因为它同时是**我们自己开发期的回归工具**（与 screeps-ok CI 互补：screeps-ok 管官方语义
一致性，dev kit 管真实场景行为）。
