# 玩家与 Mod 的连续工作负载

本批在双玩家单轮恢复验收基础上，补充恢复后持续执行和可复跑的完整玩家闭环采集。当前新增实现、验证命令和原始结果在本笔记归档；仅作为独立房间合成负载的本机证据，不代表完整架构性能目标完成。

## 实现与边界

[mod-player-probe](../../../crates/kk-cli/src/probes/mod_player.rs) 驱动每玩家独立房间中的一个 worker、source 和 [Mod counter](../../../compat/fixtures/mod-player/workload/server.js)。每轮玩家 API 发出 counter.increment，并根据 worker 能量采集或升级，写回 Memory 序号。全部玩家的执行结果、原生/Mod 提交结果和 namespace 计数均须符合预期。worker 合成寿命覆盖整个窗口，不测孵化、死亡、共享房竞争或真实 bot。

阶段时间沿用 runtime；进程窗口包括逐轮状态核验和 RSS 采样。预热与边界快照在窗口外；report 的 window_actions 排除预热次数。CLI 不改变默认玩家/Mod 预算，记录预算及包/脚本指纹。JSON/KKPV 和并发 1/2 的测试要求全部负载快照相同。空转 Mod 即使玩家方法返回 OK，状态检查仍须拒绝。

[连续恢复测试](../../../crates/kk-runtime/tests/mod_multiplayer_tests.rs) 在每种通道/并发配置运行 24 轮：首轮异常两次触发自动停用，第 9/17 轮注入 RawMemory/segments 写回失败。失败保存全部玩家与世界、namespace、时钟代际及上一轮 console；空请求重试发布两名玩家缓存。后续逐轮核验执行序号、Memory、segments、通知和 counter 计数，仅恢复轮要求正扣费，不对耗时作断言。模块全局 calls 随 Mod VM 重建清零，已提交 namespace 的 increments/transfers 持续累计，二者不混淆。

首次 debug 采集测试在第 50 轮因空仓管理 JSON 省略 energy 键失败，已按当前管理序列化约定把缺键读作 0。连续测试首次失败因快照辅助函数假定上一轮 console 为空，已改为保存并比较已提交 console。两次属于工具/测试断言修正，没有修改游戏执行规则。

## 验证与采集

最终 release CLI 的 SHA-256 为 `e33eb5fda135148d88e582a54fc576c5878d8d6c94374ee852c333227ecb40e5`，源码基点 `765b942`，工作区含本批及已有用户变更。最终 [验证清单](mod-player-workload-validation-2026-10-04.json) 固定源码、日志、夹具、报告及二进制指纹。

- `KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check`：格式、Clippy、全仓测试和文档门通过；599 项 Rust 通过、10 项忽略；19 项 JavaScript 通过、0 项跳过。最终补齐证据链接后另跑文档检查。
- release 定向测试：CLI 三项通过，runtime 双玩家测试三项通过。CLI 额外覆盖跨越默认 1500 tick 寿命；负面测试分别拒绝 Mod 空转和玩家异常。
- 固定 screeps-ok `34a630efa42908c117c19c25d2350fa2387bd633`，重新构建适配器并用最终 release 跑全量棘轮：1854 通过、153 失败、907 跳过，零转红、零转绿。已有失败/跳过没有变成已支持能力。
- [CLI 冒烟原始结果](mod-player-workload-smoke-2026-10-04.json)：单玩家零预热成功；32 玩家串行 JSON 与并发 32 的 KKPV 三轮完整状态等价；零玩家、零采集 tick、超出 50000 样本上限均退出 1，不生成报告。这些短窗口不作为性能基线。

本批未修改 runner、runtime 或 engine 的生产执行逻辑；真实 bot 本次未重跑。上批 [bot 与恢复验收](mod-multiplayer-recovery-2026-10-04.md) 的 Overmind 既有异常、TooAngel 普通日志错误仍保留原结论，不能据本次合成闭环宣称真实 bot 零错误。

长窗口第一次失败来自放置参数误写 `ageTime`，被接口忽略后使用默认寿命；没有生成一个有效窗口，[失败归档](mod-player-workload-failed-2026-10-04.json) 保存该尝试。其运行曾与兼容性门禁重叠，不纳入性能结果。修正为 `ticksToLive` 后，新增初始寿命验证；该验证曾误用管理快照不存在的 ageTime 字段而失败，最终按快照的 ticksToLive 校验。最终全仓检查及兼容门禁结束后才采集以下十个窗口，期间无本任务的并行构建或测试。

## 十个独立窗口

[采集脚本](../../../scripts/benchmark/research/mod-player-workload-capture-2026-10-04.py) 固定两玩家、并发 2，JSON/KKPV 各五次独立进程，预热 500、采集 3000 tick；奇数轮 JSON→KKPV，偶数轮 KKPV→JSON。脚本核验 release、报告口径、阶段样本数、有限非负指标、包/脚本/预算身份和全部工作量；保留所有原始报告，失败不发布 complete=true。可在仓库根复跑，输出须为新路径：

```sh
cargo build --release -p kk-cli
python3 scripts/benchmark/research/mod-player-workload-capture-2026-10-04.py /tmp/mod-player-new-windows.json
```

[全部原始窗口](mod-player-workload-windows-2026-10-04.json) 共 30000 个采集 tick，加 5000 个预热 tick。两种通道的三个边界完整快照与动作次数一致；每窗口每名玩家实际执行 3000 轮，提交 3000 次 Mod 递增、2000 次升级、1000 次采集，Mod namespace 总计 7000（含两玩家预热）。没有失败、未执行或玩家异常的轮次。

下表为五个窗口指标各自的中位数及最小..最大，非合并 tick 后的分位数。CPU 包含逐轮核验/RSS 采样；wall 为 EmbedEngine 内部范围；RSS 为采样峰值，单位 MiB。

| 通道 | CPU core-ms/tick | wall P50 ms | wall P99 ms | 采样峰值 RSS MiB |
|---|---|---|---|---|
| JSON | 0.307（0.299..0.317） | 0.176（0.167..0.176） | 0.573（0.568..0.656） | 126.00（124.95..127.83） |
| KKPV | 0.396（0.393..0.401） | 0.223（0.222..0.225） | 0.630（0.619..0.660） | 124.72（124.06..125.16） |

这份同负载本机样本中，KKPV 的 CPU 和 wall 中位指标均更高，不能默认宣称二进制通道在小世界里更快。尚未做成本归因、不同对象规模矩阵或专用机器复测，也没有比较官方私服/xxscreeps；不能据此改变全局通道策略或宣称整体架构达标。下一批可沿同一负载排查玩家物化与通道成本，再扩充共享房和更大对象规模；本批未把旧 native/empty 性能差异归因为已解决。
