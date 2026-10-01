# F2 R6：RoomVisual + 交互 + 公共 API 冻结实施方案

> 状态：**已实现并收口**（2026-09-30。决策点 D1–D7 用户拍板全按建议；实现
> `a7af796` → 三路评审清偿 `4aa2a81` → 外部复审仲裁清偿 `83088f2`，收口记录
> 见上位方案 §12 与本文件 §8；权威源取证勘误见 §3.1 勘误注）。
> 上位方案：[F2 渲染器批](f2-renderer.md)（§7 R6 行）；前序：[R5 方案](f2-renderer-r5.md)
> 与 [R5 验证记录](../research/notes/renderer-r5-verification-2026-09-30.md)；架构现状：
> [frontend.md](../architecture/frontend.md)；视觉底账：[官方渲染器拆解](../research/notes/renderer-official-analysis.md)。
> 基线：`f2-renderer@1e3b50c`（renderer 261 测 + shared 4 测 + kk-cli 7 测全绿）。
> 官方对照：本机 screeps/renderer `a2db4a7`（RoomVisual 不在其内，〔官拆〕§1.6/G1）；
> 官方 API 文档权威源 **screeps/docs 仓库 `api/source/RoomVisual.md`**（2026-09-30
> 二轮取证勘正——docs.screeps.com 整页锚点抓取会命中同页 Game.map.visual 的
> 表，见 §3.1 勘误注）；官方 engine 序列化形：本机 screeps/engine
> `src/game/rooms.js:1146-1218`。

## 1. 交付什么

R6 收三类能力并冻结公共 API：

1. **RoomVisual 五原语**（line/rect/circle/poly/text）的样式映射与渲染，visual
   数据进 feed 通知路径；
2. **交互**：单个 RoomScene 的活动房间选择与非活动房间更新隔离（多房间 feed
   只写当前显示房）；R2 的 `pickAt` 内部接口升级为公共拾取回调/悬停高亮面；
3. **API 冻结**：`createRenderer` 公共面定型 + d.ts 类型导出 + 库构建；字体
   加载责任入库（G7）；`__room` 诊断句柄与诊断统计去留裁定；R5 移交的契约
   文档化项（目标罩 owner=来源取消语义、texts 口径等）落 TSDoc。

出口验收：官方 visual 用例集观感比对（黑盒，§7）+ API d.ts 评审 + 既有全部
测试零回归。**不做**（沿上位方案）：R7 性能预算/内存 soak 不提前宣称；引擎
既有文件不动；443 常量完整生成管线、世界地图（M4）、G3 徽标自托管不动。

## 2. 已核实的起点与缺口（逐条落行）

### 2.1 RoomVisual：契约有壳、通知断裂、渲染为零

- 契约壳已在：`packages/shared/src/feed.ts:134-182` 定义 `VisualStyle` 与五原语
  union（`LineV/CircleV/RectV/PolyV/TextV`），`RoomFeed.setVisuals` 在
  `feed.ts:189`。**但字段集与官方 API 文档不齐**（对照见 §3）：
  - 缺 `lineStyle`（`'dashed'|'dotted'`——官方五原语除 text 外均声明）；
  - `lineWidth`/`strokeColor`/`lineJoin`/`font`（CSS shorthand）系我方自造名，
    官方实为 `width`/`stroke`/无 lineJoin/`fontFamily+fontSize+fontStyle+fontVariant`；
  - circle 官方 radius 在 style 内（`circle(x,y,style)` 默认 10），我方 `CircleV.r`
    在顶层（`feed.ts:154-160`）。
- 通知断裂：`FeedStore.setVisuals` 只写仓库**不 notify**（
  `packages/renderer/src/feed/room-feed.ts:120-122`）；`FeedChange` 不携带
  visuals（`room-feed.ts:43-51`，notify 构造 `:77-89`）——visuals 永远到不了场景。
- 渲染为零：`RoomScene` 层栈无 visual 层（`scenes/room.ts:66-74` 五层：terrain/
  ground/objects/lightingDisplay/effects），无任何 RoomVisual 消费代码。
- 悬案：applySnapshot 保留旧 visuals（`room-feed.ts:103`——R1 清偿记录登记的
  R6 拍板点）。
