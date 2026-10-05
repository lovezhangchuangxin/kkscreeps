# 有效规则读取夹具

base 声明 `base:period`（i32，默认 4）与 `base:rate`（f64，默认 0.5），自有对象 `base:meter` 的 tick 每轮经 `api.rules.effective` 读取节奏与增益：偶数节拍给 `value` +1，每轮给 `pool` 累加增益，并把执行期实际读到的 `period`/`rate`/`builtin:link.capacity` 记入 namespace state。tuner 覆盖这三条规则为 2、0.25 与 700。全部行为走公共 SDK 装载与冻结路径，没有按包 ID 的 Rust 分支。

若 base 的处理器误用注册闭包里缓存的默认值，最终数值会退化为默认节奏（100 轮 value=25、pool=50）；实际冻结值应得到 value=50、pool=25。内建 `builtin:link.capacity` 的覆盖同时验证跨命名空间读取。双房场景与断言见 [sim-mod-rules](../sim-mod-rules/scenario.json)，契约见[有效规则读取](../../../docs/architecture/mod-rules.md)。

场景断言在 100 与 2000 轮下都通过：数值类 gte/delta_gte 只在 100 轮附近有区分力，长跑时对"读错规则"的检出依赖 state 的 period/rate/capacity 三条精确相等断言。

在仓库根目录执行：

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-rules/scenario.json --ticks 2000 --report /tmp/rules-sim.json
target/release/kkscreeps mod inspect --root compat/fixtures/mod-rules --report /tmp/rules-inspect.json
```

这是功能夹具，不是内容预设或性能比较；故障重试、重建与停用后的读取由 kk-runtime 集成测试验证。
