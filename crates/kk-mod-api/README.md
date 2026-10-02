# kk-mod-api

本 crate 保存玩法贡献的纯数据描述，启动时校验并冻结为 `ContributionPlan`。它只依赖基础常量、serde 和 SHA-256，不包含 V8、引擎处理器或世界变更。

`freeze` 消费贡献和配置，检查定义归属、重复键、引用与类型版本、字段默认值和限额、数值及关联约束、规则覆盖冲突和处理器依赖。成功后提供只读规则数组、类型/字段目录、意图描述、预编排入口以及计划指纹。注册顺序不改变成功结果；没有附加处理器的入口使用 `Dispatch::Native`。

稳定 `Key` 用于跨版本身份，Type/Field/RuleHandle 仅用于当前计划。字段句柄携带所属类型，`get_field` 拒绝跨类型使用。作业和视图边界先检查计划指纹，再使用整数句柄；不能从其他计划复制句柄或将其写入存档。

`builtin::link()` 提供一个 link 家族的描述及 15 项默认规则。默认值读取 `kk-core`，包括缺省为零的 RCL 0 配额。Store 使用有界映射，允许空 store，不能假设 energy 键必然存在。字段缺省为 Missing 的项目由后续内建初始化绑定负责，避免复制一份与有效规则不同的缺省 energy、hits 或容量。

Record 声明成员及其中可省略的名字；optional 与 nullable 分别表示缺失与 null。link 的转移载荷要求 id 和 amount，resourceType 可省略，与现有结算行为一致；预检和 vanilla 转换仍由后续原生绑定处理。

当前边界：

- link 的有效规则与槽分派已由 kk-engine 装配消费（`LinkRules`、`ContributionDispatch`）；本 crate 仍不执行处理器、不接触世界或 V8。

- 声明 binding 标识不等于安装实现。宿主必须核对 `required_bindings()` 并连接真实实现；未知引用在冻结时拒绝，但声明后未安装的绑定须由装配层拒绝。
- 投影目录保留公开/owner/server-only/条件权限，不是已授权视图，不编码 KKPV 或执行权限判定。
- 处理器依赖只允许同一入口、同一种 kind；After 列表已逆序，执行器不能再次反转。尚不执行 wrap/next 或回滚。
- `Source` 摘要由可信装配者提供，冻结器不读取或认证源码。link 工厂摘要目前只覆盖描述工厂，玩家模块摘要为空；接入运行路径时须加入真实 native/玩家代码摘要。外部 Mod 加载器须拒绝占用 builtin 命名空间。
- 已有类型上的扩展字段、运行期参数清洗、候选扩展状态、自定义 FIND/LOOK 与 JS Mod 加载尚未接入。

运行 `cargo test -p kk-mod-api` 验证冻结契约。完整接入顺序见[领域与投影设计](../../docs/designs/domain-projection.md)。
