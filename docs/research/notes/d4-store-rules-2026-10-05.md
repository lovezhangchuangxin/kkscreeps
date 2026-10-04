# D4 仓库容量规则（2026-10-05）

本批基于 `19b3d92`，承接真实场景入口后的容量规则开放。现行机制见[仓库容量规则](../../architecture/store-rules.md)，原先容量和实验室槽切换的常量例外见[核心经济链记录](economy-gcl-2026-10-04.md)。

## 本批变化

新增 10 条具名容量规则，经冻结校验后解析为 StoreRules。Generic 放置和建造共用该快照；实验室既有矿物容量、首次绑定/释放，终端 tick 归一，以及同格容器自动承接资源改读同一规则。标准玩家常量同步到安装预检和实际沙箱，修复 canary 默认值误拒绝使用覆盖常量的 player.js。

冻结计划的内置目录由 15 条 link 数值扩为 25 条，source revision 与摘要同时纳入新规则文件。规则覆盖影响计划指纹，恢复继续核验原包快照。既有对象容量字段、共享仓竞争与统一发布纪律保持原有语义；本批不增加 JS 结算调用。

## 验证与范围

新增冻结测试覆盖官方默认常量、0/u32::MAX、缺键、负数/小数/越界与未裁决覆盖冲突。运行时真实 JS Mod 测试分别使用 JSON/KKPV，覆盖全部容量及标准常量、显式字段、已有矿物、终端持续 tick、实验室转入/取空、建造后首次转入、同轮竞争、晚期 RawMemory 写回失败和缓存重试。玩家安装模块同时核验 container/lab/link 的覆盖值，零容量目标遵守原玩家预检。自动承接测试将 10 点能量掉到剩余容量 3 的容器上：容器增加 3，掉落物在当轮衰减 1 后保留 6，计入衰减后资源守恒。

这是 D4 的容量数值第一片。特殊提取策略、实验室绑定策略、typed 仓库/creep 容量、transfer/withdraw 处理槽及 builtin decline 的多对象撤销面均未宣称完成。没有正式测量默认热路径的相对开销；StoreRules 每房按值拷贝，不能据“不增加 JS 调用”声称零开销。

最终新鲜 release 使用 `cargo build --release -p kk-cli`，适配器显式 build，上游 pin 保持 `34a630efa42908c117c19c25d2350fa2387bd633`。门禁通过绝对 `KKSCREEPS_BIN` 指向本次 release：

```bash
KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check
KKSCREEPS_BIN="$PWD/target/release/kkscreeps" node compat/scripts/ratchet.mjs
target/release/kkscreeps sim run --scenario compat/fixtures/sim-economy/scenario.json --ticks 2000 --report docs/research/notes/d4-store-economy-json-2026-10-05.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-economy/scenario.json --ticks 2000 --kkpv --report docs/research/notes/d4-store-economy-kkpv-2026-10-05.json
```

完整 check 通过格式、workspace Clippy、636 个 Rust 测试（11 个忽略）、JavaScript workspace 测试（兼容适配器 14 个通过）和文档链接检查。新增 2 个冻结测试与 5 个真实 Mod 运行测试全部进入最终门禁。

完整棘轮保持 1918 passed / 89 failed / 907 skipped，总数 2914，地板 1918 不变。与当前 HEAD 和脚本规定的 HEAD~1（核心经济链）均零逐用例状态变化、零转红；无新增跳过或豁免。两个原有豁免未命中转红，保留已有警告。汇总见[机器记录](d4-store-rules-2026-10-05.json)，全量结果见[套件报告](../../../compat/reports/kkscreeps.json)。

默认经济夹具在两通道均完成 2000 tick，7 条断言全部通过，玩家错误为零，升级新增 1468 点、实际孵化 4 个替补；最终世界与 Memory 相同，且与上一批原始报告一致。报告指纹对应最终 release 二进制，原始记录：[JSON](d4-store-economy-json-2026-10-05.json)、[KKPV](d4-store-economy-kkpv-2026-10-05.json)。这些无 Mod 场景证明默认玩法保持原结果，Mod 覆盖由上述运行时测试验证，不能混为同一验收。

## 提交前复审

复核规则域、消费者接线、两个计划装配入口、字段优先级、标准常量与 canary 一致性、实验室槽切换、终端归一、自动资源承接及失败候选。发现并修复了自动承接仍用固定 container 容量的问题；沿用原全局容量语义，未擅自改为单对象场景容量。修复后重建 release 并重跑完整门禁与双通道场景，没有剩余阻断项。
