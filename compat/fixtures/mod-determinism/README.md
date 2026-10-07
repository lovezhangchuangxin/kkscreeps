# mod-determinism 夹具

G22.b 确定性玩法时间/RNG 的共享验收包根目录。

- `dice/`：模块顶层捕获 init 流抽值与装载期 `Date.now()`（协议恒 0）；
  `dice:roller` 对象每 tick 抽两次 `Math.random()` 并记录
  `Date.now()/new Date().getTime()` 终值，全部写 `ctx.state`。
  等值断言（同种子重跑、冷恢复续跑、失败重试）即协议成立；异种子
  抽值相异即种子生效。sim 场景见 [sim-mod-determinism](../sim-mod-determinism/README.md)。

UTC Date 回归：dice 额外记录本地 getters/setters、多参数构造、无时区 ISO 解析、Date 文本与 locale 文本；`sim_tests::determinism::utc_date_is_identical_across_process_timezones_and_cold_restore` 在 UTC、上海、洛杉矶独立进程及跨时区冷恢复中比较完整 Mod state。旧宿主日期语义明确拒绝自动恢复，见[协议契约](../../../docs/architecture/gameplay-determinism.md)。
