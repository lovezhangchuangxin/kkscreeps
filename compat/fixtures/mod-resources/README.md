# N2a 资源目录夹具

只用公共 SDK 的两个独立包，验证[命名空间资源目录](../../../docs/architecture/mod-resource-catalog.md)：

- **ore**：声明 `ore:crystal` 与 `ore:dust`，并覆盖 `builtin:store.container.capacity` 为 50——新资源的库存表示、容量与搬运掉落全部沿既有规则面，没有专用 Rust 分支。
- **beacon**：manifest 依赖 `ore`；自身声明 `beacon:spark` 并在 `builtin:link.tick` 写自有 state。选择 beacon 时 ore 经依赖闭包自动加入，用于组合验收。

市场与跨房对 Mod 资源的显式拒绝由 Rust 测试覆盖
（`crates/kk-engine/tests/settle_tests/mod_resource_family.rs`），
不依赖这两个包。
