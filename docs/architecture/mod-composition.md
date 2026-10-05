# Mod 处理器依赖与组合诊断

公共 JS SDK 已开放 `dependsOn`，接入原有冻结图；同一注册输入的图排序、实际链安装、停用回退和诊断共用执行选择逻辑。报告展示冻结结果与当前执行掩码，不是逐 tick trace、冷恢复清单或任意组合可运行的证明。

## 处理器声明

```js
export default function register(api) {
  const first = api.handlers.before('builtin:link.tick', { id: 'first' }, ctx => {});
  api.handlers.before('builtin:link.tick', {
    id: 'second',
    dependsOn: [first, 'other:handler.ready'],
  }, ctx => {});
}
```

before/wrap/replace/after 均返回处理器稳定键字符串 `<namespace>:handler.<id>`。选项只接受 id 和 dependsOn；省略 dependsOn 等同空依赖，提供时必须为稳定键字符串数组，null/字符串/非法成员报错。数组重复项按集合归一；不接受拼错的 depends_on、priority 等未实现选项。选项在声明 op 转换期规范化读取，包含原型上的 dependsOn；getter/Proxy 转换错误即使被 JS 捕获仍使注册失败，允许合法嵌套注册，Rust 不持有跨 JS 借用。

dependsOn 引用**处理器键**，不是 slot、Mod ID 或 process 绑定。可以引用稍后注册的处理器及其他包；冻结时须存在，且位于同一 slot、同一种 kind。未知键、循环、跨槽/跨 kind 依赖及多个 replace 都在安装执行链前拒绝，错误包含相关槽/处理器绑定。依赖环诊断列出未就绪集合，可能包含环的下游，不宣称为最小环。

每种 kind 分别拓扑排序，就绪集合按稳定键字典序选择。before 按拓扑序；wrap 按拓扑序从外到内，返回时退栈；after 按拓扑序的**逆序**执行。因此 after 的 dependsOn 表示声明图中的前置关系，实际回调顺序反向；不能把它解释为 after 的“在另一个 after 完成后运行”。多个 replace 即使指定顺序仍冲突，没有最后注册优先。

manifest dependencies 管包存在、版本和注册/加载顺序；handler dependsOn 管具名处理链关系，两者不同。只引用处理器符号允许包无 manifest 依赖，但被引用包必须已安装；需要固定包版本或注册期使用其他包内容时仍应声明 manifest 依赖。处理器依赖不自动下载包、不暗中加入未选择内容，也不提供跨包 JS 导入能力。

## 组合数据

`Registered.composition` 为注册成功后的类型化结果；`EmbedEngine::mod_composition()` 返回包含当前停用掩码的独立诊断副本。未创建 JS Mod VM 的原生默认引擎返回 None，sim 报告中为 null；显式注册空根目录仍能检查 builtin 组合。修改返回数据不会修改引擎。

| 字段 | 含义 |
|---|---|
| report_version / plan_hash / sources | 报告版本 1、真实冻结计划 SHA-256 和来源版本/服务端/配置/玩家面摘要 |
| handlers | 按 key 排序的处理器 key、slot、kind、binding 和 depends_on 集合；保留停用处理器的声明 |
| slots | 槽 key/stage、original_center、frozen dispatch、active 链及 execution 模式 |
| rules | key、default、实际冻结 value、definition_source、selected_source、所有 overrides 和 consumers |
| dependencies | manifest 依赖与跨包 handler 依赖合并后的 Mod 级停用关系；不代表注册拓扑序 |
| disable_closures | 每个包停用时的完整传递影响集合，包含自身 |
| disabled_mods | 本报告应用的执行停用集合，按包 ID 排序 |

active 链的 before/wraps/after 是实际运行顺序，center 是当前选中的唯一绑定，native_center 与 decline_allowed 沿宿主执行约定。槽所属包停用时 active=null、execution=inactive；停用 replace 后 center 回退 original_center。停用后规则、schema、玩家 API 和 state 保留，selected_source 即使已停用也保持原冻结来源，不能把执行停用解释为移除覆盖。

