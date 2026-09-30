# F2 R5：动效系统实施方案

> 状态：已实现并验证（含已登记灰区），2026-09-30。实现基线 `f2-renderer@0c09f85`，工作目录
> `.worktrees/renderer/`。详见 [实施与验证记录](../research/notes/renderer-r5-verification-2026-09-30.md)。
> 下文保留实施时方案；真实远攻/群攻输入受分支引擎缺口限制，已明确登记。
>
> 上位方案：[F2 渲染器批](f2-renderer.md)；架构现状：[frontend.md](../architecture/frontend.md)；
> 视觉底账：[官方渲染器拆解](../research/notes/renderer-official-analysis.md)。
> 官方源码对照固定为本机 screeps/renderer `a2db4a7`，实现时仍逐 call-site 复核。

## 1. 下一阶段交付什么

下一阶段开发 **R5 动效系统**：让已能显示房间和移动 creep 的 renderer 表达
攻击、治疗、采集、建造、结构动作、说话以及对象常驻状态变化。

继续沿用 R0–R7 顺序，R5 内按下面六步实施，**仍作为一个完整切片收口**，
不把内部步骤改为新的独立里程碑。R5 后进入 R6（RoomVisual、交互与公共 API
冻结），再做 R7（性能和内存验收）。

R5 的完成口径是“规格实现 + 每类可运行夹具 + 自动验证 + 双后端人工验证”；
引擎尚无输入的效果明确标记合成演示，不能因此宣称真实引擎端到端可用。

## 2. 已核实的起点与缺口

- 已有 `AnimationClock`、`TweenManager`、`OverlayGroup`、creep 移动/转向、
  攻击/治疗受击罩、死亡淡出、快照瞬态重置及资产释放。基线 renderer 206 测。
- `ObjectsStage` 同时承担对账、动作和附属地形绘制。新增战斗/常驻效果应进入
  独立运行时，保留这里的视图对账与场景协调职责。
- `RoomDelta.says/actionLogs` 已预留，但 `FeedChange` 和 `applyDelta` 通知只
  传 events；需先接通输入。`actionLogs` 仍是 `unknown[]`，不是已完成协议。
- `RoomEvent.data` 是 unknown。真实攻击/治疗提供 `targetId` 及
  `attackType/healType`；build 还提供目标 x/y 和 incomplete。升级、预定和
  攻击 controller 事件缺 `targetId`，应读取该房间唯一 controller，不能统一
  用 targetId 解析全部事件。
- transfer/withdraw 都投影为 EVENT_TRANSFER，withdraw 的 objectId 与
  targetId 方向反转。只有确认是 link 的能量传输才映射官方 transferEnergy
  光束，普通资源转移不能一概画成 link 光束。
- 原官方效果由 actionLog 驱动，当前引擎字段仍为压缩位图，不能可靠重建全部
  动作。群攻、lab 反应、power、nuke 等不足输入以合成夹具覆盖并登记缺口；
  不能从库存/血量变化猜测动作。
- 目前结构填充量直接更新，部分闪烁/旋转/波纹冻结在静态相位；R5 接管其
  动态部分。已有字体加载只在 harness，say 字体使用也需明确加载责任。

## 3. 运行时组织

```text
RoomDelta + 本 tick 前后对象索引
              │
              ▼
ActionResolver（纯数据校验、目标解析、输入通道归一）
              │ EffectCommand[]
              ▼
EffectsStage（一次性效果实例、对象动作通道、常驻调度）
              │
        ObjectView 绑定 / 层路由 / OverlayGroup
              │
              ▼
AnimationClock → 属性采样 → Pixi 写入 → lighting dirty
```

建议新增模块（名称随实施调整，职责保持）：

| 模块 | 职责与边界 |
|---|---|
| `anim/effect-specs.ts` | 色表、时序、形状和层路由常量；与官方源码一一对应 |
| `anim/action-resolver.ts` | 纯函数，将 events、says、明确形状的 actionLog 归一为命令，无 Pixi 依赖 |
| `anim/effects-stage.ts` | 效果建立、更新、结束、快照重置、房间销毁；持有一次性实例 |
| `anim/effects/*` | 光束、咬合、目标罩、群攻波、说话等有限效果播放器 |
| `anim/ambient-scheduler.ts` | 按对象/部件注册常驻动画；状态与可见性控制激活/休眠 |
| `anim/property-channels.ts` | 合成基础位置/动作偏移、基础 alpha/闪烁/淡出，避免多个 tween 互相覆盖 |
| `assets` 构建脚本与清单 | 构建期生成可拉伸束纹理，纳入确定性产物和新鲜度校验 |

