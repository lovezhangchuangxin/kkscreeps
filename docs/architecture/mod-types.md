# Mod 玩家类型生成

作者包声明的内容（对象类型、意图、资源、建筑）在玩家面有确定形态，但作者只能从架构文档拼装安装 API 的调用签名。`kkscreeps mod types` 从**冻结计划**确定性生成玩家面 TypeScript 声明（`.d.ts`），让玩家模块的 `api` 参数、类绑定、emit 调用与字段类型在 IDE 中直接可用。生成器在 `kk-mod-api/src/typegen.rs`（`player_declarations(plan)`），CLI 编排在 `kk-cli/src/mod_tools/types.rs`，走与 [mod inspect](mod-composition.md) 相同的真实注册路径（发现 → 配置校验 → 选择展开 → 冻结），不推进世界。

```bash
cargo build -p kk-cli
target/debug/kkscreeps mod types --root compat/fixtures/mod-typed --out /tmp/mod-typed.d.ts --report /tmp/mod-typed.json
target/debug/kkscreeps mod types --root <Mod根> --out <声明文件> --check --report <报告>   # 新鲜度门禁
```

## 生成内容

- **Mod 对象投影 interface**（`playerClass` 命名）：投影信封键（`id`/`room`/`x`/`y`/`pos`/`type: 'mod'`/`modType` 字面量）恰好声明一次——投影把信封键最后写入（`kk-world` 的 mod 记录裁切），同名声明字段（位置字段）被等值覆写，不重复出现。字段按可见性映射：Public 必选、Owner 可选（`?`，JSDoc 注明非归属者投影不含此键）、ServerOnly 不生成。另产出 `{PlayerClass}Constructor` 供 `api.objects.bind`：运行时把绑定类仅作原型载体（`setPrototypeOf` 从不构造），构造器类型为 `new (...args: never[]) => unknown`——不做实例约束（extends 内建类、只带部分 getter 的玩家类无法满足投影 interface）。
- **意图参数类型**（按意图键分段 Pascal 化命名）与 `emit` 窄化签名：intentKey 为字面量、args 按参数 schema 类型化；可空参数是可选参数（宿主把缺省 args 归一为 null，非 nullable 时省略会在结算侧被拒；内联形态的 `| null` 已含在类型里，Record 引用名由签名补拼）。可执行性门与引擎装配**共用同一实现**（`kk-mod-api/src/intent_gate.rs`）：域/合并模式/参数转换/授权/接收者域任一未接线形态在生成期即拒绝（压制关系自 G07.b 起可声明可执行），该门核验意图描述符支持范围；具体槽入口、生命周期与可执行绑定仍须由引擎装配核验，生成成功不代替场景运行。
- **建筑绑定**：`{PlayerClass}Constructor` 与 `api.buildings.bind` 字面量窄化。
- **`ModInstallApi`**：`builtins`（内建类表，`abstract new (...args: never[]) => object` 使 `class X extends api.builtins.RoomObject` 在 strict 下成立）、`objects`/`buildings`（按类型键字面量窄化的 bind 重载）、`globals.define`、`view.field`（动态字段逃生舱）、`intents.emit`、`queries.install`（返回 `{ find: 数字字面量; look: 类型键 }`）。空目录退化为 `{}`。注意本文件是**全计划联合面**：安装 API 在运行时按各包命名空间裁剪（某包的 player.js 只能 bind/emit 本包命名空间的键），跨包键类型通过但运行时拒绝——生成头部有同样提示。
- **`ModResourceName`**：本计划声明的命名空间资源联合（`RESOURCES_ALL` 的 Mod 部分）。
- **内建类清单**：`ModBuiltinClassName` 联合与玩家面安装运行时（`kk-runner` prelude `mods/player-api.js` 的 `BUILTIN_CLASS_NAMES`）是**双份维护**，`kk-cli` 测试逐项断言一致（顺序与内容），漂移在测试期暴露而不是玩家安装期失败。

ValueKind 全集映射：Bool→`boolean`；Number→`number`（Signed/Unsigned/Float 数值域、字节数与项数上限入 JSDoc）；String→`string`；ObjectRef→`string /* 类型键 */`（投影为对象 id）；Enum→字面量联合（集合序，与声明书写顺序无关）；Array→`ReadonlyArray<T>`（递归元素类型，可含枚举联合、可空元素或嵌套数组）；Map→`Record<string, T>`；Record→内联 interface 或具名 interface（意图参数）；nullable→`T | null`。