- 消费方盘查：`VisualStyle|RoomVisual|setVisuals` 全仓 grep 仅 shared 定义 +
  FeedStore 存储 + harness HUD 的 `summary().visuals` 计数（dev/main.ts:225）。
  **api/editor/kk-cli 现无任何 visuals 消费面**——契约字段重排无破坏面。

### 2.2 多房间：仓库多房、场景单房、无过滤

- `FeedStore.rooms` 按 room 键多房存储（`room-feed.ts:67`），数据面 OK；
- 但 `index.ts:185-189` 的 `feed.onChange` 接线**对 roomKey 不过滤**，任何房间
  的变更都直写唯一 `RoomScene`；`RoomScene` 无房间键概念（`scenes/room.ts` 全文
  无 room 标识）——两夹具交替 applySnapshot 会互相覆盖场景，无活动房语义。
- 夹具房间名互异（plains=W1N1…showcase=W0C0），多房演示原料现成。

### 2.3 交互：pickAt 只有几何，无回调、无对象命中、无悬停

- `RendererHandle.pickAt` 是纯几何换算（`index.ts:239-249`，screen→world/cell），
  注释自证「对象命中 R3」至今未落；
- `CameraController` 事件面只有滚轮/拖拽/右键（`core/camera.ts:43-77`），
  pointermove 非拖拽时不产生任何回调；无 hover 状态；
- `ObjectView` 已带 `id/typeKey/x/y/motion`（`objects/objects-stage.ts:66-87`），
  命中所需元数据齐——命中可走**数据面**（feed 对象按格查，ADR-009 不遍历
  Pixi 树），无需 Pixi hitTest。

### 2.4 字体：加载责任全在 harness

- `dev/index.html:11-21` @font-face Roboto 400/700 + `dev/main.ts:25-29`
  `document.fonts.load` 三规格（55px 名字/82px bold 徽标/60px say——
  `objects/specs.ts:69,206-207`、`anim/effect-specs.ts:12` 取证）；库侧零字体
  契约，`atlasBaseUrl` 只管 atlas。G7 盯梢面：加载失败静默回退、不可验证。

### 2.5 公共面与库构建：冻结前的已知非最终项

- 标注「R6 评审」的临时面：`localPlayerId`（`index.ts:44-46`）、
  `tickDurationMs`（`:47-51`，G6）、`setCameraState`（`:80-82`）、
  `setPlaybackRate`（`:83-85`）、`__room`（`:256-262` defineProperty 附加）；
- `__room` 的 harness 依赖面：`dev/main.ts:75`（`?smoke=1` 猴补
  `objects.update`/`lighting.update` 采样 `:78-89`）、`:90-95`（`?sample=` 定帧）、
  `:206-211`（seek 循环逐步推动画）——**移除需替代面**（§6 D6）；
- 诊断统计现状：`RendererStats`（`index.ts:54-66`）全普通数据（backend/fps/
  zoom/atlasFrames/lighting/effects/ambient/lightingPass；R5 已正名 `texts`）；
- 库构建缺失：`package.json` 无 exports/files/main/types、无 build 脚本；
  tsconfig `noEmit`（无 d.ts 产出）；`vite.config.ts` 仅 dev harness 用。

### 2.6 R5 移交项（验证记录「评审补充登记」与文末）

| 移交项 | R6 处置 |
|---|---|
| `__room` 诊断字段与 getStats 诊断面去留 | §6 D6 裁定；统计面冻结（texts 口径维持） |
| 目标侧工作罩 owner=来源、来源删除即取消 | 契约文档化（TSDoc + §7 收口记录） |
| texts 统计口径 | 已正名，冻结不再改 |
| G7 字体加载契约 | §6 D5 |
| say 断流一 tick 回收等适配扩展 | 随冻结面写进 TSDoc 语义清单 |

## 3. 官方契约取证（G1 黑盒锚）

### 3.1 官方 API 文档（权威源 screeps/docs 仓库 `api/source/RoomVisual.md`）

