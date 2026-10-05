# 仓库容量规则

Generic 仓库的默认容量现从冻结的 `EffectiveRules` 读取，JS Mod 可在注册时覆盖，处理器执行期可经[有效规则读取](mod-rules.md)读取同一冻结值。`StoreRules` 在计划装配时解析一次；放置、建造、实验室槽切换、终端 tick 和标准玩家常量共用该快照。没有 Mod 时保持官方默认值，不增加 JS 结算调用。

```js
export default function register(api) {
  api.rules.override('builtin:store.container.capacity', 1000);
  api.rules.override('builtin:store.lab.energyCapacity', 100);
  api.rules.override('builtin:store.lab.mineralCapacity', 200);
}
```

## 已开放的数值

所有键带 `builtin:` 前缀，接受 0..=u32::MAX 的整数。冻结阶段拒绝越界、小数与未裁决的多来源覆盖冲突；规则和来源内容改变计划指纹。多资源结构的总容量用 i64 相加，不截断为 u32。

| 规则键（省略 builtin:） | 同步的玩家常量 | 使用位置 |
|---|---|---|
| `store.container.capacity` | CONTAINER_CAPACITY | 放置、建造、同格资源自动承接 |
| `store.factory.capacity` | FACTORY_CAPACITY | 放置、建造 |
| `store.terminal.capacity` | TERMINAL_CAPACITY | 放置、建造、终端每轮容量归一 |
| `store.tower.capacity` | TOWER_CAPACITY | 放置、建造、energy 槽 |
| `store.lab.energyCapacity` | LAB_ENERGY_CAPACITY | 放置、建造、energy 槽、空矿物仓恢复 |
| `store.lab.mineralCapacity` | LAB_MINERAL_CAPACITY | 放置已有矿物、首次绑定矿物槽、取空后恢复 |
| `store.powerSpawn.energyCapacity` | POWER_SPAWN_ENERGY_CAPACITY | 放置、建造、energy 槽 |
| `store.powerSpawn.powerCapacity` | POWER_SPAWN_POWER_CAPACITY | 放置、建造、power 槽 |
| `store.nuker.energyCapacity` | NUKER_ENERGY_CAPACITY | 放置、建造、energy 槽 |
| `store.nuker.ghodiumCapacity` | NUKER_GHODIUM_CAPACITY | 放置、建造、G 槽 |

link 保持既有 `builtin:link.capacity`。spawn、extension、storage、creep 携带量及 boost 数值尚未迁入这组规则。

内置贡献仍使用历史入口名 `builtin::link()`，现在同时声明 link、creep 资源、移动、战斗与建造族槽及仓库规则；默认规则目录为 25 条，内建槽共 13 个。数值消费者具名绑定为 `store.init/build/constants/labSlots/terminalTick/containerDeposit`，它们不是可挂 JS handler 的动作槽。

## 权威字段与玩家行为

`storeCapacity` 和 `storeCapacityResource` 仍是对象的实际容量，JSON/KKPV 投影直接提供这些字段，玩家 Store getter 与 transfer/withdraw 预检优先读取它们。放置时显式字段优先于默认值；这组规则不按类型重新否决已有 Generic 仓库，也不增加类型白名单。

实验室首次装入非 energy 资源时按规则绑定矿物容量，取空后恢复能量加矿物的总容量。显式场景容量只在放置时保留，后续实验室绑定/释放沿用这组默认规则。终端有控制器时按原归属及 RCL 条件归一为有效规则容量或 0，无控制器时保留字段原值；覆盖容量不绕过归属、配额、干扰或提取限制。

掉落、采集溢出及死亡资源通过共享的容器承接入口，按全槽合计库存和冻结的 container 容量计算余量，剩余资源继续进入掉落物或墓碑。这个入口沿用原来的全局容量口径，不使用场景的单对象显式容量覆盖。

玩家显式请求超过可用容量仍由现有 API 返回错误，省略数量时按已提交视图计算可转数量；结算再次按本轮候选余量钳制。同轮两个 creep 的 API 都可能返回 OK，后一个动作仍可能因先前转入已填满仓库而不产生效果。零容量目标保持原有 API 判断，不承诺返回 ERR_FULL。

标准全局常量在玩家沙箱构造时同步；Mod `player.js` 的安装预检和真实安装读取相同的规则值。模块重建复用已注册配置，结算恢复重新冻结原包快照并核对计划；停用处理器不撤销已经冻结的数值覆盖。

## 当前边界

这是 D4 容量数值的第一片；后续已开放 [creep transfer/withdraw 处理槽](creep-resource-slots.md)及正常拒绝。实验室槽绑定策略、nuker/powerBank 禁止提取或终端干扰策略仍未逐项开放。link transfer 和 creep 资源动作已接入执行者/目标、事件与 Mod state 的局部恢复；其他内建多对象动作须随处理槽定义自己的撤销边界，见[Mod 执行](mod-execution.md)。规则已冻结，但没有完整检查点、规则迁移或运行中容量重配置。

实现与验收见[交付记录](../research/notes/d4-store-rules-2026-10-05.md)。
