# Renderer R5 实施与验证记录

2026-09-30；工作目录 `.worktrees/renderer`，实现基线 `0c09f85`，方案提交 `4ca02d5`。
官方对照固定为本机 screeps/renderer `a2db4a7`；依赖 PixiJS 8.21.0。
本次为单代理源码复核、实现和人工验证，未执行上位方案中的三路 subagent 评审。

## 实现边界

feed 传递 events/actionLogs/says，ActionResolver 纯函数归一输入；EffectsStage 管有限效果，AmbientScheduler 管常驻与状态触发动画。
ObjectsStage 保留视图对账、前后对象索引及窄 ActorBinding；AnimationClock 是唯一生产时间入口。
PositionChannels 合成运动与咬合偏移，PropertyChannels 合成 join 基值、常驻因子和转向；跨层位移/淡出继续交给 OverlayGroup。

实现包括近战/采集咬合、远程攻疗与工作束、工作目标罩、群攻波、controller 动作、tower 瞄准/flare、link 传输、lab 双向反应及 glow、power 束/图标、say、nuke 预落地与落地效果。
常驻包括 source/mineral/deposit flicker、portal/keeper 波、tower 空闲旋转、extractor、孵化、cooldown/disabled、controller/site/invaderCore 闪烁、observer 眼部移动与 powerSpawn store 脉冲。
填充量与进度字段按白名单线性补间一个收到的 tick，之后钳制终点，不推断新的逻辑状态；离屏常驻跳过 Pixi 写入，恢复时按统一时钟取相位。

事件优先使用显式坐标，再查当前/前态对象，controller 使用唯一目标；无目标时跳过并计数。
同源同种动作按目标 id/坐标去重，明确 actionLog 覆盖事件；群攻每来源一波，受击罩按目标合并颜色。
零 amount 保留动画；hit-back/dismantle/nuke 攻击子类型只贡献受击罩；仅确认的 link→link 能量转移画传输束。

快照取消瞬态并即时同步；删除对象取消附属播放器，已发射束与落地爆炸独立完成；文本纹理精确销毁，atlas 仍由租约释放。
say 私有灰气泡仅本玩家可见，公开红气泡可见；同对象新消息替换旧消息、跟随运动、一 tick 后释放。
诊断统计为普通数据：有限实例/节点/文本、常驻激活/休眠、lighting dirty/实际渲染/成员/私有源。

## 输入能力

| 动作 | 验证输入 | 能力边界 |
|---|---|---|
| 近战、远程治疗、采集、建造、修理、升级 | actions.json 真实公开意图链 | 第一个 delta 具有 EVENT 1/4/5/6/7/9；healType=2；工地完工移除仍有 (14,17) |
| 移动、近战、治疗、死亡 | creeps.json 真实公开意图链 | 保留 R4 回归 |
| rangedAttack/rangedMassAttack | 明确标注的合成输入 | 分支引擎虽定义意图，本分支没有结算处理器；登记 G5 |
| tower/link/lab/power、say、nuke、claim/controller 额外动作 | effects/lifecycle 合成 | actionLog 为 renderer 适配，不能解释引擎 u16 位图 |

引擎既有结算和投影未改动；只扩展 `kk-cli/export_fixtures.rs` 的只读夹具场景。
PWR 16–18 图标名称在官方 processor 中存在，但本次官方 vendor 未提供对应图片；19 也无图标映射。束仍播放，缺图标计 skipped，不造替代资产。

## 验证中修复的缺陷

1. 缺结构/自然贴图时 helper 返回 Container，后续 join 按 Sprite 调用 anchor/size，完工工地链可崩溃；改为透明 Texture.EMPTY Sprite。
2. lighting 替换 EMPTY 为 RenderTexture 后，Sprite.setSize 的目标尺寸不被 v8 保存，输出精灵放大上千倍，只采到环境色；替换后重新设世界尺寸。
   旧坐标测试用 view/renderSize 公式模拟输出，漏检实际 scale；现直接断言 display.width/height/scale 并用真实 scale 还原边缘坐标。
3. 对照参考帧修正束主体 ADD、lab reactionLight 应用 glow、群攻波 ADD 与 EaseOutQuad、spawn/nuke 默认 EaseOutQuad、say 一体轮廓及测量失败回滚。
4. nested lighting 通道变动向所属部件传递 dirty；snapshot/移除/状态取消保持明确的属性所有权。
5. harness seek 消费先前 tick 时长，避免多个历史光束叠积；末态和异常停机同步暂停动画时钟。

## 自动检查

renderer 256 测（原 206 + 50）、类型检查、资产新鲜度通过；含解析/去重、官方关键点、渠道合成、移动中动作、完工目标、nuke 移除、私有 speech、离屏休眠、长帧/暂停和八次压力重放基线。
kk-cli exporter 四项测试通过，验证六份真实夹具确定性、时间序、creep 行为及 actions 事件/完工坐标。
收口检查：工作区全部 pnpm 测试绿，shared 4 测；kk-cli 全部 7 测、Clippy 和 rustfmt 绿；27 个生成文件重生哈希全部一致；Vite harness 生产构建通过，文档链接和 git diff 空白检查通过。
另补核弹 landTimeSpeed 档位改变时重启脉冲，以及短 tick 无法容纳固定 300ms power 图标延迟时的安全回收。

