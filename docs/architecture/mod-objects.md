# Mod 同房候选对象能力

`ctx.objects` 已向自有对象动作及自有对象 tick 的同步处理器开放。能力作用于本次房间候选，不要求作者维护 ID 或索引；只有整轮成功才发布。当前只支持 Mod 对象，不含原生对象创建/删除、资源、建造 trait、跨房或世界操作。

## 接口

所有对象视图为独立 JSON 快照 `{ id, room, typeKey, fields }`，没有可写宿主引用。修改返回的 fields 不会写回候选；需要调用 update，或对当前目标使用既有 `ctx.target.fields`。字段支持范围由类型 descriptor 决定。

| 调用 | 结果和检查 |
|---|---|
| `get(id)` | 本房存活 Mod 对象视图；不存在或位于其他房间返回 null；非法 ID 或本房原生对象为故障 |
| `list(typeKey = '', { after = '', limit = 128 } = {})` | `{ objects, next }`；空类型列出本房全部 Mod 对象，指定类型须在计划中；按 ObjId 数值升序，游标排除自身，末页 next 为 null |
| `create(typeKey, fields)` | 创建当前绑定 namespace 的自有类型，返回视图；填充 descriptor 默认值后校验完整字段和位置，宿主分配 ID |
| `update(id, patch)` | 同房自有类型的部分字段更新，返回更新后的视图；合并后完整 schema 校验，不提供删除必需字段或任意替换 RoomObject 的接口 |
| `remove(id)` | 删除同房自有类型；无返回值，目标缺失或在其他房间为故障 |

limit 必须是 1..256 的整数；类型、ID、游标须为原始字符串。每次列表读取当前候选，不是跨页固定视图；遍历时插入、移位或删除对象可能改变后续页的内容。ID 排序不随位置更新变化。

create/update/remove 的“自有”指**执行该处理器的绑定 namespace**，不是槽所属包。例如 other 的钩子可以读取 example 的对象，也可以创建 other 的类型，但不能通过 objects 更新或删除 example 的对象。既有 target.fields 仍按处理链共享目标字段的契约工作。Mod 共用可信 realm，这些检查表达 SDK 能力范围，不提供恶意包间安全隔离。

内建资源/移动/战斗/建造动作、link tick 等槽不提供此能力，调用会失败；它们的领域能力和局部恢复范围仍分别定义。不能借用 link 钩子创建自有对象来扩大内建恢复写面。

## 同步和引用失效

每次对象操作，包括 get/list，先校验并同步本处理器的 target.fields/state，再操作候选，最后刷新 ctx。因此尚未结束的目标字段修改也可立即查询到；保留的字段容器引用按现有上下文刷新规则更新。create 填充默认值，浮点 backing、位置字段和对象信封坐标沿与管理初始化相同的校验，位置索引同步更新；未声明 positionFields 的对象沿管理放置口径使用 (0,0)。

删除当前目标后，ctx.target.id 保留调用身份，ctx.target.fields 变为 null，get(id) 返回 null。当前处理链仍继续，后续钩子须处理 null 或通过 get 检查存活；宿主不自动终止 wrap/after。删除前保存的 detached fields 引用不能复活对象；再次 update/remove 失效 ID 为故障。新对象取得新 ID，不复用已删除对象的身份。

能力只在所属处理器同步执行期有效；保存方法供下一次处理器使用会拒绝操作，wrap 可在 next 前后使用，处理器结束后不能继续写候选。输入字段 getter/Proxy 可执行 JS；转换期间不持有 Rust 候选或 OpState 借用，转换后重新检查调用状态。被捕获的校验、转换或宿主故障仍使整链失败，不允许发布部分写入。

## 遍历资格和恢复

每房先执行动作与移动，再冻结本轮 tick 的 ID 列表。动作中新建对象本轮取得 tick 资格；tick 中新建对象下轮才取得资格；遍历到已删除 ID 时跳过。room 名及 ObjId 的稳定顺序、自有 tick 不支持 decline 的契约见[对象 tick](mod-object-tick.md)。

可正常拒绝的自有动作在整槽开始武装对象日志：字段首触、对象增删、空间索引格首触、事件长度和共享 ID 游标。`api.decline` 撤销整条链的对象写入及参与 namespace 的暂存 state，同时保留此前成功动作。恢复不仅还原坐标，还恢复同格对象的原顺序；没有整房预克隆。

单槽日志最多 256 条对象变换记录；每个对象字段只记录首次修改，插入和移除各计一条，索引格首触记录另外保存。到达上限后新增记录为故障；反复更新已记录对象不额外消耗对象条数。这个条数门槛不是 Rust 堆或进程 RSS 的硬限额，预算边界见[Mod 执行](mod-execution.md#资源预算)。

处理器异常、生命周期返回 decline、超预算或晚期玩家写回失败，均丢弃整轮，包括所有房间对象、ID、state 和玩家候选。runtime 重建玩法 VM 后复用缓存玩家结果重新结算，不重复运行玩家脚本。闭包和 JS 全局变量不在权威恢复状态中。

## 独立 JS 示例

[官方 nursery 原型](../../mods/official/nursery/README.md) 只用公共 SDK 周期创建火花，火花更新年龄并到期删除；player.js 提供类及 FIND/LOOK 查询。[场景](../../compat/fixtures/sim-mod-objects/scenario.json) 通过真实玩家 VM 检查动态对象和空间查询，sim 报告的 mod_objects 包含全部已提交 Mod 对象，便于检查无初始别名的对象。

验证涵盖出生资格、自删/同房删、分页、权限、getter 嵌套、正常拒绝、整轮与晚期故障、双房缓存重试、停用保留数据及 JSON/KKPV 一致。记录见[N1b 交付](../research/notes/n1b-mod-objects-2026-10-05.md)。本片没有生产库存、可建造建筑或内容预设；后续计划见[完整玩法](../plans/mod-complete-gameplay.md)。
