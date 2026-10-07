# mod-intent-suppression 压制声明包（G07.b）

两个同形对照包，验证 `suppressedBy` 声明的端到端语义：

- `suppress/`（包 id `suppress`）：counter 类型 + `suppress:bump`（置
  amount）+ `suppress:mark`（置 mark，**声明被 bump 压制**）。两意图都设
  绝对值，期望不随 tick 累积。
- `free/`（包 id `free`）：同形对照，不声明压制——两意图都按键序执行
  （bump 先置 3、mark 后置 7）。查询全局用 `FIND_FREE_COUNTERS`，与
  suppress 包组合选择时不撞全局名（玩家类名同理由 Counter 改为
  FreeCounter）。

压制语义：同接收者同 tick、压制者键存在即跳过被压制键、只看存在不看
压制者自身成败（其参数越界被丢弃仍压制）——与 vanilla
`Action::executes` 同源；冻结校验要求同 scope 同接收者、未知压制动作与
互压环拒绝。

sim 场景（`../sim-mod-intent-suppression/`）与验收见该目录 README。
