# M0 报告：真实 bot 环境兼容性验收

> 2026-09-27。M0 验收关卡之一：**两个真实开源 bot 在桩环境（deno_core + CommonJS 加载器 +
> lodash 预置 + stub Game）下加载运行无崩溃，缺失 API 清单清零或登记**。

## 结论：通过

| bot | 形态 | tick | 完成 | 硬中断 | 缺失 API | console 错误 |
|---|---|---|---|---|---|---|
| tooangel | 89 个多模块 CJS + 重 lodash | 2000 | **2000/2000** | 0 | **0 项** | 0 |
| overmind（v0.5.2.1 release 产物） | 1.07MB 单文件 bundle（TS 编译） | 2000 | **2000/2000** | 0 | **0 项** | 每 tick 1 条（见登记项 1） |
| wasm-bot（合成夹具） | main.js + mod.wasm | 5 | 5/5 | 0 | 0 | 0（"wasm module loaded: true"） |

宿主进程全程稳定，预算桶无一次硬中断，bot 的 loop 每 tick 均真实执行
（tooangel 累计 room.find 上万次调用、市场查询、CostMatrix 反序列化；overmind 完成其
Memory 版本迁移流水线并持续运行其殖民逻辑与可视化）。

运行方式：`scripts/fetch-bots.sh` 拉取 → `kkscreeps bot-check --path <dir> --ticks 2000
--report <out.json>`（release 二进制；世界为 bot-check 内置的"家"：W1N1 的
controller/source/spawn/creep 各一）。详细 JSON：`bot-tooangel.json` / `bot-overmind.json` / `bot-wasm.json`。

## 验收过程中发现并修复的桩环境缺口

按发现顺序（这正是本验收关卡的目的——用真实 bot 暴露桩环境的洞）：

1. **类全局缺失**：tooangel 首跑即 `ReferenceError: Room is not defined`。官方运行时把
   41 个类挂成裸全局（从 engine/src/game 的 `defineProperty(globals,...)` 提取全集，含
   Spawn=StructureSpawn、Energy=Resource 别名）→ 补桩类全局。
2. **类层级缺失**：overmind 给 `RoomObject.prototype` 打补丁（`.ref`），而桩类没有继承链。
   按官方层级接通（Structure* 大多 extends OwnedStructure，wall/road/container/portal 直接
   extends Structure，RoomObject 为根）。
3. **原型链接缺失**：tooangel 的架构是给官方原型打补丁（`prototype_*.js` 全系，如
   `Room.prototype.execute`）——Game 上的对象必须挂在类全局的原型链上，补丁才能流动，
   `instanceof` 才成立。修复后 tooangel 从"降级空转"变为全速运行。
4. **具体 API 缺口**：`Game.cpu.getHeapStatistics`、`Game.market.getAllOrders`、
   `Room.memory`（Memory.rooms 懒别名）、`Structure.isActive`——全部补桩。
5. **探测仪表缺口**：缺失 API 探测原本只覆盖 Game 顶层，下探到 cpu/market/map/shard/
   gcl/gpl/房间/全部包装对象后才得到诚实清单。

## 登记事项（非缺失 API，不阻塞 M0 关卡）

| # | 事项 | 处置 |
|---|---|---|
| 1 | overmind `Visualizer.drawLogo` 每 tick `reading 'poly'` 异常：其可视化逻辑依赖真实房间状态（哑世界无地形/对象细节），每 tick 被 runner 捕获打印、bot 下一 tick 重启循环，宿主不受影响 | M1 引擎有真实世界后复核；属游戏行为缺失而非 API 面 |
| 2 | tooangel 首个 tick 起 "Executing room failed: reading 'position'"（bot 房间逻辑对哑世界结算结果的降级路径，自行捕获继续） | 同上 |
| 3 | 两个 bot 的发布形态均不含 `.wasm` 模块（overmind 仓库构建链为 2018 年代 rollup 0.62 + TS 2.9，Node 24 不可用，按预案取 release 单文件产物）→ wasm 的 require 语义用合成夹具 wasm-bot 覆盖（require 返回 Buffer → WebAssembly.Module 编译成功） | wasm 路径已有自动化覆盖；overmind 源码构建链如需真实验收，待社区工具链更新 |
| 4 | CJS 垫片的 `this === module.exports` 等边角语义按 Node 语义实现并单测锁定；与官方实现的逐项对照 | M2 玩家 API 层对照官方 game.js 复核 |
| 5 | bot 直接 `new Room(...)` 等构造桩类得到的实例无游戏语义（构造计数器记录） | M2 真实类实现后自然消除 |

## 对后续里程碑的输入

- tooangel/overmind 已进入可持续回归：bot-check 命令一行即可重跑（建议进 CI 的周期任务，M1 起每里程碑跑一次）；
- 桩环境的类全局全集 + 类层级 + 原型链接是 M2 玩家 API 层的**行为基线要求**——正式实现必须保持这些 bot 可跑；
- 哑世界投影（world JSON → stub Game）的形态已被两个 bot 验证可用，M1 把它换成真实结算产物即可。
