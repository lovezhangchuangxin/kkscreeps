# 玩家侧体验一致性红线（Compatibility Charter）

> 2026-09-27 确立。约束：**内部可以任意优化，但玩家侧体验必须与官方一致，不得出现游戏脚本 API 的变动或差异。**
> 本文是所有 crate 设计与验收的硬约束，优先级高于一切性能优化。
>
> **适用范围**：本红线约束**玩家侧**（玩家脚本可见的 API 与行为）。服务器管理侧不在范围内：官方私服的 JS mod 生态（screepsmod-* 等）依赖官方 Node.js 进程内部与数据库集合，本项目不做官方 mod 兼容（[ADR-007](../decisions/ADR-007-mod-system.md)），自建 mod 机制（见本目录 README 的规划清单）。

## 一、完全不可变的部分（1:1 复刻，逐项验收）

1. **API 面**：约 340+ 方法/属性、443 个全局常量（盘点清单见 `../research/notes/game-api-inventory.md`）——名称、签名、返回值、错误码（`OK/ERR_*`）逐个对照官方实现移植；数值表直接从 `common/lib/constants.js` 生成，不手抄。
2. **运行时语义**：游戏循环的分阶段推进（官方模型为三阶段，本项目内部拆为五步，见 [game-loop.md](../architecture/game-loop.md)）、intents 延迟结算、同 tick 冲突动作优先级图（被覆盖动作仍返回 OK）、对象每 tick 重建、global 跨 tick 持久与 reset 时机、`Game.cpu.halt()` 行为。
3. **CPU 经济**：`tickLimit = min(500, limit + bucket)`、bucket 上限 10000 / 每 tick +limit、0.2 CPU/intent 计费（say/pull 免费、幂等、可退费）、硬超时按 tickLimit 计费 + skipTicksPenalty、`getUsed()` 计量公式与计费窗口（含 Memory parse 计入玩家 CPU 的官方语义）。
4. **数据语义**：Memory/RawMemory（2MB 上限、惰性 parse、tick 末写回）、segments（100KB×100、激活 10 段）、InterShardMemory、`creep.memory` 别名机制。
5. **模块系统**：CommonJS 多模块（require/module.exports、循环 require 半成品语义、模块缓存跨 tick、代码更新重建）、lodash 3.10.1 全局 `_`、`.wasm` 二进制模块。
6. **沙箱环境**：V8 语义（已选 deno_core）、无 setTimeout/网络/文件、`WebAssembly` 可用性同官方。
7. **寻路结果**：路径必须与官方逐点一致（含 tie-breaking）——通过 FFI 复用官方 `driver/native/src/pf.cc` 源码保证，不自行实现可能产生等价但不同路径的算法。

## 二、内部替换但玩家不可感知的部分（优化主战场）

序列化格式（从 JSON 文本改为直接的二进制编码，思路来自 rkyv——一个 Rust 的零拷贝序列化库）、存储引擎（从 lokijs/MongoDB 改为嵌入式键值库 redb/libmdbx）、进程拓扑（5 个以上进程改为单个二进制）、按坐标的空间索引、对象按类型分桶存放（SoA，即同类数据连续排布的内存布局）、按房间的流水线推进、增量推送——玩家看到的仍是同样的 JS 对象图、同样的 sockjs 协议（官方客户端使用的 WebSocket 兼容协议）、同样的渲染。判据：**任何玩家脚本在官方服务器与本实现的输出序列（console、intent 序列、结果状态）不可区分。**

## 三、灰区与默认决策（发现新灰区须在此登记）

| 事项 | 默认决策 | 说明 |
|---|---|---|
| 增量化后 Memory parse 的计费值 | **按官方等价工作量虚拟计费**，不因实现变快而少计 | 官方按实际耗时计费；我们增量化后实际耗时趋近 0，若照实计费会改变经济。虚拟计费保持玩家体感公平 |
| tick 速率变快 | 允许（这正是优化目标） | 官方不同 shard tick 速率本就不同；CPU 是预算制，玩家能做的事不变 |
| CPU 读数逐 tick 数值 | 不保证与官方逐毫秒相同 | 官方跨机器也不会相同；保证的是公式与口径一致 |
| 附加能力（如 ESM 入口、附加 API） | **默认关闭，开关显式开启** | 严格模式下零差异；开关开启的行为属于本实现扩展，不算官方兼容性破坏 |
| 官方 bug | screeps-ok 覆盖的全部复刻；未覆盖的官方 bug 若被证明有玩家依赖再评估复刻 | 例如官方 market `_.shuffle` 用 Math.random 破坏确定性——replay 需要确定性时以"官方行为"为准还是"官方文档"为准，逐例登记 |
| `Math.random` 等非确定性源 | 保留官方行为（真实随机） | 官方本就不可复现；内部结算的确定性（replay 用）与玩家可见随机性分开处理 |

## 四、验收关卡（不做完不算兼容）

1. **screeps-ok 行为一致性套件**（`sources/screeps/screeps-ok`，vanilla 2897 用例）从 M1 起进 CI 持续跑 parity；
2. 真实开源 bot 回归（tooangel/overmind 类）在 deno_core + CJS 垫片 + lodash3 底座上原样可跑；
3. 官方客户端/renderer 可正常连接游玩（API/sockjs 协议兼容）；
4. 对照移植检查表：每个 API 移植时登记"官方实现 file:line ↔ 本实现 file:line"映射。
