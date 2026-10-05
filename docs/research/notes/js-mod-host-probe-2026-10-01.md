# JS Mod 宿主验证记录（2026-10-01）

本记录是 [06 架构复审](../06-architecture-and-js-mod-review.md)和 [Mod 设计](../../designs/mod-system.md)的技术证据，不是生产 Mod 系统验收。

## 版本与复跑

运行环境、rustc、源码 commit、fixture 和 release 二进制 SHA256 均在[结果 JSON](js-mod-host-probe-2026-10-01.json)中记录。deno_core 0.412.0，spike 独立 Cargo.lock 保持原样；新增 `[workspace]` 防止父 workspace 误收，原父 workspace 的 `exclude = ["spikes/*"]` 未使该独立 manifest 可直接运行。

在仓库根目录执行：

```sh
cargo build --manifest-path spikes/deno-spike/Cargo.toml --example js_mod_host --release --locked
cargo build --manifest-path spikes/deno-spike/Cargo.toml --example js_mod_host --locked
python3 docs/research/notes/js-mod-host-probe-2026-10-01.py
```

宿主：[js_mod_host.rs](../../../spikes/deno-spike/examples/js_mod_host.rs)；JS 行为：[js_mod_host.js](../../../spikes/deno-spike/examples/js_mod_host.js)。初始化前进入 current-thread Tokio runtime；否则 V8 延迟任务在某些运行路径会 abort，不能把裸 JsRuntime 初始化当作稳健的执行环境。

## 验证内容

候选是 `Rc<RefCell<Txn>>` 拥有的 f64 向量，权威状态是单独的 committed 副本。

- replace 将值设为 10；wrap 依次 +1、调用 native +3、native 内 JS lifecycle +2、外层 +4，最终 20。
- native callback 中再次执行读写 op，验证 JS→Rust→JS→Rust 的真实再入；debug 检查通过，native op 标记 reentrant，进入 JS 前释放所有候选/OpState 借用。
- 外层异常把候选改成 999，但 committed 保持 20；嵌套 lifecycle 异常让候选改成 23，但 committed 同样保持 20。宿主显式丢弃候选，没有把这项行为说成 deno_core 自动提供事务。
- 失败 callback 改动 JS 全局 volatileAttempts，候选回滚后仍为 1；销毁重建 VM 后为 undefined。
- Infinity 字段写入和冻结后注册被拒绝；纯 JS 无限循环经另一线程在约 100 ms 后发 terminate，宿主重建后执行 6×7 成功。该案例不测 native 阻塞。

所有案例在 5 个独立 release 进程和 1 个 debug 进程中通过。

## 微基准口径

同一进程中依次测试 1、100、1000 元素；每模式预热 5×100 遍、测量 5×100 遍；每遍所有元素 +1。field 模式每元素 2 个 fast op；batch 模式 serde_v8 全量数组读写共 2 个 op。各次断言元素最终值和计数一致。5 个 release 进程串行运行，不与当前构建 perf-probe 同时争用。

5 个进程中位数（µs/遍）：field 为 0.067/2.051/11.508；serde batch 为 0.174/2.291/20.548。运行脚本保存每次原始样本，不只保存汇总值。结果说明普通数组转换成本可能高于数量更多的 fast ops；不能外推为所有 direct ops 都比所有 batch 快，更不能泛化为玩家热点字段应回调宿主。

未测：真实 RoomState/字符串/schema、JS heap 配额、权限、磁盘、冷/热玩法回放、Mod 相互隔离、类型升级、同世界多线程、官方对照。实验主价值是确定调用和回滚边界，数字用于否定“batch 必然更快”的过早优化。
