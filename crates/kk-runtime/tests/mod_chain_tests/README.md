# mod_chain_tests 夹具契约

本目录测试 Mod 处理链（资源/移动/战斗槽、自有槽、扩展槽）与引擎候选的交互。夹具的隐性契约集中在此——**写新测试前先读一遍，可把夹具试错从多轮压到一轮**。共享工厂与读面助手在 `harness.rs`，历史助手名（`mover/fighter/enemy/creep_at/pos/pos_in/hits/events/observe_pkg/predictable_id`）保持不变，直接 `use harness::…`。

## 引擎与包

- `engine(&[("alpha", src)], kkpv)` 建主房 W1N1（rcl 6，owner p1）+ 玩家 p1；`engine_with_shard` 自定义玩家/多房。
- Mod 源码必须经 `package(body)` 包裹（`export default function register(api){…}`）——裸函数体会以 `ReferenceError: api is not defined` 注册失败。
- 每个 `[false, true]` 双通道循环覆盖 JSON 与 KKPV 注入；两通道玩家可见面必须一致。

## 放置契约（place_object）

| kind | 支持字段 | 坑 |
|---|---|---|
| `creep` | name/owner/pos/body/store/ticksToLive/boosts | **hits 不可放置**，恒为 body×100——预伤害/治疗测试须先用真实攻击造伤；spawning 状态不可放置 |
| `structure`+spawn | name/owner/pos/hits/store | 走类型化 Spawn 变体 |
| `structure`+Generic（tower/rampart/container/link 等） | pos/owner/**hits**/store | hits 可直接放低值（结构摧毁测试用它） |
| `structure`+extension | pos/owner | **落成的是 constructionSite 不是建成结构** |
| `constructionSite` | pos/structureType/owner | 建造动作的目标 |

## 投影契约

- world_snapshot 沿原生投影不含 Mod 对象；验证 Mod 增删回滚或动态对象全集用 mod_object_snapshots，初始 alias 或已知 ID 的字段可用 get_object。
- 事件流**每 tick 整体替换**：断言只反映最后一次 `run_player`/`run_players` 的那一轮。
- road 的 `ticksToDecay = next_decay_time − time`：跨 tick 断言须计入时间推进。
- creep 的 `actionLog` 不进对象投影——动作日志变化只能经对象快照恢复间接锁定。
- `Game.creeps`/`Game.spawns` 只含**己方**对象；敌方目标经 `Game.getObjectById('<字面 id>')` 注入（玩家代码用 `format!` 拼）。

## 对象 ID 契约（mod 源码与运行期通信）

Mod 源码在包构建时固定，**读不到运行期 ID**。两条通道：

- 玩家→mod：不可行（Memory 属玩家 VM）。
- 可预测 ID：`harness::predictable_id(n)`——放置序第 n 个对象（1 为房间基设对象如控制器，首个手动放置为 2）。改向目标**首个放置**、字面量嵌进 mod 源码、放置后断言命中；假设破裂立即报错而不是静默测错。

## 处理器链契约（写 Mod 测试源码时）

- **按执行者门控**：挂链槽后全房同类动作都过链。拒绝/改向 handler 必须 `ctx.target.fields.name === '<目标>'` 门控，否则误伤同房其他执行者。
- **选择性 replace 对非目标执行者裸 return = 静默取消默认中心**：非目标分支必须 `ctx.<能力>.request(<原参数>)` 重发。
- replace 返回 `undefined` 即取消默认中心；wrap 才有 `next`（恰一次）；decline 五位置=before/wrap 前/replace/wrap 后/after。
- 能力（`ctx.resources/movement/combat`）只在所属处理器同步期内有效；战斗能力**恰一次消耗**（request/applyDamage/applyDamageBatch/applyHeal/cancel/native 先到者消耗，之后调用是故障）。
- 玩家面 API 与 FIND 常量注意：`FIND_HOSTILE_STRUCTURES` 含控制器而控制器无耐久面——攻击目标选择要按 `structureType` 过滤，否则 bot 静默空转。

## 动作次序（EXECUTION_ORDER 常用段）

`transfer < heal < rangedHeal < dismantle < attack < harvest < move < repair < build < rangedMassAttack < rangedAttack < say < suicide`——"此前动作保留/后续动作继续"断言按此选动作对。

## 双玩家与并发

`engine_with_shard` 声明多玩家多房；`e.set_player_concurrency(n)` + `e.run_players(&codes)`（codes 须在放置后以字面 id 构造）做串行=并发一致性断言（整 `world_snapshot()` 相等）。

## 自有对象生命周期

`object_tick.rs` 覆盖同步 tick；对象声明的 tick 自动派生 `<typeKey>.tick` 槽与 `<typeKey>.tick.process` 绑定。生命周期 args 为 null，无玩家授权前提；decline 使用 `return api.decline(code)`，在 tick 中明确拒绝。对象动作先执行，tick 读取随后候选；异常撤销整轮，缓存玩家在恢复后不重跑。

对象操作案例在 object_ops.rs：ctx.objects 查询 Mod 同房候选，写入限于绑定 namespace 的类型；get 返回 detached snapshot，update 部分合并，自删后 target.fields=null。动作中新生本轮 tick、tick 中新生下轮 tick。拒绝撤销对象、位置/格内顺序与 ID；故障则丢弃整轮。

伤害策略案例在 damage_policy.rs：ctx.combat.get 返回同房耐久/部件脱离副本；applyDamage 只在近战/单体远程槽开放，不复用默认功率、距离或 rampart 改向；当前调用的数值/越界/已结束处理器能力故障不能被 catch + decline 掩盖；跨调用旧 token 拒绝访问，捕获后可继续新调用。

治疗策略案例在 heal_policy.rs：applyHeal 仅在近/远程治疗槽开放，与 native 共用延迟队列及身体恢复；覆盖同轮致死伤害抵消、远处目标拒绝保留此前队列/转账/后续移动、孵化目标、非法/重复消耗和生命周期、缓存重试。

群体策略案例在 mass_policy.rs / mass_policy_faults.rs：战斗分页脱离读面，批量一次消耗；多目标/结构/spawn 孵化对象的局部恢复，原始 op 小数/转换重入/能力门，缓存重试；玩家 getObjectById 查询须使用规范长 ID，短 ID 仅宿主 ObjId::parse 能力入口支持。

反击表专项在 `retaliation_policy.rs`/`retaliation_faults.rs`：native/request/JS 单体及群体组合、实际 rampart 命中、结构荆棘、safeMode 策略替换、拒绝保留此前双队列、反击致死缓存重试，以及转换/消耗/能力过期。孵化目标通过真实 spawnCreep 创建；place_object 会归一化为已出生，不能用 spawning 字段伪造此夹具。
