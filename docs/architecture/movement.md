# 移动领域：登记、仲裁与策略

移动按拉拽登记 → 请求预检 → 竞争选胜 → 准入 → 障碍追加 → 取消链 → 代价 → 对象 tick 执行。各阶段的候选、拒绝与读取时点见下文；玩家门与寻路边界见[玩家预检规则](player-precheck-rules.md#移动玩家门g02d2)。

## 代码与维护入口

纯数据在 `kk-mod-api/src/movement/`，候选与仲裁在 `kk-engine/src/settle/movement/`，候选适配在 `settle/working/movement.rs`，JS 操作在 `kk-mod-host/src/chain/movement/`，运行时委托在 `kk-runtime/src/embed/mod_chain/movement.rs`。旧公开模块路径保留重导出；新增能力使用领域入口。

## 拉拽登记

`builtin:room.movePullRegistration` 是拉拽意图登记后的房间级策略槽：全部拉拽意图（含其经原生校验：相邻、非孵化、循环链拒绝）登记完成后、移动请求预检之前整房一次进入 JS，Mod 可对配对做**单向否决**——被否决配对在预检输入构造之前双向移除，**等同从未登记**。它补齐移动域的最后一个登记期决策点，与预检槽（请求级否决）、竞争槽（排序）、准入槽（终值覆盖 + 链预演）、障碍槽（胜者级阻挡）构成五阶段开放链。

### 拉拽登记：执行时点与输入

`arbitrate_room` 依序执行：**拉拽登记 → 预检 → 竞争 → 准入 → 障碍 → 取消链**。拉拽否决最先应用：被否决配对的 `pulled/pulling` 标记不进入预检/竞争/准入视野，准入预填的 `effective_pulled` 与执行期的疲劳转移（被拉者疲劳记给拉者）同源反映。

输入 `ctx.args.pulls` 为整房有效配对，按拉者对象 ID 升序、不截断：

- `pullerId/targetId`：对象 ID 字符串；
- `pullerX/pullerY/targetX/targetY`：双方当前位置（登记时点已相邻）；
- `pullerOwner/targetOwner`：归属玩家（供跨主人策略）。

任一侧对象已从房间移除（或不再是 creep）的配对不进输入、不可否决（拉者侧由配对复核按"保留现有标记"处理，目标侧残链由让位链的对象存在性检查断链）。无有效配对的房间不进入 JS，原生整链不付费。

### 拉拽登记：能力面与边界

- `ctx.pull.isDenied(pullerId)` / `ctx.pull.deny(pullerId)`：读取与追加否决；id 须为本次输入中的拉者，域外对象报错而非静默收窄。
- **单向收紧**：没有放行或新增方向。新增拉拽没有属主意图——疲劳转移会向未行动玩家计费，属宿主表示约束；这是新增拉拽的意图/计费边界，不能类比静态障碍放行（见[移动预检](#请求预检)的 G02.d1）。
- 正常拒绝（`api.decline`）整房丢弃全部否决，配对表保持原状；整槽故障按既有 Mod 故障契约重试/停用，停用后槽回退 native 配对。
- 否决键是拉者：重复登记后到覆盖先到（见下），每拉者至多一条有效配对，键无歧义。

### 拉拽登记：登记一致性与复核顺序

同批把 `add_pulling` 的重复登记改为**后到覆盖先到并保持两表双射**：拉者换目标时清除旧目标的被拉标记，目标换拉者时清除旧拉者的拉动标记。此前覆写会留下单向残标，配对复核（`validate_pull_pair`）中首个失败配对的清理会连带清除当前配对，结果依赖胜者遍历序（对象创建序，任意）——开放登记策略前该缺陷不可达（原生意图流每拉者每 tick 至多一条 pull 意图，由指令表结构保证），但任何未来的新增方向都会触达。双射之下每个失败配对的清理只触及自身两侧，复核与遍历顺序结构无关，槽输入的 `pulling` 标记也不再出现已失效的残标。

原生意图流每拉者每 tick 单目标不变量不变（`BTreeMap<ObjId, BTreeMap<Action, IntentData>>`）；多拉者同目标时后到覆盖先到，旧拉者标记即时清除。

### 拉拽登记：组合

- 与预检槽：否决配对后预检视野 `pulled/pulling` 均为假（等同从未登记）；预检自身否决拉者**请求**的既有语义不变（配对保留，被拉者在链内取消——两级否决的下游不同）。
- 与准入槽：否决配对即改变准入预填的原生初值（被拉豁免消失）；准入槽仍可在其上覆盖终值。
- 多包 replace/after 组合并集，与既有槽一致。

### 拉拽登记：验收

引擎测试（输入完整性、否决等同从未登记的逐阶段等价、候选域校验、双射与遍历顺序无关、离房拉者边界）与运行时测试（字段形态 + 原生对照、decline 回退、组合、能力门、故障停用回退）见 `settle/pull_registration/tests.rs` 与 `mod_chain_tests/move_pull_registration.rs`；anchor（全量禁拖）/leash（反绑架：跨主人否决）两独立包 + 原生对照 + 组合的四场景 × JSON/KKPV 双通道与检查点冷恢复见 [sim-mod-move-pull-registration](../../compat/fixtures/sim-mod-move-pull-registration/README.md)。证据见[交付记录](../research/notes/g03b-pull-registration-2026-10-06.md)。

## 请求预检

`builtin:room.movePrecheck` 在全部意图与移动动作槽登记完成后、竞争输入构造之前整房一次进入 JS。2026-10-07 的 G02.d1 增加 `ctx.precheck.allowStatic(id)`：独立 JS 包可以放行墙地形、结构、私有敌方 rampart 和己方障碍工地；`block(id)` 仍是不可撤销的请求级否决。被否决请求从登记矩阵移除，等同从未登记，不参与竞争、不让位、不占格。

顺序为：拉拽登记策略 → 本槽 → [竞争排序](#竞争排序) → [准入与链预演](#准入与预演) → [障碍追加](#障碍追加) → 原生取消链 → [移动代价](#移动代价) → 对象 tick 位移。静态放行随同一请求传给准入预演和实际取消链；动态占格、安全模式、获准让位与级联取消仍按既有规则处理。

### 请求预检：登记与原生回退

两个入口 `intent_move`（玩家意图）和 `slot_request_direction`（移动槽能力）共用 `movement/static_obstacles.rs` 的结构判定：

- 无预检贡献时，结构/私有 rampart 在登记时直接拒绝，保持原生路径；墙和己方工地仍在仲裁时拒绝。
- 有本槽贡献时，原生静态拒绝的请求也暂存进入输入，同时保存**登记时的拒绝快照**。成功后先过滤未放行的登记拒绝与显式 block，再构造竞争输入；放行绑定请求者，不会借同格落选者的许可放行胜者。
- 空处理器或正常拒绝仍兑现登记快照：即使结构在后续动作中被删除，也不能把原本未登记的请求误放行。相反，登记后新增的障碍按仲裁快照复核；显式 allowStatic 可覆盖该静态障碍。
- 坐标、方向、ID、孵化与候选提交约束继续由宿主校验；静态玩法障碍属于可替换决策。

请求级否决与障碍槽胜者级否决不同：预检移除强者请求后弱者可独占目的格；选胜后挡住强者不会重新选弱者。障碍槽的追加 block 也能挡住静态放行者。

### 请求预检：公共 SDK

有本槽贡献且存在请求时进入 JS；无请求不调用。`ctx.target.fields` 恒为 null。`ctx.args.requests` 按对象 ID 升序、不截断，每对象至多一条：

| 字段 | 含义 |
|---|---|
| `moverId/owner` | 请求者 ID 与归属 |
| `originX/originY`、`targetX/targetY` | 原格与目的格 |
| `movePower/weight` | 含 boost 的 MOVE 功率与移动重量 |
| `pulled/pulling` | 预检时点的原始拉拽标记，配对一致性复核在准入预填发生 |
| `terrainWall/terrainSwamp/hasRoad` | 目的格地形与道路摘要 |
| `objects[]` | 目的格对象：`id/kind/owner/isPublic/structureType`；尚无让位预期 |
| `nativeRegistrationBlocked` | 登记时结构/私有 rampart 的原生拒绝快照 |
| `nativeStaticBlocked` | 预检时静态初判，含墙和己方障碍工地；不含 creep 动态占格 |

`ctx.precheck` 只能在所属处理器同步执行期使用：

| 方法 | 语义 |
|---|---|
| `isBlocked(id)` | 显式 block，或上述两静态初判任一为真且未 allowStatic；不预测动态让位 |
| `block(id)` | 对本次输入中的请求追加不可撤销否决 |
| `allowStatic(id)` | 放行本次输入中的请求的静态障碍；不能清除任何显式 block 或动态占格 |

因此 `allowStatic` 后 `isBlocked` 为 false 并不保证最终移动：竞争落选、疲劳/MOVE 门槛、动态占格及后续障碍策略仍可能拒绝。放行只在本房本 tick 生效，下一 tick 重新决策。

### 请求预检：独立 JS 组合

[ghost 与 seal 两个独立包](../../compat/fixtures/mod-move-static/README.md)分别实现穿越和区域管制：

```js
export default function register(api) {
  api.handlers.before('builtin:room.movePrecheck', { id: 'ghost' }, (ctx) => {
    for (const r of ctx.args.requests) ctx.precheck.allowStatic(r.moverId);
  });
}
```

```js
export default function register(api) {
  api.handlers.after('builtin:room.movePrecheck', { id: 'seal' }, (ctx) => {
    for (const r of ctx.args.requests) {
      if (r.targetX >= 30) ctx.precheck.block(r.moverId);
    }
  });
}
```

组合时 ghost 放行墙与私有 rampart，seal 保留区域否决；即使另一个处理器随后再 allowStatic，显式 block 仍然生效。native 中心不写 JS 决策，多包顺序经[组合诊断](mod-composition.md)冻结。既有 [fenced/courtesy 请求否决场景](../../compat/fixtures/sim-mod-move-precheck/README.md)继续适用。

### 请求预检：拒绝、故障与恢复

- `api.decline(code)` 丢弃整条链全部 JS 放行、显式否决和暂存 namespace state，再按登记快照恢复原生过滤；其他移动槽仍正常运行。这是整房回退边界。
- 域外 ID、非字符串、跨槽或异步过期调用走能力故障，不能被 catch + decline 掩盖；整轮不发布，按[Mod 执行恢复](mod-execution.md)重建 realm。
- 晚期写回失败重用玩家结果缓存，重新决策后世界与 state 恰一次提交；停用包重装后按有效链执行，全部贡献摘除则恢复 native 登记。
- 新集合是 tick 内瞬态，无新持久写入域；检查点只保存已提交世界与 Mod state。内建槽仍为 20，catalog revision 随本族演进（G02.d1 自 v14 升 v15、G02.d2 升 **v16** 现行），覆盖输入/能力语义变更；旧计划身份的检查点按已有身份门拒绝，没有自动迁移。

### 请求预检：验收与后续

[四个真实玩家场景](../../compat/fixtures/sim-mod-move-static/README.md)核对原生、ghost、seal、组合，JSON/KKPV final、冷启动续跑、namespace 恰一次与回放一致。引擎和运行时另覆盖墙地形/结构/私有 rampart/己方工地、不可逆否决、动态取消链、准入预演、移动槽入口、拉拽、源房障碍放行后跨房、decline/能力门/停用/失败重试。证据见 [G02.d1 交付记录](../research/notes/g02d1-static-movement-2026-10-07.md)；G02.c 的[历史证据](../research/notes/g02c-move-precheck-2026-10-06.md)保留当时的单向接口。

玩家 `move(direction)` 没有静态地形/结构预检，本片在正常身体和疲劳条件下可直接发出请求（疲劳/MOVE 门自 G02.d2 起读计划派生常量，见[移动玩家门](player-precheck-rules.md#移动玩家门g02d2)）。`moveTo`/PathFinder 仍使用原生寻路障碍，**不会自动规划穿墙路径**——放行决策在结算段产生，时序与粒度均不可被寻路感知；该边界已有回归测试钉死（墙列下 moveTo 沿部分路径止于墙前、ghost 放行下同 tick 玩家搜索仍绕行墙格）。声明式寻路规则（计划级可走性）登记为 G02.d3 设计。跨房到达格仍沿用既有原生过渡语义（不复检目的房占格/地形），本片不改写它。G02/G03 父项保持部分完成；孵化门槛、完整仲裁替换和障碍槽第二次预演属于后续切片。

## 竞争排序

2026-10-06 已开放 `builtin:room.moveContest` 房间级槽。独立 JS 包可改变同格竞争者的排序：输入提供全部竞争格与默认评分要素，JS 通过优先级数值候选调整排序，宿主按 优先级 → 默认四键 → 登记序 稳定排序选胜。这只是排序切片：准入门槛已另行开放为独立槽（见[JS 移动准入](#准入与预演)），获准集合与执行效果仍由原生路径决定（G03/G04 另行开放）。

### 竞争排序：输入与结算

全部 move/pull 意图登记完成后、仲裁开始前，房间内**存在多竞争者目的格**且有本槽贡献时，整房一次进入 JS；无贡献或无竞争格的房间不进入 realm，不构造输入（默认路径零开销，改造前后微基准无可测量回归，见[交付记录](../research/notes/g01-move-contest-2026-10-06.md)）。`ctx.args.contests` 按目的格给出，竞争者按登记序排列、**不截断**：

| 字段 | 含义 |
|---|---|
| `contests[].x/y` | 被竞争的目的格 |
| `contenders[].id/owner` | 竞争者对象 ID 与归属 |
| `contenders[].originX/originY` | 登记时的原格 |
| `contenders[].originRequestCount` | 原格被请求总数（默认评分第一键的原始值） |
| `contenders[].reciprocal` | 对穿：原格请求中存在站在本竞争格上的对象（此时默认第一键取 100） |
| `contenders[].pulled/pulling` | 被拉/拉者（默认评分第二、三键） |
| `contenders[].movePower/weight/powerRatio` | 含 boost 的 MOVE 功率、重量（≥1）与两者之比（默认评分第四键） |

`ctx.target.fields` 恒为 null（房间级槽没有对象目标；`target.id` 是诊断占位符，不可用于对象能力）；本槽不开放移动请求、资源、战斗或对象能力。非 creep 登记者按防御值填充（owner 空串、功率 0、重量 1；其默认评分为全 0，与原生一致——登记路径当前只有 creep）。`ctx.contest` 是槽专属能力（不可枚举、只在处理器同步期有效）：

| 方法 | 语义 |
|---|---|
| `getPriority(id)` | 读取当前选择；未设置为 0 |
| `setPriority(id, value)` | 写入或覆盖一个竞争者的优先级；id 须在本次输入中，value 须为 ±(2^53−1) 内的 JS 安全整数 |

排序键为 `priority` 降序，再回落默认四键（原格请求数/对穿 100、被拉、拉者、功率/重量比），完全平手保持登记序。未设置的竞争者处于 0（默认层）：正值抬升、负值压低、同值不改变原生结果。选胜之后仍按原逻辑取消不可移动或被阻塞者，不自动递补落选者；准入（疲劳/MOVE/被拉门槛）已按 G02.a 开放为独立槽（见[JS 移动准入](#准入与预演)），链取消与执行阶段同源消费准入终值，不再二次复核 `can_move`。

### 竞争排序：独立包组合

seniority 包替换排序（每格 ID 最小者优先，与默认“功率比最高者胜”相反）：

```js
export default function register(api) {
  api.handlers.replace('builtin:room.moveContest', { id: 'seniority' }, (ctx) => {
    for (const cell of ctx.args.contests) {
      let oldest = null;
      for (const info of cell.contenders) {
        if (!oldest || info.id < oldest) oldest = info.id;
      }
      for (const info of cell.contenders) {
        ctx.contest.setPriority(info.id, info.id === oldest ? 1 : -1);
      }
    }
  });
}
```

另一个 heaviest 包用 after 抬升每格重量最大者，可叠加在任意 before/replace 之上：

```js
export default function register(api) {
  api.handlers.after('builtin:room.moveContest', { id: 'heaviest' }, (ctx) => {
    for (const cell of ctx.args.contests) {
      let heaviest = null, best = -1;
      for (const info of cell.contenders) {
        if (info.weight > best) { best = info.weight; heaviest = info.id; }
      }
      ctx.contest.setPriority(heaviest, 2);
    }
  });
}
```

before/中心/replace/after 与其他槽同构：native 中心为无操作（保留 before 设置的优先级，类比受击槽的默认 TOUGH 中心）；after 在整槽成功后执行。多包平手关系按[handler 依赖与组合诊断](mod-composition.md)冻结。真实场景中三种排序选出三个不同胜者、组合由 after 层压回最重者，见[场景夹具](../../compat/fixtures/sim-mod-move-policy/README.md)。

### 竞争排序：拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条竞争链：丢弃全部优先级候选与暂存 namespace state，回退默认四键排序；此前已登记的请求与拉拽关系保持参与仲裁。已登记的其他 creep 请求不受影响。
- 优先级候选不直接改写世界。非法数值（非整数、超出 ±(2^53−1)）、输入外 ID、越权能力（移动请求/资源/战斗/受击）的故障不能被 catch + decline 掩盖；错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。
- 晚期写回失败经玩家缓存重试整轮：竞争结果与 namespace state 恰一次提交，不重复执行玩家代码。停用出错的 replace 后回退 native，健康钩子继续组合。
- 仲裁结果是 tick 内瞬态，不进入检查点；检查点只保存已提交世界与 Mod state。该批内建槽增至 15（现 18，后续批次见[JS 移动准入](#准入与预演)与[JS 移动预检](#请求预检)），新增冻结描述和 native 绑定改变 PlanHash；旧计划身份的检查点不会被当作同一玩法静默加载。

### 竞争排序：验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-move-policy/README.md)提供原生、seniority、heaviest 和两包组合；JSON/KKPV 完整 final 一致，竞争后保存退出与冷启动续跑等价。专项覆盖输入完整性与要素、非法数值/外部 ID、能力门、负优先级层、停用回退、晚期失败缓存重试与拒绝回退，结果见[交付记录](../research/notes/g01-move-contest-2026-10-06.md)。

障碍准入的仲裁半边已开放单向追加（见[JS 移动障碍](#障碍追加)），疲劳/MOVE/被拉门槛、整房请求视野与链预演见[JS 移动准入](#准入与预演)；静态放行已由[JS 移动预检](#请求预检)贯通，移动代价与 pull 登记策略分别见[移动代价](#移动代价)和[拉拽登记](#拉拽登记)；完整执行效果仍待 G04 后续片。意图静态预检已开为[JS 移动预检](#请求预检)（否决在竞争输入构造之前生效）。竞争槽输入只含竞争格；房间其余请求经准入槽的 `ctx.args.requests` 整房视野可见。JS 策略的结算开销由 Mod VM 预算统一管控，密集竞争场景的端到端对照随 G30.d 滚动补测。

## 准入与预演

2026-10-06 已开放 `builtin:creep.moveAdmission` 房间级槽。独立 JS 包可替换移动准入门槛（原生为疲劳 0 且有存活 MOVE，或有效被拉）：仲裁选胜后对全部获胜执行者整房一次进入 JS，候选初值即原生 `can_move` 结果，JS 可读可覆盖；准入终值**同源**约束链取消与执行阶段——`movement_execute` 不再按当前身体二次 native 复核，缺项一律未获准（fail-closed）。同日 G03.a 追加**整房请求视野**（`ctx.args.requests`，含竞争落选者）、**拉拽配对 ID**（`pulledBy/pulling`）与**链预演**（`ctx.admission.evaluate()`）：JS 得以按全房拓扑选择获准移动集合，覆盖让位链、交换、循环、拉拽与阻塞传播的可见性——级联后果由宿主用与正式仲裁完全相同的取消链代码预演，JS 不重推导。障碍的仲裁半边另行开放为单向追加槽（见[JS 移动障碍](#障碍追加)），意图静态预检仍由原生判定。

### 准入与预演：输入与结算

竞争优先级槽（若有贡献）先运行并选出各目的格胜者；随后仲裁在**链取消之前**暂停，把全部胜者的准入要素整房一次交给 JS；JS 终值确定后才取消未获准者及其阻塞链。房间内没有移动胜者时不进入 realm（无贡献路径零开销）。准入槽的触发面比竞争槽宽：竞争槽只在存在多竞争者目的格时进入 JS，准入槽对装有贡献的房间**只要存在移动胜者**即整房进 realm 并为每个胜者分配输入字符串；密集移动服务器的结算开销由 Mod VM 预算统一管控。`ctx.args.movers` 按对象 ID 升序、不截断：

| 字段 | 含义 |
|---|---|
| `movers[].id/owner` | 执行者对象 ID 与归属 |
| `movers[].originX/originY` | 登记时的原格 |
| `movers[].targetX/targetY` | 获准的目的格 |
| `movers[].oldFatigue` | 意图时疲劳（原生门槛的疲劳输入） |
| `movers[].activeMoves` | 存活 MOVE 部件数（原生门槛的部件输入） |
| `movers[].movePower` | 含 boost 的 MOVE 功率 |
| `movers[].weight` | 移动重量（≥1） |
| `movers[].pulled` | 有效被拉标记：配对复核的集合与旧链取消时点等价（拉者仍指向被拉者且被拉者获准移向拉者当前格，否则解除残标；拉者已移除时保留标记），遍历顺序为胜者对象 ID 升序。同一拉者多目标残标的顺序差异在现有意图流不可达（每拉者每 tick 至多一个 pull 目标）；开放 pull 登记策略时须重审该顺序语义 |
| `movers[].pulledBy` | 有效拉拽配对的拉者对象 ID（与 `pulled` 同点复核，配对失效按未拉处理）；无配对为 null（G03.a） |
| `movers[].pulling` | 登记的拉动目标对象 ID；目标竞争落选时配对残标不从拉者侧复核（本轮拉动不生效），无登记为 null（G03.a） |

`ctx.args.requests` 是**仲裁前的完整原始请求矩阵**（G03.a）：每个房间请求一条（含竞争落选者——他们不在 `movers` 中），按 目的格坐标、格内登记序 排列，逐条 `moverId/x/y/index/originX/originY/won`；`won` 标记该登记者是否为其目的格的仲裁胜者。原始矩阵在选胜时零拷贝暂存，Working 每房每 tick 重建，天然瞬态。

`ctx.target.fields` 恒为 null（房间级槽没有对象目标）；本槽不开放移动请求、资源、战斗或对象能力。非 creep 胜者按防御值填充（owner 空串、部件 0、功率 0、重量 1），原生准入为否——与原生一致。`ctx.admission` 是槽专属能力（不可枚举、只在处理器同步期有效）：

| 方法 | 语义 |
|---|---|
| `getAdmitted(id)` | 读取当前终值；初值即原生结果，输入外对象为 false |
| `setAdmitted(id, admitted)` | 覆盖终值；id 须在本次输入中，admitted 须为布尔 |
| `evaluate()` | **链预演**（G03.a）：按候选当前终值只读运行与正式仲裁完全相同的取消链（同一 `cancel_blocked` 代码，无第二套判定），返回会存活的胜者对象 ID 升序数组；不写世界、不改变候选。按调用时点即时计算而非快照——预演后继续 `setAdmitted` 或正常拒绝都会使该结果失效。预演时点在障碍槽之前——障碍追加只会收紧，预演结果是最终取消结果的上界 |

准入与排序解耦：竞争槽（`builtin:room.moveContest`）决定谁胜出，准入槽决定胜出者能否执行。放宽准入（如疲劳者放行）不等于绕过障碍——目的格被未获准让位者占据时仍按阻塞链取消；级联后果（交换/环/让位链上扣住一员导致整链取消）不必由 JS 推导，`evaluate()` 直接给出宿主链结论。获准集合选择因此闭环：**暂存候选（JS 布尔终值）→ 宿主链验证（预演/正式取消同代码）→ 终值整体替换原生预填**；宿主不重跑默认门槛覆盖 JS 结果。玩家面方向形态 `move()`/`moveTo()` 的疲劳与 MOVE 预检自 G02.d2 起读[移动玩家门](player-precheck-rules.md#移动玩家门g02d2)计划派生常量，与本槽原生预填同源（默认即官方门槛）；规则放宽后疲劳/无件移动者经方向形态直达仲裁，`move(target)` 结构通道与移动槽能力两条旧通道不变，准入覆盖对全部通道同样生效。

### 准入与预演：独立包组合

tireless 包 replace 全部放行（忽略疲劳/MOVE 门槛）：

```js
export default function register(api) {
  api.handlers.replace('builtin:creep.moveAdmission', { id: 'tireless' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      ctx.admission.setAdmitted(mover.id, true);
    }
  });
}
```

strict 包 after 收紧配重不足者，可叠加在任意 before/replace 之上——after 层总能把整面放行收回到自身门槛：

```js
export default function register(api) {
  api.handlers.after('builtin:creep.moveAdmission', { id: 'strict' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.movePower < mover.weight) {
        ctx.admission.setAdmitted(mover.id, false);
      }
    }
  });
}
```

G03.a 的 wave 包 replace 用链预演实现"车队全停"：预演发现任何已获准移动会被取消链级联吞掉时，本轮全房停下；calm 包 after 用整房视野把竞争胜者"还给"落选者重新排队。两者见[场景夹具](../../compat/fixtures/sim-mod-move-arbitration/README.md)。

native 中心为无操作（保留候选中的原生准入终值，类比竞争槽）；多包平手关系按[handler 依赖与组合诊断](mod-composition.md)冻结。真实场景中原生拒绝无件移动者、tireless 放行、strict 拒绝超载、组合由 after 层压回，见[场景夹具](../../compat/fixtures/sim-mod-move-admission/README.md)。

### 准入与预演：拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条准入链：丢弃全部覆盖与暂存 namespace state，保留原生预填终值（等同没进过 JS）；此前竞争槽的排序结果不受影响。这是**整房级**回退边界——不重放、不逐执行者恢复，也不复用单执行者移动槽的请求快照边界。
- 准入候选与链预演都不直接改写世界：`evaluate()` 在克隆状态上运行取消链，正式仲裁状态不受影响。输入外 ID、非布尔终值、越权能力（竞争优先级/移动请求/资源/受击/障碍追加）的故障不能被 catch + decline 掩盖；错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。执行阶段只读准入终值表，缺项（未参与仲裁或已取消）一律未获准。
- 晚期写回失败经玩家缓存重试整轮：准入结果与 namespace state 恰一次提交，不重复执行玩家代码。停用出错的 replace 后回退 native 门槛，健康钩子继续组合。
- 准入终值、请求视野与预演结果都是 tick 内瞬态，不进入检查点；检查点只保存已提交世界与 Mod state。该切片未新增槽（当时内建槽 17、revision v10；现 20，见[JS 移动预检](#请求预检)）：输入字段与能力是槽内扩展，不改变 PlanHash 与检查点身份（基线对照实验一致，见交付记录）。

### 准入与预演：验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-move-admission/README.md)提供原生、tireless、strict 和两包组合；JSON/KKPV 完整 final 一致，准入后保存退出与冷启动续跑等价。专项覆盖输入完整性与要素、原生配对时序（含配对破裂）、放行/收紧两端、非法写入/外部 ID、能力门、停用回退、晚期失败缓存重试与拒绝回退，结果见[交付记录](../research/notes/g02-move-admission-2026-10-06.md)。

G03.a 的[场景夹具](../../compat/fixtures/sim-mod-move-arbitration/README.md)提供原生、wave（链预演全停）、calm（落选者视野扣胜者）与两包组合；同样双通道一致与冷恢复等价，引擎侧另有 预演幸存集 == 实际取消链幸存集 的等价性单测（交换/环/链/拉拽/覆盖/占位拓扑）。

静态放行已由[JS 移动预检](#请求预检)贯通，`evaluate()` 同源消费静态放行，动态占格仍复核。当前不开放孵化中/spawning 门槛和完整移动执行效果；移动代价与拉拽登记分别见[移动代价](#移动代价)和[拉拽登记](#拉拽登记)。

## 障碍追加

2026-10-06 已开放 `builtin:room.moveObstacle` 房间级槽。独立 JS 包可对获胜执行者**追加**目的格障碍阻挡：输入提供全部胜者的目的格对象/地形摘要与 native 初判，JS 通过单向 `block` 能力追加；取消链按 追加阻挡 ∥ 链内 native 判定 取消。静态放行已由[移动预检](#请求预检)接通登记与仲裁，本槽初判消费该静态许可；本槽仍只追加否决。移动代价见[代价槽](#移动代价)，完整执行效果仍为后续。

### 障碍追加：为什么只允许追加

放行 native 判定的阻挡（让移动者进入未让位对象占据的格）会破坏坐标表示有效性——两个对象同格。因此能力面是单向的：`ctx.obstacle.block(id)` 只能追加，没有放行。追加取消沿让位链传播：交换/环移中一方被追加阻挡后，依赖其让位的后续请求由引擎链内递归取消（native 判定此时看到"不再让位的占据者"自然阻挡），JS 不逐格重算。链内 native 判定相对预填时点只会收紧（让位被取消）不会放宽。

### 障碍追加：输入与结算

准入槽之后、阻塞链取消之前，房间内存在移动胜者且有本槽贡献时整房一次进入 JS；无贡献或无胜者的房间不进入 realm。`ctx.args.movers` 按对象 ID 升序、不截断，含全部胜者（准入终值已定，被拒者仍在列——对其追加阻挡无额外效果）：

| 字段 | 含义 |
|---|---|
| `movers[].id/owner` | 胜者对象 ID 与归属 |
| `movers[].targetX/targetY` | 获准的目的格 |
| `movers[].terrainWall` | 目的格地形为墙（无路时 native 阻挡） |
| `movers[].hasRoad` | 目的格有路 |
| `movers[].objects[]` | 目的格对象摘要：`id/kind/owner/isPublic/structureType/yielding` |
| `movers[].objects[].kind` | `creep` / structureType / `constructionSite` / 类型名；Mod 对象为空串 |
| `movers[].objects[].structureType` | 工地在建结构类型；其余对象为 null |
| `movers[].objects[].yielding` | creep 让位预期成立：胜者且准入终值为真。准入被拒的胜者必被链内取消，不标让位 |
| `movers[].nativeBlocked` | 预填时点（全部胜者未取消）的 native 初判；链内让位取消只会收紧 |

`ctx.target.fields` 恒为 null（房间级槽没有对象目标）；本槽不开放移动请求、准入、竞争优先级、资源或对象能力。`ctx.obstacle` 是槽专属能力（不可枚举、只在处理器同步期有效）：

| 方法 | 语义 |
|---|---|
| `isBlocked(id)` | 读取当前判定：native 初判或已追加的阻挡 |
| `block(id)` | 追加阻挡；id 须在本次输入中，单向、不可撤销 |

取消链对每个胜者取 `追加阻挡(id) || check_obstacle_at_xy(链内)`；被取消者从胜者表移除并递归检查其原格上的其他请求（让位链传播）。

### 障碍追加：独立包组合

stay-north 包 replace 拦截南下（目的格 y≥20）：

```js
export default function register(api) {
  api.handlers.replace('builtin:room.moveObstacle', { id: 'stayNorth' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.targetY >= 20) ctx.obstacle.block(mover.id);
    }
  });
}
```

no-swap 包 after 禁止依赖让位链（目的格上有 yielding creep 即拦截），可与任意 replace/before 叠加——追加集在同一候选上取并集：

```js
export default function register(api) {
  api.handlers.after('builtin:room.moveObstacle', { id: 'noSwap' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.objects.some((o) => o.kind === 'creep' && o.yielding)) {
        ctx.obstacle.block(mover.id);
      }
    }
  });
}
```

native 中心为无操作（无追加阻挡）；多包平手关系按[handler 依赖与组合诊断](mod-composition.md)冻结。真实场景中原生交换成立、stay-north 拦南下、no-swap 拦交换、组合全拦，见[场景夹具](../../compat/fixtures/sim-mod-move-obstacle/README.md)。

### 障碍追加：拒绝、故障与恢复

- `return api.decline(code)` 正常拒绝整条障碍链：丢弃全部追加与暂存 namespace state，取消链只按 native 判定；此前竞争/准入槽的结果不受影响。
- 障碍候选不直接改写世界。输入外 ID、非字符串参数、越权能力（竞争优先级/准入/移动请求/资源）的故障不能被 catch + decline 掩盖；错误/预算超限使整轮候选失败，沿[Mod 执行恢复](mod-execution.md)销毁并重建 realm。
- 晚期写回失败经玩家缓存重试整轮：追加结果与 namespace state 恰一次提交。停用出错的 replace 后回退 native 障碍判定，健康钩子继续组合。
- 追加集是 tick 内瞬态，不进入检查点；检查点只保存已提交世界与 Mod state。该批内建槽增至 17（现 18，见[JS 移动预检](#请求预检)），新增冻结描述和 native 绑定改变 PlanHash；旧计划身份的检查点不会被当作同一玩法静默加载。

### 障碍追加：验收与剩余范围

[真实玩家场景](../../compat/fixtures/sim-mod-move-obstacle/README.md)提供原生、stay-north、no-swap 和两包组合；JSON/KKPV 完整 final 一致，障碍判定后保存退出与冷启动续跑等价。专项覆盖输入完整性与对象摘要（含 yielding 与 native 初判）、墙地形无路判定、追加的让位链传播、两独立策略与并集组合、非法写入/外部 ID、能力门、停用回退、晚期失败缓存重试与拒绝回退，结果见[交付记录](../research/notes/g02b-move-obstacle-2026-10-06.md)。

本槽保留单向追加；静态地形/结构放行已由[JS 移动预检](#请求预检)贯通，`nativeBlocked` 在选胜后消费该放行及动态占格初判。本槽的 block 仍可否决放行者。当前未开放：摘要中的安全模式主人（creep 阻挡的安全模式分支现只能依赖 `nativeBlocked` 初判，后续切片可附带以增强可解释性）、障碍槽自身的链预演（准入槽的 `evaluate()` 已开放，障碍追加后无第二预演点）、完整移动执行效果。整房请求视野经准入槽 `ctx.args.requests` 提供。

## 移动代价

`builtin:room.moveCost` 是取消链结算后的房间级策略槽：终局幸存者（被取消链移除的请求者不出现）按对象 ID 升序整房一次进入 JS，Mod 可对每个幸存者**替换疲劳代价**——缺项/拒绝/故障回退原生代价（重量×地形费率）。它开放移动域的执行期数值规则，与登记期（拉拽登记、预检）、仲裁期（竞争、准入、障碍）构成完整策略链。

### 移动代价：执行时点与输入

`arbitrate_room` 依序执行：拉拽登记 → 预检 → 竞争 → 准入 → 障碍 → 取消链 → **代价槽**；执行循环随后消费覆盖值。输入 `ctx.args.movers` 每条携带：

- `id/owner/originX/originY/targetX/targetY`：身份与起讫格；
- `weight`：与实收同源的裸 `move_weight`（非 MOVE/CARRY 件数+负载，无下限——纯 MOVE 件自重 0；预检/准入槽的展示值另有 max(1) 下限，跨槽口径不同）；
- `terrainSwamp/hasRoad`：目的格地形要素（原生费率 平原 2/沼泽 10/路 1 覆盖沼泽）；
- `bodyLength`：全身件数（道路磨损按此计算，仅供策略参考）；
- `pulledBy`：链首拉者 ID（疲劳原生转记对象），无配对为 null；拉者对象已移除时链首退化为自身 ID；
- `nativeFatigue`：原生代价（weight×费率），未覆盖时的执行值——策略可直接在其上做相对调整。

无幸存者的房间不进入 JS，原生整链不付费。

### 移动代价：能力面与边界

- `ctx.cost.getFatigue(id)`：当前候选值，缺省为**原生代价**（不是 0）——组合场景中后层可读到先层的改写。
- `ctx.cost.setFatigue(id, value)`：数值替换，升/降双向（可替换规则：滑行面降、泥沼面升）；值域 0..=u32::MAX 整数，id 须为本次输入中的幸存者，域外报错。
- 正常拒绝整房丢弃全部覆盖；整槽故障按既有契约重试/停用，停用后回退原生。
- **宿主固定的执行效果**（不在替换面内）：疲劳转记给链首拉者（`add_fatigue`/`head_puller`）、道路磨损（ROAD_WEAROUT×全身件数）、敌方工地踩踏、跨房入边缘疲劳归零、每 tick 恢复（-2×MOVE 件效能）。这些效果的开放须另行设计（疲劳恢复策略与重量作为竞争评分要素已另拆登记）。
- 任意传送不属于方向请求能力，不在此槽混入。

### 移动代价：组合

- 多包 replace/after 组合逐层改写：后层经 `getFatigue` 读到先层终值并可再覆盖，终值由最后一层写入决定。
- 与准入槽：准入门槛读**上一 tick 结算后的疲劳**，本 tick 代价改变下一 tick 的准入形态——策略使代价 ≤ 恢复量即可实现持续行军（swampwalker 演示），代价 > 恢复量则隔 tick 行军（bog 演示）。

### 移动代价：验收

引擎测试（输入完整性含原生要素与链首拉者、覆盖替换与缺项回退、域与值域校验）与运行时测试（沼泽原生停走 vs 策略续走的原生对照、decline 回退、replace×after 组合、能力门、故障停用回退）见 `settle/move_cost/tests.rs` 与 `mod_chain_tests/move_cost.rs`；swampwalker（沼泽压平原当量）/bog（全量翻倍）两独立包 + 原生对照 + 组合的四场景 × JSON/KKPV 双通道与检查点冷恢复见 [sim-mod-move-cost](../../compat/fixtures/sim-mod-move-cost/README.md)。证据见[交付记录](../research/notes/g04a-move-cost-2026-10-06.md)。
