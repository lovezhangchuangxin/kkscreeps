# 建筑 RCL 配额规则

结构数量配额（官方 `CONTROLLER_STRUCTURES` 表：每结构类型 × 控制器等级的可建上限）现为冻结的 `EffectiveRules` 规则族，JS Mod 可在注册时覆盖；玩家预检与权威结算读同一冻结值。`StructureQuotaSnapshot` 在计划装配时解析一次，随每轮 Plane 进入结算；玩家侧 `CONTROLLER_STRUCTURES` 常量由同一快照折叠注入（键形与官方表一致），`createConstructionSite` 预检与 `isActive` 判定因此与结算同源。没有 Mod 时折叠表与官方表逐键相等，玩家可见行为不变。

```js
export default function register(api) {
  api.rules.override('builtin:structure.quota.tower.rcl8', 20);
  api.rules.override('builtin:structure.quota.constructedWall.rcl8', 3);
}
```

## 已开放的数值

规则键为 `builtin:structure.quota.{type}.rcl{N}`：type 覆盖官方表全部 16 个内建结构类型，N 按官方键集（spawn/extension/road/container 有 0..8，其余 1..8），共 132 条；默认值等于官方表值，域为 0..=u32::MAX 整数。键集刻意与官方表同形：未覆盖时折叠出的玩家表与官方表逐键相等（含缺失键的 NaN 语义），覆盖只改写被覆盖的键。

| 消费者 | 位置 | 语义 |
|---|---|---|
| 工地创建结算 | `settle/room.rs` | 超过当前 RCL 配额的 createConstructionSite 意图无效 |
| link 转移 isActive 门 | `settle/structures.rs` | 配额为 0 时 link 视为未激活 |
| spawn/extension 放置 | `settle/spawn.rs` | 放置时按当前 RCL 配额清点 |
| 终端容量归一 | `settle/economy.rs` | 有控制器时按 RCL 配额门归一 |
| extractor 采集门 | `settle/creep/harvest.rs` | 配额为 0 时不可采集 |
| 玩家预检 | prelude `creation.js`/`structures.js` | 读注入的 `CONTROLLER_STRUCTURES`，错误码与结算一致（ERR_RCL_NOT_ENOUGH） |

结算侧统一经 `settle/structures.rs::allowed_for` 读取快照；快照未含的类型（Mod 建筑等非官方类型）回落静态官方表——Mod 建筑不占内建配额表，口径不变。旧 `builtin:link.rcl0..8` 九条规则已并入本族（键形改官方 1..8）：此前该族只进引擎不进玩家面，link 的预检读静态表，迁移后预检与结算同源，LinkRules 不再携带 rcl。

`isActive` 与 link/spawn 的权威动作门均按当前等级的名额、到控制器的距离及稳定对象顺序清点。RCL8 配额为 1 也不能省略清点：配额覆盖可以不单调，换包降低配额后已有结构也不会自动删除。超过当前配额的对象保留，但不占激活名额。

## 预检时序与同源

玩家预检在同一轮内只看已提交对象：单轮内连建多个工地全部通过预检，配额由结算收口；下一轮预检读「已提交数 vs 注入表」才产生 ERR_RCL_NOT_ENOUGH。预检表与结算快照派生自同一冻结计划（G02.d2/G07.a 同款双端机制），canary 预检、玩家 worker、检查点恢复与包切换重建都重注入同一折叠表。builtin revision 随本族进入 v18：计划指纹变化，旧检查点按身份门拒绝（无存量用户，不迁移）。

## 当前边界

配额只约束内建结构类型；逐房差异、动态配额（tick 期决定）与 Mod 建筑专属配额未开放。资源容量族见[仓库容量规则](store-rules.md)；规则机制与冻结语义见[有效规则读取](mod-rules.md)。

夹具 [mod-quota](../../compat/fixtures/mod-quota/README.md)（两独立包：抬塔/压墙）与场景 [sim-mod-quota](../../compat/fixtures/sim-mod-quota/README.md)（原生对照/单包/组合 × 双通道 + 冷恢复）经真实 CLI 验证三面互证：预检放行数、世界工地数与注入表读值一致。