> **勘误注（2026-09-30 二轮取证，外部复审坐实）**：本节初稿值（circle radius
> 10、形状 strokeWidth 0.5、text fontFamily/fontSize 分字段、padding 2、
> opacity 0.5）系 **docs.screeps.com 整页锚点抓取命中同页 Game.map.visual
> 表**所致——四次「独立」WebFetch（主会话×2、评审 A/C 各×1）共享同一失效
> 机制，构成集体误读；MapVisual.md 考古（radius 10/strokeWidth 0.5 吻合）
> 坐实来源。下表为仓库源权威值，实现/测试/夹具已按此清偿（`83088f2`）。
> **纪律沉淀：官方文档取证必须走仓库源（GitHub contents API），不用整页
> 站点锚点抓取。**

| 原语 | 签名 | 样式字段与默认值（仓库源权威值） |
|---|---|---|
| line | `line(x1,y1,x2,y2,[style])` | `width` **0.1**、`color` #ffffff（**any web format**）、`opacity` 0.5、`lineStyle` undefined(实线)/dashed/dotted |
| rect | `rect(x,y,w,h,[style])`（x/y=左上角点） | `fill` **#ffffff**、`opacity` 0.5、`stroke` undefined(无)、`strokeWidth` **0.1**、`lineStyle` |
| circle | `circle(x,y,[style])` | `radius` **0.15**、`fill` **#ffffff**、`opacity` 0.5、`stroke` undefined、`strokeWidth` **0.1**、`lineStyle` |
| poly | `poly(points,[style])`（**折线**——描边开放） | `fill` **undefined(无填)**、`stroke` **#ffffff**、`opacity` 0.5、`strokeWidth` **0.1**、`lineStyle` |
| text | `text(text,x,y,[style])`（x/y=**基线点**） | `color` #ffffff（any web format）、`font` **number\|string 四形态**（`0.7` 游戏坐标 / `'20px'` 绝对像素 / `'0.7 serif'` / `'bold italic 1.5 Times New Roman'`；缺省官方未载——我方取 0.8 官方示例值，G1）、`stroke` undefined、`strokeWidth` **0.15**、`backgroundColor` undefined（**启用时文字垂直居中，默认基线**）、`backgroundPadding` **0.3**、`align` center、`opacity` **1.0** |

另有 `clear()`、`getSize()`（≤500KB 限额——初稿写 1000KB 系旧版记忆，仓库
源为准）、`export()/import()`。四原语默认值不对称（line 无 fill 概念、poly
默认描边不填充、rect/circle 默认填充不描边）——映射层逐原语应用，不共享一
张默认表。颜色官方为 **any web format**（命名色合法，文档示例 green/aqua），
不得登记灰区。

### 3.2 官方 engine 序列化形（screeps/engine `src/game/rooms.js:1150-1199`）

线格式 `{t:'c'|'l'|'r'|'p'|'t', …坐标几何, s:<玩家原样 style>}`；**服务器端
不补默认值**（rooms.js 只透传 `s`）——默认值全部由闭源壳在渲染端应用。因此
§3.1 默认值表就是壳行为的唯一权威记录，映射层在**我方渲染端**应用默认值
（与官方壳同位），单测逐值断言。

### 3.3 坐标相位（官方明载契约，非黑盒推断）

仓库源明载：*All draw coordinates are measured in game coordinates and
centered to tile centers, i.e. (10,10) will point to the center of the creep
at x:10; y:10. Fractional coordinates are allowed.*——visual 坐标即格中心、
浮点合法；rect 的 x/y 明载为左上角点；text 的 x/y 明载为基线点。与我方对象
锚点/世界空间同相位：pixel = 游戏坐标 × cellSize。仍属 G1 的黑盒面收窄为：
文字垂直基线的像素近似（无背景取文本框底边≈基线+descender）、线端帽形状、
dash/dotted 具体节奏、非基础 web 色形态。

## 4. 模块与契约设计

### 4.1 shared 契约增量（无破坏面，§2.1 盘查为零消费方）

- `VisualStyle` 重排为官方字段全集：`width?/color?/opacity?/lineStyle?`（line）、
  `fill?/stroke?/strokeWidth?/opacity?/lineStyle?`（rect/circle/poly）、
  `radius?`（circle）、text 的 `fontFamily/fontSize/fontStyle/fontVariant/
  stroke/strokeWidth/backgroundColor/backgroundPadding/align/opacity/color`；
  删除 `lineWidth/lineJoin/strokeColor/font/r`（自造名，无消费方）。