运行时只有既有 renderer ticker 一处时间入口。播放器不得另建 ticker、
setInterval 或 Date.now 时钟。公开 API 继续只暴露普通数据。

归一命令保留动作语义和输入来源，例如
`{ action: 'rangedAttack', sourceId, targetId?, target: {x, y}, origin: 'event' }`，
以及 `{ action: 'say', sourceId, message, private, origin: 'say' }`。
颜色/时长/宽度由规格表决定；解析层不携带 Sprite、Texture 或回调。

对象视图向动画运行时提供内部窄接口：获取对象锚点、获取声明的动作部件、
注册/移除跨层节点、写入动作通道。避免让播放器直接访问 ObjectsStage 私有
Map，也避免把整棵 Pixi 树作为公共契约。

## 4. 六步实施顺序

| 顺序 | 实现内容 | 当步可检查结果 |
|---|---|---|
| 1. 输入与规格 | 建立事件映射/输入能力表；传递 says；定义 actionLog 适配的最小明确形状；迁移 EVENT 常量为单一 TS 来源并校验 Rust 数值；保存解析需要的变更前状态 | 真实事件可解析；缺字段/未知事件处理明确；合成与真实输入可区分 |
| 2. 运行时与光束 | 建 EffectsStage、对象动作通道与重置/销毁路径；生成普通/模糊边缘束纹理；实现 beam、目标罩与结束回收 | 远程攻击、远程治疗及建造夹具可播放；不逐帧重画 beam、不逐效果创建滤镜 |
| 3. 战斗与工作动作 | 近战/采集咬合、群攻波、build/upgrade/repair、controller 动作、tower 转向/flare、link/lab/power；扩展受击罩类型门 | 每种已定义动作有夹具和时序锚；结构与 creep 分支正确 |
| 4. 常驻与状态补间 | source/glow 呼吸、portal/keeperLair 波、tower 空闲旋转、extractor 旋转、孵化、cooldown/disabled/controller 闪烁、填充量补间 | 状态取消即停；动作与基础状态不抢写；静止/不可见件不持续产生无用更新 |
| 5. say 与 nuke | say 气泡生命周期/覆盖/跟随；nuke 描述符、预落地脉冲与落地效果；补齐合成输入家族 | 重放/死亡/快照清理正确；nuke 爆炸可独立于已消失对象完成 |
| 6. 验证与收口 | 官方关键帧对照、双后端、变速/长帧/快照/销毁、性能冒烟、文档与灰区登记、评审和修复 | R5 范围逐项完成或明确登记；不把 R7 验收提前标成完成 |

首个实施目标为 **步骤 1–2 的纵向闭环**：从一条真实形状的远程攻击事件进入
feed，到蓝色光束与目标罩播放，再到完成回收、快照取消、renderer 销毁。
这一闭环通过后，再按表补效果家族。

## 5. 输入语义必须先定下来

### 5.1 目标与坐标

先用事件显式坐标（例如已完工 build 的 x/y），再用目标当前对象，必要时读取
变更前索引；controller 事件使用房间唯一 controller。缺失或歧义时跳过该
效果并提供开发诊断，不能落到 (0,0)。对象的 type/structureType 使用现有
resolveTypeKey 归一，tower 等不能仅检查 `state.type === 'tower'`。

解析层输出逻辑格坐标及对象 id，运行时统一换算世界坐标。光束起点按触发时
对象实际渲染位置采样，目标使用命令捕获的位置，具体跟随语义逐播放器按
官方源码复核；目标之后被移除，不得让束端点丢失或跳到默认值。

### 5.2 双通道与重复事件

事件通道先支持真实投影能证明的动作；actionLog 通道必须有明确的每对象动作
结构及运行时守卫，旧 u16 位图不作为官方 actionLog 解释。

常量统一只涉及 R5 使用的 EVENT_* 和攻击/治疗子类型，从既有 Rust 常量源
生成或校验 TS 定义；完整 443 个游戏常量的生成管线另行推进。

同一逻辑动作同时由 events/actionLog 给出时只播一次：按房间/tick/来源/
动作种类及目标语义匹配，明确 actionLog 对同一动作的优先级。不同动作不能
只因 source/target 相同被合并。群攻一个来源只播一个波，受击罩按目标合并
本 tick 攻击/治疗颜色。零 amount 事件若官方仍产生动作，不擅自过滤。

claim 没有可靠事件则保留待 actionLog 输入；hit-back/dismantle/nuke attackType
必须各自确认效果，不能全都画成近战咬合。

### 5.3 重置、移除与时钟

