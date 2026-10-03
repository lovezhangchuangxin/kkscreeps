# Mod 上下文优化复审与修复（2026-10-03）

复审范围为尚未提交的 [拥有值复制优化](mod-context-copy-2026-10-03.md) 与 [类型化读取封套](mod-context-envelope-2026-10-03.md)，同时核查其候选边界与性能报告门。发现并修复原先桥接路径已存在、但本轮测试未覆盖的三处问题；没有把这些问题归因于类型化读取新增。

## 发现与修复

1. **P1，写入 getter 可导致 debug 进程 abort**。`#[serde]` 转换会访问 getter；getter 调用读/写/native op 时，普通 write op 的 deno_core 重入断言在不可 unwind 的 FFI 边界触发。带外层期限的子进程测试实测 `non-unwinding panic. aborting`。修复将 write 入口声明为 `nofast, reentrant`，转换时不持 OpState 或候选借用。允许 getter 的 JS 调用不会放宽能力或 native 恰一次限制。
2. **P1，被捕获的载荷转换失败未使候选失败**。原先转换在 op 函数体前发生，错误绕过 ActiveChain::outcome；Mod 捕获抛错 getter 的异常后，槽可继续成功。非法绑定的 getter 也会先执行，随后才拒收绑定。独立探针分别实测槽成功与 getter 调用次数为 1。修复先核验能力/绑定，再在 TryCatch 内转换；转换失败留存，转换后再次核验 getter 引发的候选故障。错误传播不访问 JS 异常对象属性。
3. **P2，性能门可放过无效计时值**。原先只查分位数字段存在，NaN / Infinity / 负值 / 布尔或字符串都可进入汇总；15 个无效墙钟输入的拒收测试先红。现在拒收非有限、负值与非数值，要求分位数有序、最大值不低于 P99；CPU 同样校验，阶段次数/错误/纳秒严格为整数。

## 验证边界

Host 子进程矩阵覆盖 getter 嵌套读/写、被捕获的嵌套故障、被捕获的转换故障和非法绑定；核验 after/native 执行与候选能力销毁。runtime 在 JSON/KKPV 两条通道令 getter 先调用 native 再抛错，核验已发布世界、generation、namespace 不变，原配置重建后重试仍不能提交部分候选。

类型化读取保持原空原型与键序，开放载荷仍为 JSON；写入保持动态 JSON 反序列化规则。此前两片性能数据保留其当时二进制摘要，本片修复后的新二进制不冒用旧测量摘要。最终验证与源码指纹见 [验证报告](mod-context-review-validation-2026-10-03.json)。

`pnpm run check` 通过：561 个 Rust、19 个 JS 测试，9 个 Rust ignored；格式/Clippy 通过。Python 全套 13 测通过，更严格的报告门重新核验此前 72 个原始窗口均通过。重新构建的 release 棘轮为 1854 passed / 153 failed / 907 skipped，零转红/转绿；真实 profile 冒烟核验完整链计数和 v2 阶段次数。TooAngel/Overmind/WASM 重新完成 2000/2000/10 tick，均无硬中断、惩罚跳过或 missing API；Overmind 既有 1 个 console-error tick、TooAngel 既有 log 错误保留。
