# G08.a 建筑字段与生命周期策略包

两个互不依赖的包（目录名即 Mod id，也是字段命名空间前缀），覆盖建筑
自定义字段声明面与生命周期 tick：

- `bl`：声明 `bl:gauge` 建筑——字段默认值、周期生产 + tick 共存：先产
  后 tick，tick 整写自定义键（原生键只读）。玩家类 `Gauge` 演示存储键
  之上的语义命名糖（`gauge.heat`）。
- `bk`：声明 `bk:beacon` 建筑——无生产、无仓库的纯 tick 建筑，证明
  字段域不绑定单一命名空间，也不依赖生产面。

字段默认值的落键（完工写 Value、Missing 不写）由引擎测试钉死
（`kk-engine` `building_lifecycle.rs`）；sim 场景的结构手放且不带自定义键，
从空记录起步、首写由 tick 整集补齐。场景与断言见
[../sim-mod-building-fields/](../sim-mod-building-fields/)。
