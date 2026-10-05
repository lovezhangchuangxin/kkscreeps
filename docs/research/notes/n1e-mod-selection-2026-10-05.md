# N1e：新世界显式内容选择（2026-10-05）

基于 90b670f，新世界创建可以显式选择安装哪些包：`include`/`exclude` 列表经宿主 `RegisterOptions.selection`、sim 场景 `mods.selection` 与 `mod inspect --select/--exclude` 三处进入同一展开语义。选择在模块装载之前解析为实际安装集合，未选包不进加载器、不注册、无类型/规则/处理器/玩家面与行为；`builtin` 固定内建不可选。

现行契约见[内容选择](../../architecture/mod-content-selection.md)，机器汇总见[JSON](n1e-mod-selection-2026-10-05.json)。交付前完成代码复审、修复及完整门禁。

## 实现与评审

`kk-mod-host` 新增 selection 模块：`ContentSelection` 输入、BFS 依赖闭包展开（集合与迭代有序，结果与目录装载序无关）与 `SelectionReport`。`register_inner` 在装载前解析选择并整体拒绝坏配置：未知包 ID、缺失依赖（含路径）、显式排除与必需依赖相撞（含路径，不静默重新加入）、include/exclude 重叠、builtin、configs/disabled 指向未选包。`CompositionReport` 新增 `selection` 字段并提升 report_version 2；sim/inspect 顶层报告同步为版本 2，sim 的 mods 元数据记录选择输入。恢复沿用分片私有启动快照：含选择的注册选项随完整包表保存，重建按同一选择解析并核对 PlanHash。

## 功能验收

定向 host 选择 12 项、runtime 引擎面 2 项、runtime 恢复身份 1 项通过。包含：闭包展开与自动加入报告、选择改变 PlanHash、同安装集合不同输入共享 PlanHash、ContentSelection serde 形状锁定、无选择时行为不变、未知/builtin/重叠拒绝、缺失依赖与排除相撞路径、排除模式安装其余、未选坏包不阻塞、handler 引用未选包沿用未知处理器依赖错误（显式选择后同一注册成功）、configs/disabled 指向未选包拒绝、引擎面未选类型不可放置且玩家 `typeof` 为 undefined、故障销毁后恢复沿用同一选择（漂移被身份核验拒绝）。

独立 alpha/beta/gamma 夹具不使用专用 Rust 分支：alpha 规则驱动自有对象增值；beta 依赖 alpha 并覆盖规则；gamma 无依赖，只在 link tick 写 state。三个场景各 100 tick、JSON/KKPV 双通路全等：只选 alpha 时 node=100、beta/gamma 玩家面与 state 全缺；只选 beta 时 alpha 自动加入、node=300（覆盖 step=3）、beacon=100、beta 玩家面可用；alpha+gamma 组合 gamma state {marked:true}。三个选择的 PlanHash 互不相同，报告双通路逐项一致。

## 原始报告

六份[场景报告](n1e-mod-selection-alpha-json-2026-10-05.json)（三场景 × JSON/KKPV）与四份 inspect 报告（[选 beta](n1e-mod-inspect-select-beta-2026-10-05.json)、[选 alpha](n1e-mod-inspect-select-alpha-2026-10-05.json)、[排除相撞拒绝](n1e-mod-inspect-reject-collision-2026-10-05.json)、[未知包拒绝](n1e-mod-inspect-reject-unknown-2026-10-05.json)）记录同一 release CLI 的身份。已逐项核对每场景两通路最终世界（含完整 Mod 对象、state、玩家 Memory）与 composition 完全一致。

## 完整门禁

重新构建 release CLI，显式以 KKSCREEPS_BIN 指定该二进制执行 pnpm run check：格式、workspace Clippy、Rust 775 项通过/11 项忽略、适配器 14 项通过、文档链接和源码规模检查（375 个文件均不超过 1000 行）通过。

固定 screeps-ok 提交 34a630efa42908c117c19c25d2350fa2387bd633，构建适配器后运行 ratchet：[完整结果](../../../compat/reports/kkscreeps.json)共 2914 项，1918 通过/89 失败/907 跳过，满足 floor 1918。与批次开始 HEAD 的所有用例身份及状态逐项相同；ratchet 对 HEAD~1 的转红/转绿均为 0，没有新增豁免。两条既有未命中豁免警告保留，不归因于本批次。

## 剩余范围

本片只做新建世界口径的启动期选择：没有运行期改选、停用即卸载或存量世界迁移/升级（四种操作须区分）。builtin 固定依赖与原生消费者的迁移随 N2/N3 逐族落实。内容预设语法未冻结，登记为后续；当前入口都是逐包 ID 显式列表。评审确认的后续测试项：选择 × 运行期停用 × 故障恢复三者组合的用例（恢复路径同时携带 selection 与 disabled，机制已覆盖但无专测）、exclude-only 成功路径的引擎面场景（解析层已覆盖，安装集合与 include 形态无分叉）。