- 原语几何形维持顶层自定形（`LineV.points` 二点对、`RectV.x/y/w/h`、
  `PolyV.points`、`TextV.text/x/y`；circle 的 radius 按 D1 入 style）。
- 新增运行时守卫 `isRoomVisual`（与 isSnapshot/isDelta 同纪律）。
- 通知路径：`FeedStore.setVisuals` 触发 notify；`FeedChange` 增
  `visuals: RoomVisual[]`（数组副本纪律同 objects）；`FeedView` 增
  `visuals(room)` 只读面（harness/场景重建读）与 `roomNames()`（D3）。

### 4.2 渲染层：VisualStage（新文件 `src/scenes/visual-stage.ts`）

- **层位**：`RoomScene.root` 顶部新挂 `visuals` 容器（effects 之上——visuals
  是标注面，官方观感恒在对象/特效之上；不受 lighting 压暗。z 序黑盒推断
  登记 G1）。R5 的 finite-effects 重挂顶仍在 effects 层内，不受影响。
- **纯逻辑半件**（可无 Pixi 直测）：每原语一个映射函数——官方样式默认值
  应用、颜色解析（hex 四形态、CSS 命名色、rgb(a)/hsl(a)、`transparent`；
  未支持的非基础 web 色形态退字段默认并登记 G1）、几何换算（游戏坐标×cell）、lineStyle
  分段参数化（dashed/dotted 沿路径切段；圆弧参数化；节奏常量入 spec 表，
  官方不可考证处登记 G1）。
- **pixi 半件**：Graphics（line/rect/circle/poly）+ Text 组合（text 底衬
  backgroundColor 圆角矩形 Graphics + 文本；v8 Text 无原生背景）。对齐/锚点：
  text align=锚点 x 语义；背景启用时垂直居中，无背景取底边近似基线
  （近似偏差登记 G1）。
- **重建策略**：`setVisuals` 全量重建（typical ≤100 条，R7 预算场景）+
  Text 精确销毁纪律（style/textureSource，R3 清偿⑤同款）；同引用短路
  （数组引用未变零成本）。每 tick 全量重建的帧成本登记为 R7 测量点。
- `RoomScene.setVisuals(visuals)` 入口 + `destroy` 对称清理；接线在
  `index.ts` onChange（随 D3 过滤后）。

### 4.3 活动房间选择与隔离（D3）

- 状态放 handle 层（`index.ts`），**FeedStore 保持纯数据仓库**（展示策略
  不进数据层）：`RendererHandle.setActiveRoom(room: string)` +
  `readonly activeRoom: string | null`。
- 语义：
  - onChange 监听只对 `activeRoom` 的变更触达场景；非活动房变更只更新
    仓库（**离房零 Pixi 写入**——单测锚：非活动房 applyDelta 后场景无
    notify/节点数不变）；
  - 首个 applySnapshot 自动激活一次（activeRoom=null→首房；此后只显式切）；
  - 切房 = 用该房当前仓库状态重建场景（terrain + objects(kind='snapshot'
    全量语义——复用「快照清瞬态防幻影移动」纪律) + visuals）；
  - **离房期间的事件不补播**（events 随增量消费，离房即弃；切回只见当前
    状态——登记进契约 TSDoc；官方观战切房同表现）。
- harness 多房演示：`?multi=1` 载入多夹具同 feed、双夹具并行回放、Tab 键
  轮切活动房，HUD 按房显示 time/objects（非活动房 time 前进=数据面在收、
  场景不动=隔离生效的肉眼证据）；单测承担严格证明。

### 4.4 拾取回调与悬停高亮（D4）

- `PickResult` 增 `objectId?: string`：命中解析走数据面——activeRoom 的
  feed 对象按格查（同格多对象取 z 序高者（`objects/z-table.ts` 全表），
  再平手取 id 序；移动中 creep 按**数据格**命中（非插值渲染位），契约
  TSDoc 写明）；`pickAt` 同形升级。
- 公共回调面：`handle.onPick(cb)`（点击：pointerup 且位移 ≤4px——阈值
  常量入 spec）、`handle.onHover(cb)`（悬停格变化时触发，含 `null` 离房/
  拖拽中抑制）；`Set` 多订阅、返回退订函数、destroy 清理（订阅不悬挂）。