快照取消一次性动作、say 和旧常驻绑定状态后按新状态重新激活；正常 delta
删除对象时取消其绑定动画，而已发射光束/核爆等由房间级运行时管理的有限
效果依规格完成。renderer destroy 释放全部实例、私有文本纹理及资产租约。

回放倍率改变只改变动画经过时间，保留当前进度；0 暂停动作/常驻动画。
逻辑 tick 仍决定 spawning/landTime/cooldown 等状态终点。状态进度插值需要
明确断流后的钳制规则，不能自行推断引擎发生了新 tick。

## 6. 属性与性能纪律

- 位移基础值、咬合偏移、转向、部件旋转使用独立通道合成，最终变换集中写入。
  join 基础 alpha、常驻脉冲 alpha、safemode 与死亡淡出以明确乘法关系组合。
  不让每 tick apply 把动画采样值重置，也不让 fade 和 pulse 争写同一属性。
- beam 使用预生成束纹理，通过长度/角度/头尾窗口变换实现官方时序；颜色、
  宽度、模糊边缘、ADD 叠加和端点裁剪仍需双后端视觉对齐。几何一次建立，
  逐帧只改必要属性。程序波形优先复用几何；复杂进度弧只在可见值变化时更新。
- 常驻动画只对注册且激活的部件采样；视口外暂停 Pixi 写入，恢复时按统一时钟
  计算相位，避免从头播放。剔除范围包含 glow/effect 外扩，不只看对象中心。
  zoom/LOD 保留接口，细节档位沿上位方案留到 M4。
- 对象局部动画不得每帧 getBounds 或重新创建 Text。say 只在文字变化时测量
  与更新纹理；字体的实际加载成功与回退需可验证。
- 收集 active effects、常驻激活/休眠数、节点/私有纹理数与 lighting dirty
  计数。由此调查此前 BindGroup warning：确认持有它的 shader/节点释放顺序，
  若发现悬挂引用就在 R5 修复；不能仅屏蔽警告。

## 7. 夹具与验收

补齐三类夹具，README 注明产出方式、输入来源及对应动作：

1. **真实动作链**：在既有只读 exporter 文件中增加确定性战斗/工作场景，
   使用公开 TickInput/intent API；不修改引擎结算/投影既有实现。覆盖投影确实
   支持的攻击、治疗、采集、建造、升级、修理及事件边界。
2. **合成效果画廊**：tower/link/lab/power、群攻、say、nuke 等不足真实输入
   的家族，普通数据注入，不伪装成 exporter 产物。
3. **生命周期压力场景**：同帧多效果、移动中动作、移除目标、完工工地、
   safemode 中途变化、重复快照、切房/重播、缺失字段、变速与长帧。

自动验证覆盖：解析与通道优先级；官方时序关键点；位置/透明度合成；效果
结束与取消；没有活动实例时零动画写入；重复 create/destroy 后节点、租约、
私有源和监听回到基线。新效果不得回归既有 206 测。

人工验证覆盖 WebGPU/WebGL；每类效果至少记录开始/峰值或中程/结束关键帧，
固定 cellSize、视角、tickDuration 与播放器时间，避免只凭运行中随机截图。
覆盖 0.5×/1×/2×/4×、暂停恢复与一次跨越结束时间的长帧。

R5 做 50 creep 并发效果和反复回放的性能冒烟，记录硬件/后端/场景/采样方法，
不以 HUD fps 代替 CPU/GPU 耗时。R7 再执行冻结预算：1080p 的
300 对象/50 creep/100 visual、渲染 ≤8ms、snapshot 对账 <2ms、首帧 <1s、
10k tick 内存稳态。R5 尚无 100 visual，不能宣称满配预算已经验收。

R5 收口评审按上位方案检查官方规格、契约/生命周期和性能三方面；本次方案
制定不执行实现或并行评审。清偿后回写上位方案的实施记录和灰区表。

## 8. 后续路线与边界

**R6**：RoomVisual 五原语；visual 通知；单个 RoomScene 的活动房间选择与
非活动房间更新隔离；拾取/悬停回调；字体/资源加载契约；去除公共句柄诊断
Pixi 字段；d.ts 导出与库构建；评审冻结 tickDuration、playbackRate 等临时 API。

**R7**：完整性能场景、内存 soak、后端对比、资源绑定警告销账、性能热点
驱动的池化/缓存优化，以及最终文档/灰区收口。世界地图按原计划留 M4。

工作仍在 `.worktrees/renderer`，主树引擎工作不受本计划修改。缺数据时登记
引擎侧需求，以合成夹具验证 renderer 实现，不扩大为引擎协议重构。
