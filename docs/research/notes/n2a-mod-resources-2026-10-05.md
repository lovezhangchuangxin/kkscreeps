# N2a：新资源目录（2026-10-05）

基于 fbd1564，JS Mod 可经公共 SDK 声明命名空间资源（键即玩家面资源名，如 `ore:crystal`），经冻结进入计划并参与 PlanHash；目录与内建 `RESOURCES_ALL` 合并后贯穿玩家常量、API 预检、清洗结算与投影。creep 对新资源的 transfer/withdraw/drop/拾取走既有资源槽链路与局部恢复边界；Generic 仓库/容器容量沿既有仓库规则面（可被 JS 覆盖）；快照按既有 `ResourceStack`/命名掉落堆编码往返。市场订单与跨房发送在订单创建、send 保存、全局消费三处显式拒绝声明资源并登记后续入口。

现行契约见[资源目录](../../architecture/mod-resource-catalog.md)，机器汇总见[JSON](n2a-mod-resources-2026-10-05.json)。交付前完成代码复审、修复及完整门禁。

## 实现与评审

`kk-mod-api`：`Contribution.resources` + 冻结合并（跨命名空间/重复拒绝）、`ContributionPlan.resources()` 与 `ResourceCatalog`（含冒号分流查找、`player_resources_all`、掉落遍历序）。`kk-mod-host`：sdk.js `api.resources.define`（未知选项 fail-loud）、convert `resource` 声明（含绕过 SDK 的形状拒绝）、组合报告 `resources` 字段与 report_version 3。`kk-engine`：Engine/Plane 携带目录，transfer/withdraw/drop/领域能力/超容掉落改读目录；市场/跨房保持内建白名单并补全局阶段双门。`kk-runtime`：分片创建时把声明资源合成玩家 `RESOURCES_ALL` 覆盖（canary 与真实玩家同值；无声明不注入）。`kk-cli`：sim/inspect 报告版本 3。

## 功能验收

定向 Rust 测试：mod-api 冻结 5 项、resources 单元 2 项、mod-host 声明 2 项、engine 结算 5 项（transfer/withdraw、未声明拒绝、掉落堆+拾取、超容掉落目录序、市场/跨房三道门）、kk-world 快照往返 1 项。

独立 ore/beacon 夹具不使用专用 Rust 分支：ore 声明 `ore:crystal`/`ore:dust` 并覆盖 container 容量 50；beacon 依赖 ore、声明 `beacon:spark` 并在 link tick 写 state。三个场景各 100 tick、JSON/KKPV 双通路全等：只选 ore 时玩家 VM 内完成 提取→转入容器（容量=覆盖后的 50）→掉落→拾取，storage 剩 80、容器 dust 30、携带 19（掉落堆衰减 1）；玩家面 `createOrder`/`terminal.send` 对 `ore:crystal` 预检返回 OK，结算三道门显式拒绝——订单簿保持为空、terminal 库存分毫不动（分叉锁进场景断言）；选 beacon 时 ore 自动加入、资源闭环与市场分叉断言保持、beacon state marked≥90；无 Mod 对照世界 `RESOURCES_ALL` 无此资源，withdraw/drop 预检 `ERR_INVALID_ARGS`。三个 PlanHash 互不相同，组合报告 `resources` 与 `selection` 双通路逐项一致。

## 原始报告

六份[场景报告](n2a-mod-resources-ore-json-2026-10-05.json)（三场景 × JSON/KKPV）与三份 inspect 报告（[选 ore](n2a-mod-inspect-select-ore-2026-10-05.json)、[选 beacon](n2a-mod-inspect-select-beacon-2026-10-05.json)、[排除相撞拒绝](n2a-mod-inspect-reject-collision-2026-10-05.json)）记录同一 release CLI 的身份；已逐项核对每场景两通路最终世界（对象、玩家 Memory、mod_states）一致。ore 与对照场景的断言在 7/37/100/137 tick 下均成立；combined 的资源闭环断言同价成立，其 `marked≥90` 依赖 link tick 计数，须 ≥90 tick（夹具 README 已注明）。

## 完整门禁

重新构建 release CLI，显式以 KKSCREEPS_BIN 指定该二进制执行 pnpm run check：格式、workspace Clippy、Rust 790 项通过/0 失败/11 项忽略、适配器 14 项通过、文档链接（271 个 Markdown/1563 链接）和源码规模检查（380 个文件均不超过 1000 行）通过。

固定 screeps-ok 提交 34a630efa42908c117c19c25d2350fa2387bd633，构建适配器后运行 ratchet：共 2914 项，1918 通过/89 失败/907 跳过，满足 floor 1918；相对 HEAD~1 转红/转绿均为 0，没有新增豁免。两条既有未命中豁免警告保留，不归因于本批次。

## 剩余范围

市场/跨房对声明资源的正式通路（订单、deal、send）与逐资源数值（容量、单价、堆叠上限）未开放，三道显式拒绝门已登记后续入口（N3 跨房与账户命令方向）。建造/生产消耗新资源随 N2b 建筑配方接通。场景初始放置 Mod 资源掉落堆仍只接受内建资源名；初始库存经容器/仓库 store 表达。玩家面暂无逐资源常量（如 `RESOURCE_CRYSTAL` 形态的具名常量），资源名直接以字符串使用。
