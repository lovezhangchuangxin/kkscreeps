# 回放夹具

形状为 `{room, ticks:[RoomSnapshot, ...RoomDelta[]]}`，遵循 shared feed 契约。
真实导出与合成输入分开标明；actionLogs 是 renderer 的明确动作适配结构，不能填入引擎 u16 位图。

| 按键 | 夹具 | 来源与用途 |
|---|---|---|
| 1–4 | plains / standard / swamp-walls / dense-edge | kk-cli，地形、结构及边界；31 个 tick |
| 5 | creeps | kk-cli，运动、近战、治疗、受击罩和死亡；31 个 tick |
| 6 | showcase | 合成，完整对象形态、敌我色、顶罩；3 个 tick |
| 7 | actions | kk-cli，近战、远程治疗、采集、建造、修理、升级；31 个 tick |
| 8 | effects | 合成，战斗/工作/tower/link/lab/power/controller/say/nuke 与常驻动画；5 个 tick |
| 9 | lifecycle | 合成，移动中动作、重复通道、完工目标移除、缺失目标、safemode、id 复用；5 个 tick |
| 0 | pressure | 合成，50 creep 同时移动和发射、双通道去重；31 个 tick |

真实导出使用固定 seed 和固定 TickInput 墙钟，经公开 intent API 结算，再用 differ 转为增量。
`actions` 第一个增量含 EVENT 1/4/5/6/7/9；远程治疗 healType=2；工地完工移除时 build 仍给出 (14,17)。
本分支引擎没有 rangedAttack/rangedMassAttack 结算处理器，不能把合成远攻/群攻称为真实端到端。

在 renderer 包目录再生：

```sh
rtk proxy node scripts/build-fixtures.mjs
rtk proxy node scripts/build-showcase-fixture.mjs
rtk proxy node scripts/build-effect-fixtures.mjs
```

首个脚本自行构建 kk-cli；后三份效果文件带 `provenance`。入库形状、事件覆盖和压力规模由 fixtures.test 验证。

harness：空格播放/暂停，`.` 推进 tick，暂停时右箭头推进动画 50ms，`+/-` 切 0.5/1/2/4 倍速，F 复位视角。
固定关键帧用 `?fixture=effects&tick=1&sample=450&view=2475,2475,0.13`；`backend=webgl` 强制回退。
seek 会先消费已结束 tick 的动画时长；选中 tick 的有限效果从 0ms 起采样。
`smoke=1` 在 HUD 记录 ObjectsStage 更新与 lighting 提交的 CPU p50/p95；这些值不包含主画面绘制和 GPU 执行时间。

官方参考（可选、本机已有官方 checkout）：

```sh
rtk proxy node scripts/prepare-official-reference.mjs /path/to/screeps/renderer
rtk proxy python3 -m http.server 5176 --bind 127.0.0.1 --directory <打印的临时目录>
```

随后访问参考页 `?fixture=effects&tick=1&sample=450&view=2475,2475,0.13`。
它使用官方 UMD 产物、适配完整 state 和显式 5ms ActionManager 步进；用于人工对照，不属于库代码。
