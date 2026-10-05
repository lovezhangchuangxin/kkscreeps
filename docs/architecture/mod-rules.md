# Mod 有效规则读取

玩法 JS 可通过公共 SDK 读取**冻结后的有效规则值**：包含声明默认值、单方覆盖与经冻结选项显式选择的多方覆盖。值由宿主在注册密封后直接从 `ContributionPlan` 的 `EffectiveRules` 装入 realm，与执行链同源同批；处理器不能借注册闭包拿到声明时的默认值冒充有效值，诊断也不能从数值相等推断覆盖来源（选择记录见[组合诊断](mod-composition.md)）。

```js
export default function register(api) {
  api.rules.define('base:period', {
    type: 'i32', min: 1, max: 100, default: 4, consumers: ['base:meter.tick.process'],
  });
  api.objects.define('base:meter', {
    version: 1, playerClass: 'RulesMeter', positionFields: ['x', 'y'],
    fields: { x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 } },
    tick(ctx) {
      const period = api.rules.effective('base:period');   // 冻结有效值，可能是其他包覆盖后的值
      if (ctx.tick % period === 0) ctx.target.fields.value += 1;
    },
  });
}
```

## 接口契约

读取入口是 `api.rules.effective(key)`，挂在既有 `api.rules` 命名空间上；`api` 对象可被处理器闭包捕获，在执行期调用。

| 方面 | 行为 |
|---|---|
| 可用范围 | 仅在宿主安装冻结规则之后：即注册完成、执行链装配后的整个 realm 生存期。注册期（模块求值与 register 回调内）调用抛错，不能退化为返回声明默认值 |
| 键 | 完整稳定键字符串（如 `base:period`、`builtin:link.capacity`）；形状先经 SDK 预检 |
| 命名空间 | 不限本包：任何已安装规则都可读。覆盖本就是跨包概念，定义方的处理器必须能看到他人的覆盖 |
| 未知键 | 抛错（不在当前冻结计划内）；在处理器内沿既有异常路径成为该绑定故障，不静默返回 undefined |
| 返回值 | 原始 JS `number`，只读；不提供运行期改写、订阅或逐 tick 重算 |
| 数值转换 | 整数与浮点统一按双精度转换：整数规则超过 ±(2^53−1) 时与候选 JSON 通路一样近似为最近双精度值，不引入 BigInt（避免破坏 state/字段的 JSON 序列化）。浮点规则在冻结期已拒绝非有限值。JS 声明/覆盖面本身只能表达双精度可精确表示的整数（V8 number 即双精度），近似只可能出现在宿主侧构造的规则值上——当前 builtin 规则均为小值，该语义是为后续原生规则预留的边界 |

该读取是计划级静态数据，不是候选能力：不新增 ctx 能力、不占用整槽 token、不触及房间候选，因此没有"仅处理器同步执行期有效"的约束；模块全局缓存读取结果不构成权威状态，重试确定性仍以 `ctx.state` 为准。

## 与配置、玩家常量的区别

- **包配置 `api.config`**：宿主提供的逐包实例配置，只影响本包注册逻辑，经配置摘要参与计划指纹；其他包不能覆盖它，也没有跨包选择问题。
- **规则**：计划全局的玩法数值目录，有声明默认值、跨包覆盖与显式选择；冻结后全计划一致，本接口返回的就是这个冻结结果。规则内容与选择都参与 PlanHash。
- **玩家常量**：标准全局常量（如 `CONTAINER_CAPACITY`）在玩家沙箱构造时从有效规则同步（见[仓库容量规则](store-rules.md)），是规则在玩家面的投影。玩家 VM 与玩法 realm 分离，玩家代码不直接读规则目录；Mod 自有规则的玩家面经其声明的字段、类与查询投影。

## 计划身份、恢复与停用

有效规则与执行链在 `install_chains` 同批安装，来源是同一个冻结计划；`values` 参与规范哈希，计划身份与规则内容不可分。整轮结算失败销毁 realm 后，恢复沿既有路径从分片私有的包快照重新注册并核对 PlanHash——重装的规则值与原计划一致，模块全局（含作者缓存的读取结果）照常丢弃。故障重试的候选语义不变。

执行停用只屏蔽结算执行：不卸载内容、不重算冻结规则，读取继续返回**原冻结选择**——即使覆盖方已被停用，其覆盖值仍然生效（与组合诊断 `selected_source` 在停用后保持不变的口径一致）。停用定义方只是让其处理器不再运行，已提交的 state 与规则目录保留。

运行期规则修改、规则迁移与检查点属于后续批次；当前没有"改规则不换计划身份"的通路。mod inspect 的 `--rule-source` 只作用于当次诊断的选择输入，与宿主冻结选项是两个入口，得到相同计划须配置相同选择。

## 独立 JS 示例

[mod-rules 夹具](../../compat/fixtures/mod-rules/README.md) 只用公共 SDK：base 声明 `base:period`/`base:rate` 并在自有对象 tick 内每轮读取，tuner 覆盖两条规则与 `builtin:link.capacity`。对象增值节奏、浮点累计与 state 记录全部由覆盖后的有效值驱动；若处理器误读注册期默认值，场景断言会得到不同的最终数值。[双房场景](../../compat/fixtures/sim-mod-rules/scenario.json) 经真实玩家 VM 验证玩家可见结果，验证记录见[N1d 交付](../research/notes/n1d-mod-rules-2026-10-05.md)。

内建消费者的既有规则面（link 数值、Generic 仓库容量）继续由 Rust 按 `LinkRules`/`StoreRules` 从同一 `EffectiveRules` 装配，JS 读取与原生结算因此天然一致；新领域规则的开放仍逐片进行，见[完整玩法计划](../plans/mod-complete-gameplay.md)。
