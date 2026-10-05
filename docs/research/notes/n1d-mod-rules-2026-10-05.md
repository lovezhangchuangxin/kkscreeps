# N1d：玩法 JS 读取有效规则值（2026-10-05）

基于 b97c130，玩法 JS 可经公共 SDK 的 `api.rules.effective(key)` 读取冻结后的有效规则值。值由宿主在注册密封后从 `ContributionPlan` 的 `EffectiveRules` 装入 realm（SDK 新增 `installRules`，与 `installChains` 同批同源），覆盖默认值、单方覆盖与显式选择的多方覆盖三种形态；处理器不能借注册闭包拿到声明默认值，注册期读取直接抛错。

现行契约见[有效规则读取](../../architecture/mod-rules.md)，机器汇总见[JSON](n1d-mod-rules-2026-10-05.json)。交付前完成代码复审、修复及完整门禁。

## 实现与评审

`EffectiveRules` 新增公开 `entries()` 供宿主遍历；realm 在 `install_chains` 内把键值对（整数/浮点统一按 f64 转换）经 `installRules` 装入 SDK 模块态只读 Map。恢复路径 `register_with_disabled` → `install_chains` 自动重装，受既有 PlanHash 与绑定清单核验；`values` 本就参与规范哈希，规则内容与计划身份不可分。停用掩码不参与该安装，读取继续返回原冻结选择。

复审确认：`installRules` 只能在密封后调用一次，畸形/重复载荷抛错并沿注册失败密封 realm；`effective` 先检查安装窗口再验键形状，未知键为处理器故障（沿既有异常路径），不静默返回 undefined；f64 域在冻结期已拒绝非有限值，JS 侧的有限数校验是双保险；没有新增 op、ctx 能力或按包 ID 的 Rust 分支。

复审清偿：realm.rs 超过 1000 行门槛，把包模块装载器拆为 `crates/kk-mod-host/src/loader.rs`（行为无关，plan_hash 不变）；补注册代码抢装 `installRules` 的失败测试、停用定义方后第三方继续读取规则目录的运行时断言，以及场景的 rate 精确断言与夹具 README 的长跑区分力说明；文档澄清 2^53 近似只可能来自宿主侧构造的规则值。

## 功能验收

定向 host 读取 4 项、runtime 恢复/停用 1 项、CLI 双通路场景 1 项通过。包含：覆盖值/默认值/浮点精确读取、跨命名空间读取 `builtin:link.capacity`、多方覆盖显式选择后的读取、注册期读取拒绝、未知/畸形键故障、停用覆盖方后读取继续返回原冻结值、整轮结算故障重建后值不变（模块全局归零佐证重建真实发生）、停用定义方后 state 停留在最后提交值。

独立 base/tuner 包不使用专用 Rust 分支：base 的 `base:meter` tick 每轮读取 `base:period`/`base:rate` 驱动增值与浮点累计，tuner 覆盖为 2/0.25 并覆盖 `builtin:link.capacity=700`。若处理器误读注册期默认值（4/0.5/800），100 轮场景将得到 value=25、pool=50、capacity=800；实际为 value=50、pool=25.0、capacity=700。玩家经公共玩家 API 读到规则驱动的已提交结果（memory value=50、pool=24.75——玩家先于同轮 Mod tick 执行，读到前 99 轮的累计）。

## 原始报告

[JSON 场景](n1d-mod-rules-json-2026-10-05.json)、[KKPV 场景](n1d-mod-rules-kkpv-2026-10-05.json)、[组合检查](n1d-mod-inspect-2026-10-05.json)和[停用预览](n1d-mod-inspect-disabled-2026-10-05.json)记录同一 release CLI 的身份。已逐项核对两场景 2000 轮最终世界（含完整 Mod 对象、state、玩家 Memory）与 composition 完全一致：两对象 value=1000、pool=500.0，state {period:2, rate:0.25, capacity:700, ticks:4000}，玩家 memory value=1000、pool=499.75；预览停用 tuner 后 plan_hash 与全部规则不变，selected_source 仍为 tuner。

## 完整门禁

重新构建 release CLI，显式以 KKSCREEPS_BIN 指定该二进制执行 pnpm run check：格式、workspace Clippy、Rust 760 项通过/11 项忽略、适配器 14 项通过、文档链接（267 个 Markdown、1519 条本地链接）和源码规模检查（366 个文件均不超过 1000 行）通过；realm.rs 拆出注册装载器 loader.rs 后回到限内。

固定 screeps-ok 提交 34a630efa42908c117c19c25d2350fa2387bd633，构建适配器后运行 ratchet：[完整结果](../../../compat/reports/kkscreeps.json)共 2914 项，1918 通过/89 失败/907 跳过，满足 floor 1918。与批次开始 HEAD 的所有用例身份及状态逐项相同；ratchet 对 HEAD~1 的转红/转绿均为 0，没有新增豁免。两条既有未命中豁免警告保留，不归因于本批次。

## 剩余范围

读取是只读快照：运行期规则修改、规则迁移、检查点与冷恢复未实现，属于 N2/N4。玩家面继续经常量与投影消费（见[仓库容量规则](../../architecture/store-rules.md)），Mod 规则没有玩家侧直读入口。内容选择/预设属于 N1e；本片没有把任何完整玩法、冷恢复或持久化能力写成已完成。
