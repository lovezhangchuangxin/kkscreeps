# Renderer R5 实施与验证记录

2026-09-30；工作目录 `.worktrees/renderer`，实现基线 `0c09f85`，方案提交 `4ca02d5`。
官方对照固定为本机 screeps/renderer `a2db4a7`；依赖 PixiJS 8.21.0。
本次为单代理源码复核、实现和人工验证；上位方案要求的三路 subagent 收口评审已由主会话于同日补齐并清偿，见文末「三路评审与清偿」。

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
PWR 16–18 图标名称在官方 processor 中存在；对应 SVG（`operate-power`/`fortify`/`operate-controller`）已自 R0 起从官方 `metadata/images/` 拷贝入库并进 atlas，实测正常渲染、不计 skipped。仅 PWR 19+ 无常量与图标映射：束仍播放，缺图标计 skipped，不造替代资产。（三路评审勘误：原稿「官方 vendor 未提供对应图片」与实测矛盾——三件 SVG 自 R0 在库，探针实测 16–18 渲染且 skipped=0。）

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

- 评审补充登记（2026-09-30）：invaderCore TTL 环为静态（官方 `invaderCore.metadata.js` siteProgress 无 actions，仅 `constructionSite.metadata.js` 有 blinking——评审后已修正实现）；`_all.metadata` hasEffect 的 flare3 红色循环顶罩（官方对带活跃 power 效果的目标挂载，含 2π/1s 自转）未实现，留待后续切片；spawn 孵化脉冲按 spawning 状态启停（官方 once+shouldRun 首启后不停，属官方 quirk 的有意偏离）；填充量补间白名单宽于官方（官方仅 link 能量与 spawn/powerSpawn resourceCircle 走补间，我方含 creep 资源圆/tower 能量条/source 能量矩形，观感更平滑）；say 气泡无像素对齐（官方逐 tick alignPositionToPixels，亚像素抖动属观感级）；目标侧工作罩 owner=来源对象（来源中途删除即取消罩，§5.3「取消其绑定动画」的解读，R6 契约文档化）。

## 三路评审与清偿（2026-09-30，主会话补门禁）

上位方案 §7 要求的收口评审由主会话三路 subagent 补齐（官方规格保真 / 契约与生命周期 / 测试夹具与验证报告实测复核）。本记录经三路逐条复核：自动检查全部复跑通过（renderer 256 测、shared 4、kk-cli 7、clippy/fmt、Vite 构建、文档链接、27 个生成文件重生哈希逐字节一致、篡改 SVG 负向测试新鲜度即红）；artifact 目录 46 件与 HUD 采样点逐一核对相符，压力数字与文本证据逐字一致（毫秒值无头不可复跑，标注为机制属实）。

**阻塞 2 项已清偿**：

1. tower 治疗事件（EVENT_HEAL+healType=2 归一为 rangedHeal）只画束，缺瞄准旋转/flare1/光照脉冲——effects-stage tower() 触发条件补 `'rangedHeal'`（官方 tower.metadata shotAnim=attack‖heal‖repair），新增测试锁定。
2. 本记录「输入能力」PWR 16–18 表述与实测矛盾（已在原节勘误；`f2-renderer.md` G5 行同步订正）。代码实测行为优于原稿声明，无需改动。

**建议处置**：已改 8 项——invaderCore TTL 环去闪烁（ambient 测试锁定）、actionLog 通道补 previous 索引回退（与事件通道对齐，测试锁定）、私有 say 隐私过滤改显式判空（前态来源不再依赖非空断言）、bite 缺失 actor 计 skipped（moving/同格仍为官方抑制不计）、诊断统计 `privateTexts`→`texts`（公开气泡也计入，R6 冻结诊断面前正名）、beginMove rotateTween 补 onEnd 清空、exporter 确定性测试对齐入库 30 tick 全长、harness HUD 增 canvas 分辨率行。登记不实现 5 项（见上节「评审补充登记」）。R4 语义回归核查：快照重建/id↔typeKey 漂移防御/fade 底 α 乘法/'visible' boolean/夹具 id 空间全部保留或合理演变（motion 门控收窄为 HIT_COVER_TYPES 六类=官方 creepActions 声明域，评审对照确认）。

**复审与返工（主会话逐行复审清偿增量）**：发现清偿自身引入一处隐私回归——私有 say 过滤改写时把「无本玩家身份（player=null，`localPlayerId` 缺省即如此）」分支错为放行，任何 owner 的私语都会渲染（清偿前逻辑与官方 `isPublic||isOwner` 语义一致为全滤）。已修正（player=null 一律滤私有）并加测试锁定：player=null 场景断言私有全滤、公开照播；测试以 `CanvasTextMetrics.measureText` 桩适配无 canvas 环境。

**层序修复（用户实测发现，2026-09-30）**：名字 overlay 盖住 say 气泡，与官方相反。官方依据：`creep.metadata.js` 处理器序 `creepActions`→`displayName`→`say` 且名字/气泡同在 effects 层、同 zIndex 稳定排序保插入序——气泡创建晚于名字，官方是气泡盖名字（名字 once 创建、say 说话时才插入）。我方根因：finite-effects 根容器在 ObjectsStage 构造期先挂入 effectsRoot，名字部件（`creep.name` 等，PART_LAYERS→effects）在首次对账时追加，插入序 rendered 名字在上。修复：每次 apply 末尾把 finite-effects 根重挂到 effectsRoot 顶（对既有子节点 addChild=移末尾，零排序，符合固定层栈纪律）；结果序=名字/常驻 overlay < 受击罩 < 光束/say/波/核爆（受击罩在名字上亦符合官方首次动作晚于创建的常见表现）。层序测试锁定并以 stash 负向验证（无修复即红）；WebGPU harness effects tick1@450ms 截图确认气泡盖住 melee 名字（ranged 名字正常显示对照）。清偿+层序修复终态 renderer 261 测。

下一阶段 R6：RoomVisual、活动房间隔离、拾取/悬停、字体及资源契约、去掉公共诊断 Pixi 字段、库构建和 d.ts/API 冻结。
