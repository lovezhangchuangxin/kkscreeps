# N2a 资源目录场景

三个场景共用 `main.js`（按 `RESOURCES_ALL` 是否包含 `ore:crystal` 分支）：

- `scenario.json`：只选 ore。玩家 VM 内完成 提取 crystal/dust → dust 转入 Generic 容器
  （容量来自 JS 覆盖后的 container 规则 50）→ crystal 掉落 → 拾取 的真实闭环；
  断言 storage/容器库存、玩家 Memory 与掉落拾取。
- `scenario-combined.json`：只选 beacon，ore 经依赖闭包自动加入；另放置 link 让
  beacon 的 link.tick state 生效。断言资源闭环与 `mod_states/beacon/marked`。
- `scenario-plain.json`：无 Mod 的对照世界。玩家常量目录无 `ore:crystal`，
  withdraw/drop 在玩家预检即返回 `ERR_INVALID_ARGS`。

`scenario-combined.json` 的 `marked≥90` 断言依赖 link tick 计数，运行 tick 数须 ≥90（其余断言 ≥7 即稳定）。三个世界的 PlanHash 互不相同；JSON/KKPV 双通路最终世界逐项一致
（验证记录见 [N2a 交付](../../../docs/research/notes/n2a-mod-resources-2026-10-05.md)）。
