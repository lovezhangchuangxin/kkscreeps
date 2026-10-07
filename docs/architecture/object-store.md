# 自有对象库存

`api.objects.define` 的 `capacity` 声明让自有对象类型携带资源库存：对象创建（结算期 `ctx.objects.create` 与场景/管理放置）即写入 `store`/`storeCapacity` 信封键（初始 `store` 为 `{"energy":0}`，与 Mod 建筑完工同形），库存只能经资源动作变动。容量随类型冻结进计划身份，不进仓库规则族（那是内建结构规则）。

```js
export default function register(api) {
  api.objects.define('mymod:crate', {
    version: 1, playerClass: 'Crate', positionFields: ['x', 'y'],
    capacity: 300,
    fields: { x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 } },
    tick(ctx) { ctx.state = { energy: ctx.target.fields.store.energy }; },
  });
}
```

## 消费链

信封语义与 Generic 仓库同形同源（`resources/generic.rs` 的解析与变更），六链接纳 Mod 对象：

| 链 | 位置 | 语义 |
|---|---|---|
| creep transfer 结算 | `settle/creep/store.rs` | Mod 对象作为接收端：容量门、邻接、事件与 Generic 同款 |
| creep withdraw 结算 | `settle/creep/store.rs` | Mod 对象作为提取源；结构专属拒绝（nuker/powerBank/terminal 封锁）不适用 |
| Mod 资源槽 | `settle/resources.rs` | `ctx.resources.read/transfer` 触达 Mod 对象（同房候选内） |
| 玩家预检 | prelude `creep-resources.js` | `type:"mod"` 目标族经其玩家类绑定接收；容量门读投影信封，错误码与结算同源 |
| 投影 | `mod_record_json` | 信封键直供玩家记录（公开口径与结构 store 一致）；玩家类经 `api.view.field(this, 'store')` 读 |
| 对象 tick | `ctx.target.fields` | payload 含信封键，Mod 执行期可读库存 |

`store`/`storeCapacity` 是投影保留键：字段声明占用在玩家面导出时拒绝。字段更新（`ctx.objects.update`）拒绝写信封键（「库存信封键由资源动作维护」）；字段校验拆出信封后针对声明字段执行。逐 tick 世界校验按类型声明核对信封在场：未声明 store 却携带信封、声明 store 却缺失信封、信封数值非法（store 非负整数映射）都拒绝结算。

## 边界

首片为单一共享总容量（未绑定资源共用剩余容量，与 Generic 共享仓同语义）；逐资源容量槽、实验室式槽绑定、损耗与跨房转账未开放（Mod 可在资源动作槽内组合）。对象死亡掉落、耐久与受击属 G09.b/c。库存随 `ModObj.data` 以不透明 JSON 持久化——检查点、KKPV 通道与冷恢复无需版本变化；无存量用户，旧计划（无 store 声明）不受影响（`store_capacity` 序列化省略 None，计划身份不变）。

已知角落（首片口径）：声明 `structureType` 字段且取值 `terminal` 的类型会让 withdraw **预检**施加终端封锁拒绝，而结算不适用（结算权威，预检偏严方向）；Mod 对象不参与 `object_owner`——敌方私 rampart 下任何人不可提取，声明 owner 字段不豁免（与 Generic 结构不同），预检侧同场景可能放行后由结算静默丢弃；字段写回（`ctx.target.fields`）传入的信封值一律丢弃并回填世界当前信封——槽内先读后写不会回滚资源转账。

夹具 [mod-objstore](../../compat/fixtures/mod-objstore/README.md)（hauler crate 300 / vault cache 100 两独立包 + 无容量 marker 对照）与场景 [sim-mod-objstore](../../compat/fixtures/sim-mod-objstore/README.md)（四轮双 creep 探针：预检返回码、提交后信封读值、满载 -8/无容量 -7 对照、组合共存、双通道与冷恢复）；Rust 侧断言见 `crates/kk-cli/tests/sim_tests/obj_store.rs`、结算域 `settle_tests/mod_store.rs` 与资源/对象单元测试。
