# Mod VM 故障恢复（2026-10-03）

## 问题与范围

基线 `f1db17c` 的世界、Mod namespace 与玩家写回已经统一候选发布，但 Mod 模块全局留在同一 realm 中。JS 已运行后，市场、校验或玩家写回返回错误时，只丢弃 Rust 候选不足以隔离模块全局污染。

本片在 kk-runtime 增加私有 ModVm 生命周期容器；引擎的槽执行器与 EmbedEngine 共用容器，容器只持有一个可用 realm。引擎结算返回错误后立即取空并销毁 realm；下次执行前，用分片创建时私有复制的包字节、注册配置和冻结选项重建，重新注册并校验原 PlanHash 与可执行绑定清单，成功后才发布。恢复失败不发布部分 VM，不消耗玩家候选，不执行新的玩家轮。

既有引擎计划、玩家 API、已提交世界和玩家状态都不重新安装。玩家失败轮仍复用原 RunResult 和 TickInput；Mod namespace 从已提交状态进入候选。成功 tick 保留 Mod VM 的模块缓存。`discard_pending_round` 不解除 Mod VM 的独立恢复门；显式恢复接口不处置玩家缓存或引擎恢复状态。

## 故障验收

- Market、Validate、Publish 与 Market/Cpu/RawMemory/Segments 写回七个故障点，JSON/KKPV 两通道：模块全局与 native 已改候选后失败，时间、代际、对象、namespace 和玩家状态不发布；重试后的目标对象、namespace、时间和代际与先独立执行的冷启动参考一致。
- 成功第二轮继续复用模块缓存；随后失败与恢复，持久 namespace 从提交值继续，模块缓存从初始值重新建立。
- 建立后修改调用方包字节与配置，再删除包目录：恢复仍使用原启动快照与配置。
- 新 VM 注册完成后注入恢复失败：缓存玩家结果、世界、Memory、segments、CPU 桶、日志保持旧值；错误玩家请求不取走缓存；空请求重试整轮，玩家全局计数证明未重跑，缓存移动意图恰一次生效。
- 丢弃外部失败轮后，世界/市场修改仍被恢复门拒绝；恢复失败不能执行新玩家代码。新建分片与 teardown 清空生命周期。
- 原冻结身份不匹配与实际模块装载/注册错误：均保持空槽，修复后可恢复。
- 依赖模块全局「第二次才成功」的故障处理器连续三次失败，证明失败污染无法改变后续结果。

## 边界

这是返回错误后的内存恢复策略。Mod 应将持久状态放在 ctx.state，模块全局只作定义与可重建缓存；重建不会使随机数、墙钟或任意模块全局成为确定性回放输入。共享 realm 的故障半径仍是整分片；确定性异常仍会阻止推进。注册/结算预算、永久卡死、OOM、panic、进程崩溃、故障停用策略与磁盘恢复留待后续切片。

本片未改默认 native 执行路径，也未声明密集 Mod 或 VM 重建成本的性能收益。当前机制以 [Mod 执行](../../architecture/mod-execution.md) 为准。

## 验证结果

- `cargo test -p kk-runtime`：32 个测试通过；包含新增 1 个 VM 恢复单元测试与 4 个集成测试，集成测试内部执行上述故障/通道矩阵。
- `pnpm run check`：fmt、workspace/all-targets Clippy、Rust workspace 测试通过（548 通过、7 忽略、42 个结果组），JavaScript workspace 19 通过，文档链接通过。
- 重新构建 release CLI，显式构建适配器；上游固定提交 `34a630efa42908c117c19c25d2350fa2387bd633`。`KKSCREEPS_BIN="$PWD/target/release/kkscreeps" node compat/scripts/ratchet.mjs`：1854 通过、153 失败、907 跳过；通过数下限 1854，相对 HEAD~1 逐用例转红 0、转绿 0。
- tooangel/overmind 各完成 2000 tick，wasm 完成 10 tick；均为 0 硬中断、0 惩罚跳帧、0 缺失 API。Overmind 保留既有首轮 console error；tooangel 的 log 级内部错误仍在，工具通过不等于完整玩法正确。bot-check 不运行 Mod realm，Mod 恢复由上面的真实结算测试验收。
- [机器验收记录](mod-vm-recovery-validation-2026-10-03.json)保存最终二进制摘要、命令、计数与 bot 原始报告。最终注释与文档调整后重新检查格式、链接及差异空白，并重新构建 release CLI；二进制摘要与棘轮、bot 使用的构建一致。未宣称性能或资源预算已验收。
