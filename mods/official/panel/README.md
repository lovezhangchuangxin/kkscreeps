# panel 官方内容包

第二个官方建筑包：与 smelter 同一公共接口、完全不同的规则——
单资源成本（仅能量）、道路寻路语义（可通行且按道路成本）、免费生产
（无输入，产能量）、低耐久。用于证明建筑面没有针对首个样例硬编码，
两个包可同时安装在同一世界（见 sim-mod-buildings 的组合场景）。

## 玩法规则

- `panel:collector`：成本 `{energy: 800}`，耐久 300，`pathing: 'road'`
  （不阻挡移动，寻路成本 1）。
- 每 20 tick（配置 `interval`）免费产出 40 能量（配置 `yield`），
  容量 200，产出满后停产。

## 玩家面

`player.js` 绑定 `PanelCollector extends Structure`；`collector.charge`
读当轮能量库存。