- 悬停高亮：白描边圆环挂 `visuals` 层（随 hover 格更新；**官方无高亮形态**
  ——官方是 hover 信息卡，不在 R6 范围；高亮为交互辅助，登记新灰区 G13）。

### 4.5 字体/资源加载契约（D5，G7）

- `CreateRendererOptions.fontsBaseUrl?: string`：库内预载 Roboto 400/700
  三规格（55px/82px bold/60px——库内实际使用面单源化为一处常量表），
  `document.fonts.load` allSettled 非阻塞语义同 harness 现行；失败=回退
  字体渲染（G7 登记），不阻塞首帧。
- 可验证性：`getStats().fonts: 'loaded' | 'fallback' | 'unset'`（plain 数据）。
- harness 迁移到契约面（index.html @font-face 保留——字体文件仍由页面
  HTTP 提供，库负责 load 时序与状态上报）。

### 4.6 公共 API 冻结表

| 面 | 处置 |
|---|---|
| `container/cellSize/backgroundColor/atlasBaseUrl/preference` | 冻结（现状语义） |
| `localPlayerId` | 冻结（G9 语义，TSDoc 收口） |
| `tickDurationMs` | 冻结（G6：动画时序参数，缺省 1000；**不承诺**等于服务器 tick 间隔——推送节奏归调用方，TSDoc 写明） |
| `fitRoom()/setCameraState()/setPlaybackRate()` | 冻结（playbackRate：动画时钟倍率，0 暂停，保留进度；数据推送节奏归调用方——TSDoc） |
| `getStats()` | 冻结现有键集（texts 口径维持）+ `fonts`（D5）+ D6 增量 |
| `feed: FeedView` | 冻结 + `visuals(room)/roomNames()`（D3） |
| `pickAt()` / `PickResult` | 冻结 + `objectId?`（D4） |
| `onPick()/onHover()` | 新增冻结（D4） |
| `setActiveRoom/activeRoom` | 新增冻结（D3） |
| `advanceAnimation(ms)` / 帧计时诊断 | D6 裁定 |
| `__room` | D6 裁定（建议移除） |
| `destroy(): Promise<void>` | 冻结（幂等、释放全部订阅/租约） |

R5 移交契约语义随 TSDoc 落 d.ts：目标罩 owner=来源（来源删除即取消罩）、
visuals 全量替换、离房事件不补播、say 隐私过滤（player=null 全滤私有）、
快照清瞬态/位置即时同步、填充补间白名单。

### 4.7 库构建（D7）

- 构建：vite lib 模式 ESM 单入口（`external: ['pixi.js', '@kkscreeps/shared']`
  ——host 应用自带 pixi，复用 workspace shared）+ `tsc emitDeclarationOnly`
  产 d.ts；`package.json` 补 `exports/types/files`（files 含 `assets/`
  atlas+fonts+LICENSE）与 `build` 脚本（进 `pnpm run check` 链）。
- 零新依赖（vite/tsc 已在 devDeps）。

## 5. 分步实施（R6 内部序，仍一片收口）

| 步 | 内容 | 当步可检查结果 |
|---|---|---|
| S1 | shared 契约重排 + FeedStore 通知/读面（§4.1） | shared/renderer 测绿；visuals 守卫单测 |
| S2 | VisualStage 纯逻辑+pixi 半件 + RoomScene/index 接线（§4.2） | visual 用例夹具 harness 截图逐原语核对 |
| S3 | 活动房间（§4.3）+ harness `?multi=1` | 隔离单测锚 + 多房演示录证 |
| S4 | 拾取/悬停（§4.4） | 命中优先级单测 + harness hover 演示 |
| S5 | 字体契约（§4.5） | getStats().fonts 三态单测 + harness 迁移 |
| S6 | API 冻结与库构建（§4.6/4.7）+ R5 移交契约 TSDoc | dist+d.ts 产物、`pnpm run check` 全链绿 |
| S7 | 验证收口：全链回归、3 路评审、清偿复审、回写上位方案 §12 与灰区表（G1/G7 更新、G13 新增） | 收口提交 |

