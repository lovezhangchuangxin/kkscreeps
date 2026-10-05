# Mod 新世界内容选择

新世界创建时可以显式选择安装哪些包：选择按 manifest 依赖闭包展开，未选包完全不安装，实际内容身份进入组合报告与 PlanHash。本片只覆盖**新建世界口径的启动期选择**；运行期改选、停用即卸载、存量世界的卸载/迁移/升级仍属后续（四种操作的区别见[内容包设计](../designs/content-packs.md#四种不同操作)）。

## 选择输入与共用语义

三处入口共用同一选择语义，均解析为 `ContentSelection { include, exclude }`：

| 入口 | 形式 |
|---|---|
| 宿主 Rust API | `RegisterOptions.selection`（默认空 = 不做选择） |
| sim 场景 | `mods.selection`（`{"include": [...], "exclude": [...]}`，可省略） |
| mod inspect | `--select ID` 与 `--exclude ID`（均可重复） |

`include`/`exclude` 均为空表示不做显式选择，安装包根目录发现的全部包（既有行为不变）。`include` 非空时从显式选择集出发展开；`include` 为空而 `exclude` 非空时安装"发现的包减去排除项"。两者不得重叠，也不得包含 `builtin`——builtin 是固定内建贡献，当前不可经内容选择安装或关闭（迁移随 N2/N3 逐族落实，见[内容包设计](../designs/content-packs.md)）。

## 展开与拒绝

展开只沿 **manifest `dependencies`**（BFS，集合与迭代有序，结果与目录装载序无关）：

- 依赖缺失：`缺失依赖：a → b（b 不在包根目录）`，含依赖路径。
- 显式排除与必需依赖相撞：`显式排除 b 与必需依赖相撞：a → b；…不会静默重新加入`，整个配置被拒绝，不重新加入被排除内容。路径从任一种子出发即可构成证据；`include` 为空时种子是全部留存包，报出的可能是较短路径。
- 未知包 ID（选择或排除了未发现的包）与 `include`/`exclude` 重叠都在装载前报错。
- 依赖环不由展开诊断，仍由依赖拓扑排序在注册期拒绝（与未选择时一致）。

版本范围校验只作用于展开后的集合：未选包的坏依赖、坏版本不再阻塞装载。

**manifest 依赖与 handler `dependsOn` 是两条边界**：前者参与选择展开（缺依赖在展开期拒绝）；后者是具名处理链关系，不参与展开、不会自动把未选包拉进来——引用未选包的处理器沿用既有"未知处理器依赖"冻结错误，不静默跳过（区别详见[组合诊断](mod-composition.md)）。需要固定包版本或注册期协作时应声明 manifest 依赖。

## 未选内容的实际范围

选择在模块装载之前解析。未选包不进入模块加载器（跨包导入本就被拒绝）、不执行注册、不产生类型/字段/规则/资源/处理器/玩家面贡献：

- `ContributionPlan` 与 PlanHash 只含已选包内容；**不同的安装集合得到不同的 PlanHash**（含规则与[资源目录](mod-resource-catalog.md)）。
- 玩家沙箱不安装其玩家模块，`typeof` 该包全局/常量为 `undefined`；其声明的资源不在玩家 `RESOURCES_ALL` 中。
- `place_mod_object` 等按已装配计划工作的入口找不到其类型。
- 逐包配置（`configs`）与停用集合指向未选包均在注册前报错——未选包没有可配置或可停用的执行面。宿主侧的 `register_with_disabled` 在此报「停用的 Mod … 未被内容选择安装」；`mod inspect --disable` 是注册后的预览，其未知 ID（含未选包）在预览处以「未知 Mod …」报错，拒绝结果一致、文案入口不同。

选择输入本身不参与 PlanHash：解析后**安装集合相同**的不同输入（如 `include=[a,b]` 与依赖 `a→b` 的 `include=[b]`）得到同一计划。比较两个世界的实际内容须同时看 PlanHash 与报告中的选择记录。

## 报告与恢复

组合报告新增 `selection` 字段（`requested`/`excluded`/`selected`/`auto_added`/`skipped`），`report_version` 提升为 2；`auto_added` 只在显式 include 非空时非空，列出因依赖闭包自动加入的包。sim 报告的 `mods.selection` 记录场景输入，inspect 报告顶层 `report_version` 同步为 2。

恢复沿用既有分片私有启动快照：runtime 保存完整发现包表与含选择的注册选项，整轮结算失败销毁 realm 后按同一选择重新解析、注册并核对 PlanHash 与绑定清单——选择漂移会改变实际安装内容，被身份核验拒绝，不会发布替代 VM。停用（`--disable`）与选择是正交输入：停用作用于已安装包的执行屏蔽，不卸载内容。

## 作者命令

```bash
cargo build --release -p kk-cli
target/release/kkscreeps mod inspect --root compat/fixtures/mod-content --select beta --report /tmp/content.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-content/scenario-beta.json --ticks 100 --report /tmp/sim.json
```

夹具与场景见[内容选择夹具](../../compat/fixtures/mod-content/README.md)，验证记录见[N1e 交付](../research/notes/n1e-mod-selection-2026-10-05.md)。内容预设（按名称选择一组包与配置）语法尚未冻结，登记为后续批次；当前入口都是逐包 ID 的显式列表。

完整状态的[基础检查点](checkpoints.md)另外核验全部发现包与选择/配置身份；缺包或选择漂移拒绝可写冷恢复。旧版分片内失败重试仍使用内存私有启动快照。