execution=native 表示原冻结 Native 内建槽的 Rust 直调；hosted 表示整槽进入玩法宿主。原本有钩子的 Hosted 内建槽在全部钩子停用后仍按原引擎执行表进入 VM，只有中心回退 native，报告仍为 hosted。这里区分整体分派与中心，避免误报零 JS 成本。

规则 value 从冻结计划按句柄读取，来源按冻结选项及候选集合记录；不靠数值相等猜来源，多个相同值覆盖仍须显式选择。规则诊断不等于 JS 处理器已能读取有效规则，后者属于 N1d。数据集合按稳定键排序，计划身份不包含停用掩码；比较执行行为时还须核对 disabled_mods。

## 停用闭包

显式 handler 跨包依赖提升为 Mod 级依赖：前置处理器所在包停用时，整个依赖包及其依赖者保守停用。manifest 和 handler 边共用闭包计算，runtime 的手动/自动停用及诊断预览使用同一算法。

同包 handler 依赖不形成跨包边。不同 kind 的处理器可以让 Mod 级关系有环，但各 kind 的冻结图仍必须无环；Mod 级环作为一个相互依赖组整体停用。没有显式声明的槽/类型引用不自动提升为 manifest 依赖；槽所属包停用仍跳过整个槽，包括其他健康包钩子。

直接使用 `ModRealm::register_with_disabled` 的可信宿主须先计算完整闭包。宿主现在检查掩码闭包完整性，缺少依赖者拒绝注册并密封失败 realm；不会保留缺前置条件的链。停用策略、缓存重试与恢复仍见[Mod 执行](mod-execution.md#显式停用与回退)。

## 作者命令

在仓库根目录构建后执行：

```bash
cargo build --release -p kk-cli
target/release/kkscreeps mod inspect --root compat/fixtures/mod-composition --report /tmp/mod-composition.json
target/release/kkscreeps mod inspect --root compat/fixtures/mod-composition --disable z-gate --report /tmp/mod-disabled.json
```

root 相对当前工作目录解析，加载其直接子包。`--configs FILE` 读取按包 ID 分组的 JSON 对象，拒绝未知包配置；包仍负责业务配置校验。可重复 `--rule-source key=namespace` 显式解决多方覆盖，重复选择同一规则、不存在的覆盖来源或未知规则报错。该选项只作用于本次 inspect；sim 尚没有相同规则选择输入，需另行配置匹配的宿主冻结选项才能得到相同计划。

可重复 `--disable ID` 仅预览停用闭包及活动链；命令没有世界，不执行 tick，也不改变运行服务器。仍注册完整包及冻结原计划，坏注册、多 replace/规则冲突等不能靠 disable 绕过。公开命令直接使用同一包/SDK/冻结路径，报告记录包目录、版本、依赖及 CLI SHA-256。

report 必须为 .json，放在包根目录之外，不能覆盖 configs。成功退出 0，注册/输入/报告 I/O 失败退出 1；参数解析错误沿 Clap 的 2。已验证输出路径后发生的输入/注册失败会用 setup_failed/error/null composition 替换旧报告，避免残留成功；非法输出路径不写文件。JSON 通过同目录临时文件替换，不是持久世界事务。

sim run 的顶层 composition 保存同结构诊断及当前 disabled_mods；有真实世界时，作者可以同时检查最终对象字段与报告链序。示例见[组合夹具](../../compat/fixtures/mod-composition/README.md)和[双房场景](../../compat/fixtures/sim-mod-composition/scenario.json)，验证记录见[N1c 交付](../research/notes/n1c-mod-composition-2026-10-05.md)。inspect 不验证所有引擎域已经开放，也不提供内容选择、更新、迁移或逐处理器计费。