夹具：`fixtures/visual-cases.json`（合成，标注来源——引擎无 visuals 通道
§8.3，exporter 不动）：五原语×样式矩阵（默认值组/dashed/dotted/对齐/背景/
描边/opacity/浮点坐标/`rect(x-0.5,y-0.5,1,1)` 单格框 idiom 用例），terrain
复用 plains 快照。harness 数字键切换（空房+setVisuals 注入）。

## 6. 决策点（请拍板）

- **D1 VisualStyle 对齐**：样式字段全对齐官方 API 名（含 `width`/`lineStyle`/
  text 分字段；circle `radius` 入 style、删顶层 `r` 与自造三字段）。备选：
  保留我方形加映射注释。**建议：全对齐**（黑盒契约 1:1，玩家按官方文档写的
  style 对象直传可用）。
- **D2 visuals 生命周期**：`applySnapshot` 时 visuals **清空**（快照=新 tick
  状态起点，官方每 tick 全量下发，防陈旧残留）。备选：维持现状保留。R1
  悬案就此销账。**建议：清空**。
- **D3 活动房间面**：`setActiveRoom/activeRoom` 入 handle + FeedView 增
  `visuals(room)/roomNames()`；首快照自动激活一次；离房事件不补播。**建议：
  按此冻结**。
- **D4 交互面**：`onPick/onHover` 多订阅回调 + `PickResult.objectId`（命中
  优先级=z 序）；悬停高亮=白描边圆环（非官方形态，登记 G13）。备选：高亮
  不做、只回调。**建议：做高亮**（R6 范围明列悬停高亮）。
- **D5 字体责任**：`fontsBaseUrl` 选项、库内预载+`getStats().fonts` 状态。
  备选：调用方全责、库只导出规格常量。**建议：库加载**（G7「定为库的加载
  责任」字面；回退可验证）。
- **D6 `__room` 去留**：**建议移除**，替代面两个普通数据契约——
  `advanceAnimation(ms)`（定帧动画推进：`?sample=`/seek/截图钉帧/未来黄金
  帧回归同用）+ `onFrameTiming` 诊断回调或 stats 计时字段（smoke=1 采样
  替代；形态实现时定，R7 perf 直接复用）。备选：`__room` 保留为 d.ts 外
  dev-only 字段至 R7（冻结面更小，但 R7 还要再动一次公共面）。
- **D7 库构建形态**：vite lib ESM（pixi/shared external）+ tsc d.ts +
  exports/files 补全。备选：tsc 多文件直出（产物形态碎，host 打包重）。
  **建议：vite lib**。

## 7. 验收口径

1. **官方 visual 用例集观感比对**（G1 黑盒）：S2 夹具在 harness 双后端截图，
   逐原语对照 §3.1 默认值表与语义（几何/颜色/透明度/线型/对齐）人工核对；
   不可考证处（文字锚点/端帽/dash 节奏/其他 CSS 色形态）逐项登记 G1，
   不宣称源码级保真。官方 UMD 参照路（R5 prepare-official-reference）不含
   RoomVisual，此条以文档+截图为基准。
2. **API d.ts 评审**：dist d.ts 为评审对象（3 路评审之一带 API 面清单核对
   §4.6 表）。
3. **零回归**：renderer 261 + shared 4 + kk-cli 7 测全绿；tsc、fmt/clippy、
   文档链接、资产生成链重生哈希一致（visuals 无新资产，预期零 diff）；
   cargo test -p kk-cli。
4. 自动验证增量：默认值表逐值断言、dash 分段几何、命中优先级、activeRoom
   隔离零写入锚、回调退订/destroy 清理、visuals 全量替换/快照清空（D2）、
   fonts 三态。

## 8. 风险与坑（含防错清单应用）

- **v8 无原生 dash**：手写分段（直线/圆弧参数化），纯函数可测；别在
  Graphics stroke 上找不存在的属性。
- **Text 生命周期**：visuals 重建若漏精确销毁 → Canvas 纹理等 GC 60s 兜底
  的累积（R3 教训）；重建路径单测锚定节点/私有源回基线。
