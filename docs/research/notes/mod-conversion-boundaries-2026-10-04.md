# Mod 转换边界（2026-10-04）

基线为 `a8708d9`。本片补齐 Mod 宿主从 V8 接收值的转换入口；候选提交、冻结计划与共享可信 VM 的结构保持现有实现。当前机制见 [Mod 执行](../../architecture/mod-execution.md)。

## 复现

有外层期限的独立子进程发现：注册声明包含 Symbol 时退出码为 -6（SIGABRT）；getter 嵌套合法声明也以 -6 退出。注册尚未开始时传入 getter/Proxy 载荷，旧入口仍先访问载荷，Symbol 同样触发 abort。自动 serde 转换发生在 Rust op 函数体的注册状态检查之前。

载荷 getter 抛异常且 JS 捕获 op 错误后，剩余合法声明仍能冻结出 Registered；测试期望 Err，实际收到 Ok。直接 invoke 与 evaluate_probe 的 Symbol 结果也触发未捕获 Rust panic，测试子进程退出码 101。密封返回值的 Vec<String> 会正常拒绝 Symbol，但仍一并使用统一转换入口，避免以后入口分叉。

## 实现

- `v8_convert::from_v8` 从写入模块移到共同边界；仅围住同步 serde_v8 转换，归一 panic 为 UnsupportedType，不格式化 panic 载荷。默认 panic hook 仍可能向 stderr 写诊断，panic=abort 构建不在保护范围。
- 声明 op 改为手动接收 V8 Local，先核验注册能力、故障和密封门，释放 OpState/Registry 借用后进入 TryCatch 与转换。明确允许 reentrant，合法 getter/Proxy 嵌套声明继续收集。
- Registry 留存转换故障，转换结束重新检查；被内层 JS 捕获的故障也阻止外层声明。模块求值后与注册回调后核验故障，失败清空待收集声明并密封，不能发布部分计划。
- invoke、seal 与 probe 返回值使用共同转换边界。probe 结果增加 TryCatch；脚本及转换一并进入已有结算 CPU/墙钟预算，超限毒化 VM。
- 普通 invoke/probe 转换错误返回 Err，普通错误后探针仍可工作；直接宿主调用方负责处置。runtime 的结算事务仍在错误时丢弃候选并销毁 realm。

## 验证

全仓检查通过：564 个 Rust 测试（10 个忽略），19 个 JavaScript 测试，格式与 Clippy 均通过。Debug/Release 的转换与预算子进程矩阵通过。固定版本 screeps-ok 为 1854 passed / 153 failed / 907 skipped，相对逐用例基线零转红、零转绿。

完整验证数字与命令记录于 [机器证据](mod-conversion-boundaries-validation-2026-10-04.json)。

转换子进程覆盖 15 种场景：Symbol、捕获的 getter 异常、不执行异常对象的格式化钩子、内层故障被捕获、合法 getter/Proxy 嵌套声明、模块求值中被捕获的错误、未开始/已密封注册门、invoke/seal/probe 的 Symbol 与 getter 异常。资源矩阵新增 8 个 CPU/墙钟场景，涵盖声明 getter、invoke 结果 getter、probe 脚本及结果 getter 的死循环；每个子进程均有 10 秒外层期限，预算故障要求 5 秒内返回并不能复用 VM。

本片未改变玩家 API、玩法规则或性能数据口径；不据此声称尾延迟改善。跨 Mod 故障归因、重复故障停用、其他 Rust panic 和进程崩溃恢复仍待后续开发。
