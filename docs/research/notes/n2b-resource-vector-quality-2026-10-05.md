# N2b 资源向量质量修复（2026-10-05）

基于 `44c73a4` 的代码质量复查确认：公开 Rust 贡献可构造重复资源的成本、生产输入和输出；JS SDK 的对象格式虽不产生重复键，冻结层仍不能依赖装载器保证该不变量。

## 复现和修复

- 最小 Rust 程序使用 `freeze_with_builtin`、`Engine::install_plan` 和一次空意图 tick：两项 `energy:50` 输入、`H:1` 输出、容量 100、实际 energy 50，旧实现提交 `{"H":1,"energy":-50}`。
- 重复成本也被旧冻结层接受；相同资源向量仅改变排列就改变 PlanHash。
- 冻结前对三类资源向量分别排序，冻结校验分别拒绝重复资源名。排序在贡献副本和冻结建筑目录形成之前执行，身份与消费者因此同源。输入与输出之间共享资源仍合法，按既有净变化语义生产。
- 回归覆盖三类非相邻重复项、三类向量重排的身份/目录等价，以及数量变化继续改变身份。两个新测试在修复前失败，修复后通过。
- 建筑冷恢复测试改为比较报告的完整 `final`（包括全部玩家 Memory/raw_memory/GCL/房间及对象读面），只排除实际线程耗时导致变化的 CPU 桶。该报告比较不替代检查点自身对全部数据域的验证。
- 删除一处测试未使用变量，恢复 Clippy 无警告。

## 验证

- `cargo test -p kk-mod-api --test freeze buildings::`：11 通过。
- `cargo test -p kk-cli cold_resume_matches_uninterrupted_building_world`：真实 CLI 冷恢复在 JSON/KKPV 两通道通过。
- 显式选择重建 debug CLI 的 `pnpm run check`：Rust 852 通过、0 失败、12 个已登记忽略；JavaScript 14 通过；Clippy 无警告；源码规模检查通过。
- 按固定 pin 准备 screeps-ok，重建 release CLI 和适配器，显式选择 release 运行 `node compat/scripts/ratchet.mjs`：2914 总用例、1918 通过、89 个既有失败、907 跳过，门禁通过；与 `44c73a4` 的逐用例状态无变化。两项既有未命中豁免警告保留。
- release CLI SHA-256：`f1a968ddff2d2396f4037921ff4f60d9013349eeedd06f53d1ba1f2ed9ff20ee`。机器报告更新到 `compat/reports/kkscreeps.json`。

原 JS 装载器已经排序资源表，所以合法 JS 包的冻结身份保持既有语义。直接 Rust 贡献中此前未排序的向量现在使用规范身份；依赖旧非规范身份的外部检查点需按严格身份核验处理，不绕过恢复拒绝。

现行约束见[可建造建筑](../../architecture/mod-buildings.md)。