- **多房切换走快照语义**：禁补间（幻影移动棘轮，R4 坑）；`'visible'` 键
  boolean；Edit 后 git diff 逐行核对（R5 层序误伤教训）；涉及过滤/清空
  改写（D2）逐分支对照旧语义真值表（player=null 教训）。
- **单位换算**：fontSize 10 / radius 10 / backgroundPadding 2 都是游戏坐标，
  ×cellSize 转像素；strokeWidth 0.5 同——call-site 系数写进映射注释
  （R4 罩时序 ×0.9 教训的反向应用：官方无 call-site 系数，别自己加）。
- **契约增量兼容**：FeedChange 增字段为渲染器内部类型；shared 增量仅
  VisualStyle 重排+新守卫，消费方为零（§2.1）——收口时再 grep 一次复核。
- **pnpm-lock 共享根文件**（R0 登记）：本片零新依赖，预期无冲突面。
- 引擎/kk-cli 零改动；主树不动，全部工作在 worktree 提交；一片一笔，
  subject「前缀：描述」冒号前 ≤48 字符、全长 ≤350。

## 8. 执行记录（收口，2026-09-30）

- **实现 `a7af796`**：D1–D7 全落地（24 文件 +1794/-141）。契约重排（官方名
  1:1）+isRoomVisual 守卫；FeedStore.setVisuals 补通知/FeedChange.visuals
  原引用/快照清空（D2）/FeedView.visuals·roomNames；VisualStage 两半件
  （纯逻辑解析+pixi 渲染，同引用短路全量重建、Text 四标志精确销毁）；活动
  房 ActiveRoomController（D3）+harness ?multi=1；拾取/悬停（D4）数据面命
  中（z 序优先）；fontsBaseUrl 库内 FontFace（D5）+getStats().fonts 三态；
  __room 移除+advanceAnimation/onFrameTiming（D6）；vite lib+d.ts 闭包守卫
  （D7）；visual-cases 用例集。
- **三路评审清偿 `4aa2a81`**（评审 A 官方规格/B 契约生命周期/C 测试实测）：
  A 1 阻塞（dash 间隙宽度感知——round 端帽致默认宽粘连，夹具假阴性）+18
  建议全清（colorOrOff/全形断言/text α 分层/fonts 重试/无地形房素地重建/
  Backend·Stats 抽零 pixi 模块+守卫升级 import 闭包遍历/TSDoc 三处/销毁设
  防/harness 两处/测试锚七处）。二轮复审 13/13 核销通过，守卫盲区三处与
  A-J4 测试锚记 R7 加固项。