## 人工画面与时钟验证

WebGPU 与强制 WebGL 在相同 cell=100、tickDuration=1000ms、1280×720 下验证。
effects tick1 固定采样 0/225/300/450/1000ms：束头尾、受击色、工作延迟、tower、link、lab、power 图标、say；1000ms finite active/nodes/texts 全为 0。
预落地 nuke、常驻对象及 observer/deposit 在同画廊验证；tick3 独立爆炸采样 100/500/1000ms，移除来源对象不截断，结束无残留。
lifecycle、重播、快照取消和数据断流长帧另做边界检查；0.5/1/2/4× 倍率与暂停恢复使用 harness 按键验证。

官方参考使用已编译的 UMD bundle 与相同合成数据，通过 `scripts/prepare-official-reference.mjs` 准备隔离 HTTP 根目录，无旧栈依赖安装或代码改动。
结构类型/对象 id/store 缺省、powerId→id、says→actionLog.say 做形状适配；首态不注入动作，随后 delta 转全量 state。
ActionManager 以 5ms 小步推进并停用 app ticker，固定镜头；tick1 的 0/300/450ms 及放大的工作/反应帧对照源码和画面。
这个对照是人工语义/观感校验，不是自动逐像素 golden；官方 say 在断流期间保持到 when 失效，我方按明确 delta 的一 tick 寿命回收。

截图及文本证据保存在本次本机 artifact 目录：
`/Users/keqing/.codex/visualizations/2026/09/30/01a0f0b1-ac63-7c51-9a0a-4a4cffd71cb1/`。
文件族 `r5-official-*`、`r5-webgpu-*`、`r5-webgl-*`、`r5-nuke-*`、`r5-pressure-*`；参考 HTML/资源映射在 `r5-reference/`。

## 性能冒烟

设备：MacBook Air / Apple M4（10 CPU、10 GPU 核）/ 24GB；Codex IAB Chromium。
视口 1920×1080；pressure 50 creep、50 去重发射，同步移动/受击，4× 播放 30 ticks。
`smoke=1` 使用 performance.now 包住实际 ObjectsStage.update 与 LightingPass.update，在 playing 帧采样，排序报告 p50/p95。

| 后端 | 帧样本数 | objects CPU p50 / p95 | lighting CPU p50 / p95 |
|---|---:|---:|---:|
| WebGPU | 465 | 0.2 / 0.5ms | 0.4 / 0.5ms |
| WebGL | 465 | 0.3 / 0.6ms | 0.4 / 0.5ms |

末态 finite active/nodes/texts=0，lighting members=101（ambient + 50×2 overlays）、privateSources=1（RT）。
一轮 lighting renders=451 / dirty=23752；同一 WebGL 实例连续两轮回放后，累计 930 个 CPU 样本，finite active/nodes/texts 仍为 0，members=101、privateSources=1。
此重复资源检查使用默认 1280×720，独立于上表 1080p 性能采样；自动集成测试另外执行八次回放。
CPU 计时精度受浏览器约束，lighting 数据包含离屏绘制的 CPU 提交，不包含 GPU 完成等待或主画面渲染；这些结果不能证明完整 ≤8ms GPU 预算。
R7 的 300 对象/50 creep/100 visual、snapshot <2ms、首帧 <1s、10k tick 内存 soak 尚未执行。

## 已登记差异和后续

- G5：引擎动作输入、say、power/lab/nuke 家族与远攻/群攻处理器缺口保持开放；合成来源不可隐去。
- G7：beam 预生成纹理的端部裁切没有官方 BlurFilter 外延柔边；nuke 连续 blur 由四档 mask 混合，source/deposit flicker 和 tower 初始角改为 id 固定种子，便于可复现。
- 明确适配扩展：attackController/claimController 给出咬合/目标罩，官方 creepActions 当前没有读取这两个字段；已消失 nuke 可用前态触发爆炸；say 断流一 tick 回收。
- 属性竞争处理：tower 发射完成后恢复空闲旋转；对同一附属动作通道覆盖旧实例，沿用 R4 的重触发取消语义。
- G2/R7：地形销毁的 BindGroup warning 已用临时创建堆栈跟踪。样本均来自 Pixi `generateTextureBatchBindGroup → GpuBatchAdaptor.execute → BatcherPipe` 缓存。
  Pixi onResourceChange 在资源销毁时清空对应绑定并 warning，尚未观察自有播放器悬挂引用；不屏蔽日志、不修改依赖私有缓存。正式缓存/内存销账留 R7。

下一阶段 R6：RoomVisual、活动房间隔离、拾取/悬停、字体及资源契约、去掉公共诊断 Pixi 字段、库构建和 d.ts/API 冻结。
