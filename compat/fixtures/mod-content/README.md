# JS 内容选择夹具

三个普通包共用同一包根目录，供显式选择验收：alpha 声明规则 `alpha:step`（默认 1）驱动的自有对象 `alpha:node` 与玩家面；beta 的 manifest 依赖 alpha，覆盖规则为 3，另有自有对象 `beta:beacon`；gamma 与两者无关，只在 link tick 上写自有 state。

只选 alpha 时 beta/gamma 完全不安装（无类型、玩家 API、行为与 state）；只选 beta 时 alpha 被自动加入并使用覆盖后的 step=3；alpha+gamma 组合两者共存。没有按包 ID 的 Rust 分支。

```bash
cargo build --release -p kk-cli
target/release/kkscreeps mod inspect --root compat/fixtures/mod-content --select beta --report /tmp/content-beta.json
target/release/kkscreeps mod inspect --root compat/fixtures/mod-content --select beta --exclude alpha --report /tmp/content-reject.json   # 退出 1：显式排除与 beta→alpha 必需依赖相撞
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-content/scenario-beta.json --ticks 100 --report /tmp/content-sim-beta.json
```

inspect 报告的 `composition.selection` 列出 requested/selected/auto_added/skipped；`--select beta` 时 auto_added 为 alpha。三个场景断言绑定 100 tick（每轮 alpha +step、beacon +1、玩家先于同轮 Mod tick 读取），更换 tick 数须同步换算期望值。契约见[内容选择](../../../docs/architecture/mod-content-selection.md)。
