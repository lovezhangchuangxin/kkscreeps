# D4 Mod 资源领域能力（2026-10-05）

本批基于 `0664a3d`，继续贡献平面 D4：在已开放的三个内建资源动作槽中提供受控的库存读取、精确转账及显式损耗，并将正常拒绝恢复扩展到同房额外对象。现行协议与限制见[资源能力](../../architecture/mod-resources.md)。

## 实现与复审

值协议归到 `kk-mod-api`，引擎独立资源模块维护副本预检、库存和容量，Generic 解析及实验室绑定/释放与 native 共用领域函数。宿主 op 校验原始数字、安全整数及本次绑定；SDK 对能力关闭所属处理器的生命周期，保留原上下文枚举键顺序。runtime 单动作日志记录首次触达的完整 RoomObject，拒绝后先恢复日志，再由引擎恢复原始动作两端及事件边界；namespace state 仍只在整链完成后移交候选。

复审核对默认 native 代码搬迁等价、依赖方向、safe integer/u32 边界、两对象副本原子性、实验室切换、字段缓存失效、SDK 序列化前置验证和刷新、能力过期、故障优先级、跨房拒绝及资源日志恢复顺序。复审修正新增能力的枚举键兼容性，并补上独立槽转账导致总库存超出 JS 安全整数范围的写后检查。默认 native 的容量算法保持原行为，不能把领域 API 更严格的校验表述为所有路径已统一。

## 验收矩阵

- JSON/KKPV 双通道替换 transfer/withdraw/link transfer，显式损耗与精确事件字段、旧库存引用刷新、跨处理器 namespace 累计。
- 库存/容量不足返回零且不收费、不记事件；显式全额损耗、独立零容量、共享池多资源合计与未绑定槽；墓碑/废墟只能作为来源。
- 多对象路由后 before、wrap-before、replace、wrap-after、after 五个位置正常拒绝；额外实验室完整恢复、此前/后续动作继续、多个 Mod state 与事件一起恢复。
- 实验室成功取空再换矿物、覆盖后容量一致；同房双玩家并发执行后以稳定结算顺序竞争容量。
- NaN、负数、小数、不安全整数、过量损耗、自指、未知资源、不存在 ID 和对象转换参数不能被 catch + decline 掩盖；失效能力与跨房/其他槽访问拒绝。
- 单动作最多触达 256 个不同端点，重复访问不重复计数；第 257 个触达故障后世界、玩家及 Mod state 不发布。
- 多对象资源候选在晚期 RawMemory 写回失败后全部回滚；重试传入抛错代码仍只提交原玩家缓存，Mod 在重建 realm 中重新结算。

## 验证结果

最终 release CLI SHA-256、固定 screeps-ok 版本、场景断言和完整结果见[验收数据](d4-resource-domain-2026-10-05.json)。测试均显式指定本轮 release 二进制，适配器已重新构建。

- `cargo test -p kk-engine -p kk-mod-host -p kk-runtime`：428 通过、0 失败、5 忽略。
- `KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check`：格式、Clippy、Rust、workspace JS、文档链接、源码规模检查全部通过；Rust 667 通过、0 失败、11 忽略，Mod 整链 59 通过，compat Node 14 通过。315 个主项目源码/测试文件均不超过 1000 行。
- 完整 ratchet：2914 用例，1918 通过、89 失败、907 跳过，下限 1918。与开发前 `HEAD`、`HEAD~1` 分别逐条比较，状态变化均为 0；没有新增跳过或豁免。报告见[兼容性结果](../../../compat/reports/kkscreeps.json)。
- TooAngel/Overmind/WASM 各完成 2000/2000/10 tick，无中断、惩罚跳过或缺失 API。Overmind 保留已有首 tick `DirectiveHarvest.create` 错误，归一随机 harvest ID 后与入库报告一致；固定世界 bot-check 不代表完整可玩性。
- JSON/KKPV 经济场景均提交 2000 tick，七条断言全部通过，最终状态完全一致：GCL 与控制器进度各增加 1468，四个替补孵化，两个初始 creep 已消失。

提交前复审未发现剩余阻断问题。本批不包含其他动作槽、跨房资源能力、资源创造、新资源目录、磁盘检查点或正式性能测量。