- **外部复审仲裁清偿 `83088f2`**（GPT 外部报告，主会话仓库源逐条仲裁）：
  **官方默认值取证对象错误成立**（§3.1 勘误注）——radius 0.15/strokeWidth
  0.1/text font 分形态/padding 0.3/opacity 1.0 全面改判；基础 web 颜色（命
  名色 148 键/rgb(a)/hsl(a)）；poly 开放描边；文字垂直锚官方明载（背景
  middle/默认基线）；有限几何双防御（**MAX_DASH_SEGMENTS=4096 截断——
  hypot(1e308,1e308) 有限会使切段 OOM，比报告所指死循环更深一层，实测坐
  实**）；悬停失效机制（数据/切房/相机变化重估）；指针手势纪律
  （pointerId/最大位移/lostpointercapture）；fonts 永不 reject+await 入回
  滚；零推进真 no-op+计时每帧刷新；PickResult.room/readonly 契约/
  ./assets/* 导出/政策 TSDoc。GPT 报告三项与 4aa2a81 重叠（无地形房、fonts
  缓存、d.ts 守卫）系首轮已清。二波增量复审（独立 agent）结论见收口提交。
- **验证终态**：renderer 322 测（基线 261 → +61）+shared 4+kk-cli 7 绿；
  tsc/闭包守卫构建（公共面 7 文件零 pixi）/文档链接/资产重生哈希一致/两包
  构建过；浏览器双后端（WebGPU/WebGL lighting real、fonts loaded、60fps）
  visual 用例集 53 条新默认值观感逐原语核对、?multi=1 八房常驻双回放 Tab
  隔离、悬停环+拾取 objectId+advanceAnimation 实测。
- **灰区收窄**：坐标相位/默认值/颜色格式/文字垂直锚规则/背景 padding 均
  升级官方明载契约（§3），G1 黑盒面收窄至：文字无背景时基线的底边近似、
  端帽形状、dash 节奏、font 缺省 0.8、非基础 web 色。上位方案灰区表 G1/G7
  更新、G13 新增（悬停高亮环非官方形态）。
- **二波增量复审记录（当时 PASS，后续完整复审发现 B1–B5，见下）**：复审员独立经 GitHub contents API 重取
  权威源逐行核对（radius 0.15=L150、strokeWidth 0.1 三处、opacity 1.0、
  padding 0.3、font 四形态、any web format 十处措辞、polyline、背景垂直居中、
  坐标语义），与主会话仲裁完全一致；NAMED_COLORS 程序化对照 68 标准值零错
  配（恰 148 键=CSS Color 4 总数）；hypot(1e308,1e308) 有限性独立推演成立，
  追加压测（off 相位天文段/超大 strokeWidth/零长段）全部 <1ms 有界返回。四
  项 P3 残留：feed.ts CircleV 注释默认值残留、83088f2 提交信息 green/aqua 表
  述失准（aqua 仅单测）、命名色键数措辞、手势三边界无行为级测试——前两者与
  措辞随手勘正（收口提交），**手势行为测试当时记 R7 加固，本轮 B2/B3 已补齐**。

- **完整二次外部复审清偿（B1–B5）**：既有默认值/双后端主用例验证通过，
  但定向探针发现基础 CSS token 解析、悬停状态重估、相机多指/capture、文字
  stroke alpha 与字体部分失败重试五项残留；原 PASS 不代表这些边界已覆盖。
  - B1：完整解析带符号/前导小数点/指数的数字、HSL deg/grad/rad/turn 单位，
    transparent 大小写/空白归一；非法颜色不再误匹配命名色原型或 hex。
  - B2/B3：离开清除光标坐标，所有重估入口统一冻结手势；释放后恢复当前
    悬停；相机只接管主键主指并匹配 pointerId，cancel/lostcapture/leave
    与卸载统一结束捕获。
  - B4：Text fill/stroke 分别携带颜色 alpha，Text.alpha 仅承担共同 opacity；
    背景保持独立 alpha。补真实 Pixi Text 样式及 middle 背景几何断言。
  - B5：任一规格失败或构造/add/load 同步异常均回滚本次全部 FontFace；
    fallback 可重试且不累积已成功 face，全部成功仍缓存复用。
  - 夹具默认宽标签改为 0.1，小圆标签靠近样例，缩短 styled 文案以免遮挡；
    保留 53 条及五原语全缺省案例。
  - 验证：renderer 350 测（+28）+资产新鲜度/typecheck/ESM+d.ts 构建及
    公共面 7 文件闭包守卫通过；kk-cli 7 测通过；文档链接检查通过。浏览器
    WebGPU/WebGL 均 visuals=53/fonts=loaded/lighting=real；原生点击命中
    (14,10) kk0000000000000002，拖拽后悬停恢复至 (21,20) 且 pick 不变。
    多指/capture 与字体失败路径以行为回归验证，未宣称浏览器真实多指测试。


- **2026-10-01 R7交接指针**：R6终态仍为350测基线，D1–D7不重开。
  R7阶段提交`821a02f`后修复Graphics context释放并裁剪rampart顶罩，工作树renderer362测通过；
  灰区G1–G13已由用户整表确认。正式预算/内存结论、完整三轮双后端及三路评审尚未收口，
  见 [R7验证记录](../research/notes/renderer-r7-verification-2026-09-30.md) 与 [交接提示词](f2-renderer-r7-handoff-2026-10-01.md)。

R7后续指针（2026-10-01）：[短窗口优化与真实回放](../research/notes/renderer-r7-short-verification-2026-10-01.md)
已按用户追加授权完成短测/生命周期修复/真实100tick场景，三路评审清偿复审。
R6 D1–D7与冻结API未重开；正常光照不扩档，长期验收暂缓且未冒称R7最终完成。

后续[放大黑屏修复](../research/notes/renderer-zoom-verification-2026-10-01.md)已通过双后端短GPU回归；
沼泽MaskFilter按视口裁剪并自行释放，R6冻结决策与公共API不变。