**TS 语法安全**：Key 允许的字段名（kebab、数字开头等）不是 TS 标识符——非标识符属性名退化为引号字符串键，枚举字面量转义（含引号/反斜杠），意图参数类型名按段净化（`alpha:sa.lute`→`ASaLuteArgs`）并对段化撞名（`a-b:c` 与 `a:b-c`）追加序号消歧；JSDoc 中的 `*/` 替换为不断开写法。全部导出符号由同一分配表管理：先保留 SDK 类型及所用 TS 泛型，再分配对象投影、对象/建筑构造器、意图参数名。名称冲突追加数字后缀，TS 保留类型名追加 Projection；例如同时声明 Foo 与 FooConstructor 时，Foo 的构造器类型为 FooConstructor2。运行时 playerClass 保持原值，投影注释标明对应类。

## 确定性、新鲜度与路径守卫

- **确定性**：生成只依赖冻结计划（输入包集、配置与选择集），无时间戳、随机数或绝对路径；同输入两次运行逐字节一致（单测与端到端测试都断言）。枚举与类型按集合序、字段按声明序输出（重排字段会改变生成字节，属预期的过期信号）。
- **新鲜度门禁**：`--check` 重新生成并与磁盘**字节**对比（编辑器改行尾同样判过期；非 UTF-8 文件按"被手改"处理），缺失、过期或被手改都退出 **2**（报告 `status=stale`），区别于注册失败的 **1**（`setup_failed`）；只读对比，不创建目录也不重写文件。不带 `--check` 运行即恢复。
- **输出与报告都必须在包根之外**：包内全部文件字节计入 `code_digest` → PlanHash，任一产物放进包目录都会改变下一次的计划身份。CLI 拒绝包内输出与报告（要求 `.d.ts` / `.json` 后缀；报告不得与声明同路径）；报告和声明均不得覆盖 `--configs`；符号链接与硬链接输入别名也被拒绝。守卫在创建任何目录**之前**执行（逐组件解析已有祖先链接，再处理 `..`，同时保护链接实体与输出目录项），被拒绝的路径不落任何文件——含失败报告，与 mod inspect 同一手法。生成头部不含 PlanHash，同一计划的声明刷新不会自我失效。
- 声明与 JSON 报告共用同目录临时文件替换：写入失败保留原文件，声明输出即使是包源码的外部硬链接，也只替换该目录项，不截断共享源码。
- `--select`/`--exclude` 与 sim/inspect 共用选择展开语义：未选包不进计划，其声明（接口、意图、资源、查询）不残留。互不依赖的两包用 `--select alpha` 与 `--exclude beta` 产物逐字节一致（端到端断言）。

## 验收

- Rust 单测（`kk-mod-api/src/typegen/tests.rs`）：ValueKind 全覆盖与信封（含 bool/ObjectRef/Float、引号键与枚举转义）、意图/资源/建筑类型化、参数命名撞名消歧、确定性与空计划退化、未接线形态与装配共用门同口径拒绝。
- 端到端（`kk-cli/tests/mod_types_tests.rs`）：两独立包组合互不串扰（含信封去重、ServerOnly 不出现、可选 args、bool/ObjectRef 映射）、选择门不残留且 select/exclude 同集同产物、新鲜度失败与恢复全周期（check 不重写手改文件）、路径守卫拒绝包内输出（含父目录不存在的投毒路径）与非 `.d.ts` 后缀、报告守卫（包外、不覆盖声明）、内建类清单与玩家面运行时逐项一致、生成物经 compat 工作区 tsc（`--noEmit --strict`）核验且消费者正例（含 extends 内建类 + bind）通过、负例（目录外意图键/错误字段类型）被 TS 编译拒绝；递归数组、类/构造器/参数/SDK/泛型重名经真实 tsc 核验，枚举元素、嵌套元素、只读约束与重命名后的参数约束用 @ts-expect-error 锁定。输入保护另见 `kk-cli/tests/mod_output_guard_tests.rs`。
- JS 夹具 [mod-typed](../../compat/fixtures/mod-typed/README.md)：alpha（全 ValueKind 字段、三种字段可见性、enum/array/map/ref、意图、资源、建筑）与 beta（owner-collection 对象、可空参数意图）互不依赖；`player.js` 是按生成声明编写的玩家模块样例。

## 当前边界

首片不做：官方 Screeps 全量 API 的 `.d.ts`（与 `@types/screeps` 的关系后续批次评估）；Room/Player/World 域意图声明（执行通道未接线，注册即拒绝）；IDE 自动配置（tsconfig paths 由作者自行指向生成文件，头部有提示）。生成面是全计划联合面，按包的独立声明面（每包一份、键按命名空间裁剪）留待后续按需评估。

所有导出符号共用分配表：对象接口、对象/建筑构造器、建筑字段接口和意图参数类型都按稳定顺序避让（数字后缀）。例如合法对象类 `FooGaugeConstructorFields` 与建筑 `FooGauge` 同时存在时，字段接口分配为 `FooGaugeConstructorFields2`，不会触发 TypeScript 的同名接口合并；消费者 `.ts` 编译回归验证实际字段形状。
