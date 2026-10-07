# Mod 资源目录

可信 JS Mod 可经公共 SDK 声明**命名空间资源**：键即玩家面与库存槽中的资源名（如 `ore:crystal`），经声明转换与冻结进入计划。资源目录与内建 `RESOURCES_ALL` 合并后贯穿常量同步、玩家 API 预检、清洗结算与投影；市场与跨房发送在开放前显式拒绝声明资源。

```js
export default function register(api) {
  api.resources.define('ore:crystal');
  api.resources.define('ore:dust', {});
}
```

## 声明与冻结

- SDK 入口 `api.resources.define(key, spec?)`：键必须以本包命名空间开头；`spec` 当前没有已知选项，非空对象整体拒绝（拼错形状静默落空等于改写目录语义）。绕过 SDK 直连 `op_mod_declare('resource', ...)` 同样按形状拒绝。
- 冻结期合并各包声明：跨命名空间键、重复声明整体拒绝。键含冒号且命名空间归本包，结构上不可能与内建资源名（均不含冒号）冲突。
- 资源目录参与 PlanHash（随贡献序列化进入规范哈希），并自然受[内容选择](mod-content-selection.md)门控——未选包不注册，其资源不在计划、玩家常量与结算目录中。
- 组合报告新增 `resources` 字段（键与定义来源），`report_version` 提升为 3；sim/inspect 顶层报告同步为 3。
- 恢复沿用分片私有快照重建并核对 PlanHash：资源目录变化会改变计划身份，被核验拒绝。

## 结算消费面

`ResourceCatalog`（`kk-mod-api/src/resources.rs`）在装配期从冻结计划解析一次，随 `Plane` 进入每房结算。含冒号的名字只查声明目录（二分），不含冒号的名字只查内建表——两条路径互不污染：

| 消费者 | 行为 |
|---|---|
| creep transfer/withdraw/drop、拾取 | 目录内资源走既有资源槽链路、事件与局部恢复边界 |
| 超容/死亡掉落顺序 | 内建声明序在前，声明键字典序随后 |
| Mod 领域能力 `ctx.resources` | read/transfer 接受目录内资源 |
| Generic 仓库/容器容量 | 沿既有[仓库规则](store-rules.md)（可被 JS 覆盖），不新增类型白名单 |
| 库存表示 | `store_extra`/Generic `store` 的既有任意命名槽，无新字段宽度 |
| 快照 | `ResourceStack`/命名掉落堆按既有编码往返 |

拾取与 transfer/withdraw/drop 共用当前资源目录的准入约束；即使快照或宿主放置了目录外的命名掉落堆，也不转入 creep 库存、不扣除拾取量、不移除掉落堆。正常对象阶段的掉落物衰减仍继续执行。

## 玩家常量与投影

计划声明的资源在分片创建时合成为玩家 `RESOURCES_ALL` 常量（内建序在前，声明键字典序追加），与 `constant_overrides`/StoreRules 同源同批：预检 canary 与真实玩家 spawn 读同一值。**无声明资源时不注入覆盖**——零 Mod 与纯内建目录世界的玩家面保持 vanilla 常量原值。玩家 Store 包装器、`RESOURCES_ALL` 缺省 0 语义、JSON/KKPV 投影都按既有通路工作，新资源数量进入两种投影。

## 当前边界：市场与跨房显式拒绝

市场订单、terminal 跨房发送**只认内建资源**：玩家面预检随目录放行（返回 OK），结算层在订单创建（`market/orders.rs`）、send 保存（`structures.rs`）与全局阶段消费（`market/terminal.rs`）三处显式拒绝声明资源（键含冒号的名字不进内建表），不产生订单、不转移库存，也不扣手续费。管理注入入口 `place_market_order` 沿用同一白名单，不开第四条旁路。全局阶段的新门同时收紧了此前仅能经宿主手工 stow 表达的跨 shard 资源 send（玩家面本就到不了该路径，属收紧而非回归）。正式的 Mod 资源市场/跨房通路登记为后续批次（N3 跨房与账户命令方向）；开放前以上门不得放宽。

首片没有逐资源数值（单价、堆叠上限）：容量沿仓库规则面，价格属于市场面。建造/生产消耗声明资源已随 N2b 建筑配方接通（见[可建造建筑](mod-buildings.md)）——成本向量与生产输入/输出按累计比例结算。场景初始放置 Mod 资源掉落堆（`place` 的 `resource` kind）仍只接受内建资源名；初始库存经容器/仓库 `store` 字段表达。

## 验收

- Rust：冻结合并/归属/重复/PlanHash（`kk-mod-api/tests/freeze/resources.rs`）、声明形状（`kk-mod-host` declarations）、结算闭环/拒绝/市场跨房（`kk-engine/tests/settle_tests/mod_resources.rs`）、快照往返（`kk-world`）。
- 独立 JS 夹具 [mod-resources](../../compat/fixtures/mod-resources/README.md) 与三场景 [sim-mod-resources](../../compat/fixtures/sim-mod-resources/README.md)：只选 ore、选 beacon（依赖闭包自动加入 ore）、无 Mod 对照世界；JSON/KKPV 双通路最终世界逐项一致，三个 PlanHash 互不相同。验证记录见 [N2a 交付](../research/notes/n2a-mod-resources-2026-10-05.md)。
