# G22.c 交付笔记：活动执行策略的持久身份（2026-10-06）

登记提交 `3a8d563`，基线 `5c6aec7`（G08.a）。前置审计（后台子代理，file:line 取证）确认空缺：检查点身份三件分散——ModIdentity（包/配置）、`engine.plan_hash`（冻结计划）、`disabled_mods`（停用掩码）——组合结果不入检查点，恢复时靠重放 disable 序列 + 闭包校验 + PlanHash 等值**间接再生**，"组合逐项一致"只是 mod_vm.rs 的注释假设；G23.a 状态版本与 G24.a 包切换缺少一个可持久/可比对的基础凭据。

## 交付内容

1. **域分隔摘要入口**（kk-mod-api `fingerprint.rs`）：`domain_digest(domain, value)`——与 PlanHash 相同的键序规范化（对象键 BTreeMap 重收集递归排序）+ 域前缀（小写字母数字/斜杠/连字符）防跨身份原像混用；`canonical_hash` 重构为共用 `canonical_bytes`。
2. **执行策略身份**（kk-mod-host `CompositionReport::execution_strategy_digest`）：输入钉死为 `plan_hash + disabled_mods + 每槽 (key/stage/execution/active)`；诊断字段（consumers/overrides/disable_closures/sources/selection 输入）刻意不入摘要。`build` 与 `with_disabled` 后均可重算。
3. **恢复自检**（kk-runtime `ModVm::recover`）：重注册产物经 PlanHash/executables 核对后，再对组合摘要等值核对——注释假设升级为机器核验，分叉即"分片继续停顿"可重试。
4. **检查点接线**：编排层新键 `execution_strategy`（保存取 VM 组合摘要，无 Mod 分片为 null）；恢复在重装配+停用重放+realm 重建后重算等值；原生检查点含该键即拒；**缺键的存量检查点跳过本项**（与故障域先例同口径，G23.a 决定是否升格硬门）。钉死清单 99→100 键（`checkpoint_schema_keys_are_pinned`）。
5. **sim 报告**：顶层 `execution_strategy` 字段，report_version 4→5（组合报告内部版本不动）。

## 验收（全部实际执行）

- **mod-host**（register_tests/composition.rs 新增 1）：摘要确定性（同组合重算等值、同包集合重注册回基线）；停用敏感（掩码变化即变化；before 链滤除停用包处理器、替换中心回退原生中心的链差异被捕捉）。
- **runtime**（checkpoint/tests.rs）：`checkpoint_schema_keys_are_pinned` 100 键；篡改 `execution_strategy` 与既有六类漂移同列拒绝且旧分片完整（`mismatch_and_invalid_payload...` 第七例）；全部 14 项 checkpoint 测试过（含官方包停用、自动停用跨冷恢复、旧版检查点兼容）。
- **sim**（sim_tests/checkpoint.rs `cold_cli_resume…`，双通道两遍）：续跑与直跑 `execution_strategy` 等值断言 + 64 位十六进制形状断言（文件 3 项全过）。
- 完整 `pnpm run check` 见提交记录。

## 设计取舍

- **引用而非复制 plan_hash**：摘要输入含 plan_hash 字符串（组合报告的十六进制形态）而非另存一份引擎域哈希，避免三处漂移错误类；恢复核对以重注册产物为权威。
- **选择输入不入摘要**：同安装集合来自不同选择输入是同一计划（selection.rs 既有语义），纳入会把"输入形态"误当"执行语义"。
- **存量检查点兼容**：执行策略是身份增强而非新状态域，缺键跳过沿用故障域先例；不兼容方向（新检查点载入旧二进制）由 deny_unknown_fields 兜底。

## 复审记录

三路后台只读评审（摘要与身份正确性、恢复链与 sim 报告、文档与门禁），无 P0；P1×2 + P2 若干，全部清偿：

- **P1-1（R1/R2 同项）**：sim 报告 setup_failed 路径版本停留 4 造成同通道分裂——两处升版提为 `SIM_REPORT_VERSION` 常量同源。
- **P1-2（R1/R2 同项）**：缺键跳过与原生注入拒绝两分支零覆盖——补 `strategy_key_compatibility_branches_are_pinned`（删键恢复成功续跑 / 原生检查点注入该键拒绝且旧分片保持原生形态）；交付笔记原「含旧版检查点兼容」表述限定为故障域形态，本笔记验收段如实记录新覆盖。
- **R1-P2-2**：null 降级口径（篡改为 null 等价 legacy 语义，其余门仍核验）在 checkpoint.rs 注释与 checkpoints.md 显式登记。
- **R1-P2-3 / R2-P2-1**：`mod_execution_strategy` 的 `.ok()` 与检查点硬失败的口径差异——访问器注释改为精确契约并加 debug_assert（失败理论不可达）；注释「同源取值」的不实表述移除。
- **R1-P2-4**：sim 等值断言防 null==null 恒真退化——补 64 位十六进制字符串断言。
- **R1-P2-5**：测试名承诺的「诊断字段不敏感」补真锁定（同安装集合不同选择输入 → 摘要等值，挂在 selection.rs 既有同集测试）。
- **R2-P2-2**：checkpoints.md 恢复链括注混淆冷恢复与故障重建路径——改写并注明 recover 三重核对属故障重建。
- **R2-P2-3**：mod-composition.md 版本表述陈旧（2→现行 4）顺手修正。
- **R2-P2-4**：mod inspect 暂不输出执行策略身份（输入已全可见、无消费方）——登记为 G23.a 跟进：凭据承重时把摘要加入 inspect 并同步升其顶层报告版本，便于作者预判停用对身份的影响。

评审人另核通过：摘要输入面完备且确定（slots 定序、ExecutionChain 序列化稳定、排除项逐一由 PlanHash 覆盖）；恢复自检时序与可重试性；检查点核对位置的三路径同源性；四象限兼容分支完备；domain_digest 的 NUL 分隔防伪强度；篡改第七例非恒真。

## 已知边界

- kkpv 块库仍只按 PlanHash 淘汰代际——停用掩码变化不失效旧块（现状语义，本批不动）。
- 摘要核验的是「组合派生一致性」，不核验 realm 安装代码与报告的逐字节一致（那由 executables/PlanHash/ModIdentity 三道门覆盖）。
- G22.b（确定性玩法时间/RNG）未开始；回放链（G25）未开始。
- mod inspect 不输出执行策略身份（G23.a 凭据承重时跟进，见复审记录）。
- `domain_digest` 返回裸 `[u8;32]`；未来新增身份域时宜以 newtype 加固防误传（复审附注）。
