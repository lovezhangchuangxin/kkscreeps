# F2 渲染器批：@kkscreeps/renderer 实现方案

> 状态：**方案定稿**（2026-09-30 用户拍板：§11 五决策点全按建议；基准
> HEAD=e7adf47，方案草案提交 cfe39fb）。分支 `f2-renderer`，
> worktree `.worktrees/renderer/`（主树留给引擎批会话，两线隔离）。
> 依据：用户 2026-09-30 指示——实现 `@kkscreeps/renderer`，**视觉与官方一致**；
> 不复用官方老栈（PixiJS 7 + 自研表达式引擎），用**最新 PixiJS + 更好架构**，
> 优化渲染性能，易扩展易维护。架构既定决策见 [ADR-009](../decisions/ADR-009-frontend-stack.md)
> 与 [frontend.md §四](../architecture/frontend.md)（本批是它的首个实装）。
> 官方渲染器逐文件拆解底账：[renderer-official-analysis.md](../research/notes/renderer-official-analysis.md)
> （下称〔官拆〕；关键常量已主会话抽查复核）。

## 0. 目标与总原则

- **视觉对官方**：同一状态数据下与官方客户端观感一致——资产同源（官方 SVG，
  ISC 许可）+ z 序同构 + 动效语义同构（时序分数/色表/规格逐项复刻，〔官拆〕
  为准）。**凡是刻意分歧或无法取证处，登记灰区表，不静默**（沿用兼容灰区纪律）。
- **架构现代化**：纯 TS 库、框架无关、数据驱动（渲染规格=静态表）；Pixi 对象
  绝不进公共 API（ADR-009 第 4 条）。
- **性能预算前置**：官方已知坑（destroy/rebuild 风暴、每文本挂全局 ticker、
  光束逐帧重画、无限动画不休眠，〔官拆〕§10）逐条设对策；指标见 §5，每片出口
  跑性能冒烟，验收以 harness 实测为准。
- **零引擎侵入**：引擎侧只新增只读 exporter（新文件），不改 `world_json.rs`/
  `snapshot.rs`（M2B 前置批宏化正押这两文件）等任何既有文件；引擎缺口登记
  回主序列（§8），不在本批顺手改。
- **每片独立走完整批次循环**（实现 → 测试绿 → harness 人工验收 → ≥3 路
  subagent 评审 → 清偿复审 → 单一归因提交，subject ≤350）。

## 1. 调研结论（2026-09-30）

### 1.1 官方渲染器（PixiJS 7.4.2 + @pixi/layers 2.0）

〔官拆〕全文。承重结论：

1. **z 序是静态表不是动态排序**：全局 5 层
   `terrain → wallGraffiti → objects → lighting → effects`，objects 层内每类型
   固定 zIndex（-1 road … 18 constructionSite，全表见〔官拆〕§2）——我方可
   预建固定层容器，**零每帧排序**。
2. **rampart 是顶层罩不是底层**：私有 rampart 画成整片色区路由到 effects 层
   α0.4 ADD，永远盖在 creep/结构上（最易做错的一条）。
3. **terrain 是合并 path + 烘焙**：wall/swamp/rampart 各生成连通轮廓合并的
   SVG path（外凸圆角 r50），烘焙 RenderTexture + md5 门控；plain 是底色+
   噪声平铺。**不是逐格贴图、无变体、无坐标 hash**。
4. **creep body 环算法**（常量已复核）：合并同类型部件 hits → 升序排 →
   非 move 自头顶向两侧对称展开、move 自正下展开，弧长 ∝ hits；掉血=弧短、
   死亡=弧消失；无 tint 无闪烁。
5. **动效全由 actionLog 驱动**，时序全是 tick 时长分数（1/4-3/4、0.6 tick、
   0.2/0.8 tick）；**无结构血条、无伤害/治疗浮动数字**（全仓 grep 裁决）。
6. **RoomVisual 不在开源渲染器内**（闭源客户端壳绘制）——只能黑盒对齐。
7. 光照=lighting 层 MULTIPLY(0x808080 环境) + display 事件 SCREEN 混合。

### 1.2 PixiJS 选型（实现期锁死）

- **v8 最新稳定（当前 8.19.x）**：WebGPU 优先 + WebGL 自动回退；对本批关键
  的能力——`RenderGroup`（容器整体 GPU 单元）/`container.bake()`（静态子树
  预变换）/`cullable/cullableChildren/cullArea`（视锥剔除）/重写版 Graphics
  （RoomVisual 与程序绘制）/BitmapText/混合模式（ADD/MULTIPLY/SCREEN，官方
  光照复刻需要）。来源：pixijs.com v8 官方指南（render-groups/performance-tips/
  v8 migration）。
- 依赖最小面：`pixi.js` 单包；不引入 @pixi/layers 等价物（v8 原生 sortableChildren
  + 静态层表已足够，见 §2）。
- 工具链：Node 24（本机 v24.11.0）/pnpm 12 workspace（已就绪）。

### 1.3 我方数据面（引擎现状，零侵入消费）

- `World::world_json()` / `world_json_for_viewer()`（`kk-world/src/world_json.rs`）
  已产出官方形状 JSON：`{time, user, gcl, visibleRooms, rooms:{name:{name,
  terrain:[2500 码], energyAvailable…, events[]}}, objects:[官方 camelCase 字段]}`
  ——**这就是夹具的现成原料**。
- terrain 编码同官方：2500 字节位掩码 wall=1/swamp=2/lava=4（`kk-core/terrain.rs`）。
- 对象面：`RoomObject` 15 typed 变体 + Generic 透传桶（tower/rampart/lab 等
  2B 前在 `data` JSON 里带 structureType 与官方字段——渲染器解析 data 即可全量
  渲染，2B 转正后零改动）。
- **动效驱动缺口**：`CreepObj.action_log` 现为 u16 位图（M1 压缩，仅 harvest/
  build/upgrade/reserve 四位，无坐标无战斗动作，room.rs:76 注释明言 M2 换官方
  13 键形态）；但 `RoomState.events`（官方 EVENT 形状，含目标 id）已在投影。
  → **v0 动效走事件面驱动**（EVENT_ATTACK/HEAL/… + id→坐标解析），actionLog
  完整形态落位后作为第二通道接入（契约双通道设计，§3）。

### 1.4 资产与许可

- 官方 SVG×117 + 程序底纹 PNG×9（`metadata/images/`），**ISC 许可**（LICENSE.txt
  已核）——vendor 进仓 `packages/renderer/assets/svg/`，随附 LICENSE 原文。
- Roboto 字体（官方文字常量 Roboto 55/60px）单独 vendor（Apache-2.0）。

## 2. 架构设计

### 2.1 包内布局（frontend.md §四 的落地细化）

```
packages/renderer/
├── src/
│   ├── core/       renderer 生命周期、ticker、viewport（平移/缩放至光标/钳边/拾取）、层栈
│   ├── scenes/     room.ts（50×50 房间场景装配）；worldmap.ts 占位（M4）
│   ├── objects/    registry.ts（类型→渲染描述符）+ 各类型实现 + creep/bodyRing.ts
│   ├── feed/       RoomFeed 适配：snapshot 按 id 对账→场景增量；delta 应用；事件→动效映射
│   ├── assets/     atlas 清单与加载、多分辨率（@1x/@2x）
│   ├── anim/       tick 时钟与插值、tween 池、效果播放器、进度条/气泡/徽标
│   └── index.ts    createRenderer 公共 API（R6 冻结）
├── assets/svg/     官方 SVG vendor + LICENSE（生成物 atlas 入 assets/）
├── scripts/        build-assets.mjs（SVG→atlas）+ check-assets-fresh.mjs（CI 新鲜度）
├── dev/            回放 harness（Vite 页面，不进库构建）
├── fixtures/       exporter 产物 + 手工边界夹具（提交入库）
└── test/           Vitest（feed 对账/differ、body 排布、z 表、visual 映射纯逻辑）
packages/shared/src/feed.ts   RoomFeed 契约 v0（§3）
```

数据流单向：
`夹具/网络投影(RoomSnapshot/RoomDelta) → feed 对账器(id 索引+池) → 场景图(固定层栈)
 → Pixi v8 渲染`；动画时钟独立于数据到达（tick 时长可配置；数据断流动画照跑，
对齐官方〔官拆〕§7）。

### 2.2 层栈（对官方 5 层的同构复刻，零每帧排序）

预建固定层容器（等价官方 pixi-layers 路由的静态化）：

```
terrain(烘焙纹理,RenderGroup) → ground(road 连通绘制) → objects(每类型固定 zIndex
子容器,静态) → lighting(离屏纹理合成:0x808080 环境光 MULTIPLY + 亮斑 SCREEN)
→ effects(动效/文字/rampart 罩) → hud(harness 交互辅助,不属渲染器)
```

- objects 层 zIndex 子容器按〔官拆〕§2 全表建槽（类型→槽映射在 registry 表）。
- **lighting pass**：离屏 RenderTexture（环境光灰底 + glow 亮斑 ADD）→ 以
  MULTIPLY 合成回主场景，再叠 SCREEN 亮斑——复刻官方 PassThroughFilter 语义；
  WebGPU 下若滤镜能力缺口则降级"仅 ADD 亮斑、无全局压暗"并登记灰区。
- rampart：整片连通色区（own `#105010/#44ff44`、enemy `#501010/#ff4444`）挂
  effects 层 α0.4 ADD——顶层罩，不进 objects 层。

### 2.3 数据驱动渲染规格（保留官方 metadata 思想，去掉运行时表达式引擎）

- 每对象类型一个**渲染描述符**（TS 静态表）：`{ zIndex 槽位, 纹理键(按状态/
  等级/变体), 程序部件(圆/弧/条/文字)声明, 动效声明(事件键→播放器+时序分数),
  差量键(哪些字段变化才重绘) }`。**表在编译期存在，无运行时求值**——官方
  "90% 视觉规格是数据"的可维护性 + 零表达式引擎开销。
- 新增对象类型 = 加一个描述符文件 + 注册（2B 转正 tower/lab 等、M4 新类型
  都走这条路）；服务端注入渲染元数据扩展点保留（描述符可被运行时补丁表覆盖）。
- 差量更新对齐官方 `props` 思想但实现为**字段级脏标记**（描述符声明关注的
  字段集，对账器只 diff 这些字段，命中才调用该部件重绘）。

### 2.4 性能对策（逐坑对应官方〔官拆〕§10）

| 官方的坑 | 我方对策 |
|---|---|
| processor 重跑 destroy/rebuild（无池） | 对象视图池（按类型，id 对账复用；dispose 仅在房卸载） |
| 每文本挂 Ticker.shared + 每帧 getBounds | 文本事件驱动更新（内容/缩放档变化才重绘）；zoom 档位化重排 |
| 光束逐帧 Graphics 重画 | 束贴图预生成（构建期烘几条束变体）→ scale/rotation 变换复用 |
| 无限动画不休眠 | 常驻动画（炮塔自转/source 呼吸）统一由动画调度器按可见性/zoom 档降频或挂起 |
| 运行时生成 SVG data-URI | 全部资产构建期栅格化入 atlas；terrain path 烘焙带 md5 门控缓存（同官方） |
| 每帧 Layer 排序 | 静态层槽（§2.2），零排序 |

terrain：官方路径合并算法移植（连通轮廓+外凸圆角）→ 一次性烘 2048² 纹理 +
噪声层；swamp 滚动噪声用 TilingSprite 偏移（GPU 侧，无重绘）。

### 2.5 动效系统

- 事件映射表：`EVENT_ATTACK/HEAL/REPAIR/BUILD/HARVEST/…`（官方 EVENT 常量）
  + 目标 id→坐标解析 → 效果播放器（咬合突进/双层光束/罩/扇形波），时序与
  色表逐项取〔官拆〕§4 数值。
- 第二通道（actionLog 完整形态落位后）：官方 13 键 actionLog 直接驱动（契约
  预留 `RoomDelta.actionLogs`，v0 不产出）。
- say 气泡：契约预留 `RoomDelta.says` 通道（引擎暂无 say 事件面——登记 §8）；
  v0 夹具手工注入演示；视觉规格按〔官拆〕§3。

## 3. 契约 v0（@kkscreeps/shared 新增 `feed.ts`；渲染器只对契约编程）

```ts
/** 地形：2500 位掩码码（wall=1/swamp=2/lava=4），同 kk-core/terrain.rs。 */
interface TerrainData { codes: number[] }          // Uint8Array 亦可（序列化面 number[]）

/** 房间快照（首帧/切房全量）。字段=world_json objects 的官方 camelCase 子集。 */
interface RoomSnapshot {
  time: number; room: string;
  terrain: TerrainData;                            // 快照携带地形（烘焙门控键）
  objects: RoomObjectState[];                      // id/type/x/y + 类型字段（typed 15 类 + Generic.data）
  energyAvailable?: number; energyCapacityAvailable?: number;
}

/** 增量（每 tick 推送）；v0 由 harness/夹具 differ 生成，M3 网络投影同形替换。 */
interface RoomDelta {
  time: number; room: string;
  upsert: RoomObjectState[]; removedIds: string[];
  events?: RoomEvent[];      // 官方 EVENT 形状（动效第一通道）
  actionLogs?: ...;          // 预留：官方 13 键形态（第二通道，v0 不产出）
  says?: SayEntry[];         // 预留：说话通道（引擎缺口登记 §8）
}

/** 玩家绘制（RoomVisual API 的序列化形；样式黑盒对齐官方，灰区表盯梢）。 */
type RoomVisual = LineV | CircleV | RectV | PolyV | TextV;   // 各含 {x,y,样式的官方字段}

/** RoomFeed：与后端变更集同构的输入接口（frontend.md §四）。 */
interface RoomFeed {
  setTerrain(room: string, t: TerrainData): void;
  applySnapshot(s: RoomSnapshot): void;
  applyDelta(d: RoomDelta): void;
  setVisuals(room: string, v: RoomVisual[]): void;
}
```

- 夹具格式 = `{ room, ticks: [RoomSnapshot, RoomDelta, …] }`（首快照+增量链）；
  exporter 直接从 world_json 逐 tick 产出，harness differ 负责快照→增量。
- 对象 id：夹具内 `string`（world_json u64 序列化）；坐标 50×50 网格。
- 契约版本化注释头（v0 未冻结，M3 投影落地时校准——feed 层薄适配隔离漂移）。

## 4. 资产管线与夹具产出

- **vendor**：`assets/svg/`（官方 117 SVG + 9 PNG，ISC LICENSE 随附）+ Roboto
  woff2（Apache-2.0 NOTICE）。
- **build-assets.mjs**：`@resvg/resvg-js`（prebuilt napi，无构建脚本，pnpm
  allowBuilds 纪律无扰）栅格化 @1x/@2x → `maxrects-packer` 打包 → Pixi Assets
  兼容 atlas JSON + PNG 入 `assets/`（**生成物入库**，和常量管线同一纪律）。
- **check-assets-fresh.mjs**：源 SVG/字体 hash 清单 vs 产物 hash，CI 新鲜度
  （挂 renderer 包 test 前，根 `pnpm run check` 自动拾取）。
- **夹具 exporter**：kk-cli 新增只读子命令（新文件，main 分发表一行）：脚本
  世界（settle_tests Fixture 同款摆法：全平原/标准房/沼泽墙混合/密集边界四张）
  跑 N tick 逐 tick 落 world_json → harness differ 转 `{snapshot, deltas[]}`
  JSON 入 `fixtures/`。**零 V8 依赖、确定性、只读公开 API**。

## 5. 性能设计与指标（已冻结为验收：2026-09-30 用户拍板）

对策见 §2.4。指标（harness perf 模式实测记录归档 `docs/research/notes/`）：

| 指标 | 目标 | 场景 |
|---|---|---|
| 帧率 | 60fps（帧预算 16.6ms，渲染 ≤8ms） | 1080p、满配房：300 对象/50 creep/100 visual/地形全类型 |
| 对账延迟 | 单 tick snapshot 对账 <2ms | 同上场景（id 索引+池复用） |
| 内存稳态 | 10k tick 回放无增长（池稳态，无累积分配） | 标准房 2000 tick 循环 |
| 启动 | atlas 预生成下 <1s 到首帧 | 含 WebGPU 初始化 |

开发机基线：本机（Apple Silicon）+ Chrome；WebGL 回退路径同场景跑通（数字
记录、不设门）。LOD/世界地图缩略（远景降细节）本批只留接口不实现（M4 地图场景一并做）。

## 6. 视觉对齐清单（验收口径）

对齐项=〔官拆〕§2-§7 的全部规格数值，验收方式：harness 对同一夹具渲染截图
vs 官方客户端（demo 模式喂同状态）人工比对 + 关键常量的单测断言（body 环/
z 表/色表/字体常量直接测代码常量）。逐片勾项，收口时全绿或进灰区表。

**灰区表（初始条目，收口时逐条销账或保留）**：

| # | 项 | 缘由 | 处置 |
|---|---|---|---|
| G1 | RoomVisual 样式映射 | 官方闭源壳绘制，开源渲染器无代码 | **R6 落地**（五原语+官方 any web 颜色+font 分形态；权威源 screeps/docs 仓库 `api/source/RoomVisual.md`，默认值逐值单测锚——初版取证误用 docs.screeps.com 整页抓取命中 Game.map.visual 表，外部复审坐实后勘正，勘误注见 r6 方案 §3.1）。剩余黑盒面收窄为：文字无背景时基线的底边近似（官方=基线点，偏差≈descender 待字体实测）、线端帽形状、dash/dotted 节奏（宽度感知自定）、text font 缺省 0.8（官方未载，取官方示例值）、非基础 web 色形态；**R7用户确认四类保留**：文字基线/默认font、端帽、dash、非基础色 |
| G2 | lighting 全局压暗 | v8 WebGPU 滤镜面若有缺口 | 真离屏 pass 已实现（世界对齐 RT + MULTIPLY 输出），WebGPU/WebGL 双后端 harness 实测 `lighting: real`；**R7用户确认按定义销账**：历史全局压暗缺口已补；防御降级路径仍在位且未触发，不能称验证过降级。normal质量档/性能差距另记 |
| G3 | 徽标图源 | 官方远程拉 screeps.com，我方无此端点 | v0 纯色圆退化（官方失败退化路径同款），M3 自托管 |
| G4 | say/浮动文字数据通道 | 引擎无 say 事件面（§8） | R5 say/says 已接通并验证隐私与寿命；合成取材，真实输入仍待引擎 |
| G5 | 动效输入与扩展家族 | 动作位图、对象/结算处理器缺口 | R5 全家族合成可运行；真实 actions 已覆盖六类事件。远攻/群攻在此分支无结算处理器，lab/power/say/nuke 仍待真实输入；PWR 16–18 图标可用（SVG 自 R0 vendor 自官方 metadata/images，评审实测渲染正常），19+ 无官方图标映射计 skipped；**R7用户确认继续分项挂账**：flare3_all.hasEffect未实现与lava独立视觉缺位是renderer自身缺口，不能全部归因引擎 |
| G6 | tickDuration 缺省 | 官方闭源壳传入值不可知 | **R6 冻结**：tickDurationMs 契约参数化（缺省 1000，G6），TSDoc 明载「仅动画时序参数，不驱动数据推送」；setPlaybackRate 冻结（动画时钟倍率、保留进度、推送节奏归调用方） |
| G7 | 字体/构建期效果采样微差 | 栅格化/抗锯齿及预模糊近似 | **R6 字体责任落库**：fontsBaseUrl 库内 FontFace 预载三规格（55/60/82bold），失败回退可经 getStats().fonts 三态验证（loaded/fallback/unset）；harness 已去 @font-face。**R7用户确认字体责任销账，采样微差保留**：束端部裁切、核弹四档 blur、固定种子 flicker/初始角 |
| G8 | ~~swamp 白罩层序~~ | ~~官方 zIndex 序下 v8 mask 失效~~ | **R2 二轮销账**（跳格 bug 污染的观察，修复后官方层序 mask 正常） |
| G9 | 用户色数据源 | 契约 v0 无 users 映射（引擎不投影用户色），官方 playerColor=users[user].color | R3 起 v0 采用官方 demo 同款 **userOwnerColor 模式色板**（own 0x8fbb93/#8fbb93、enemy 0xFF7777/#FF7777、未知 0xBBBBBB/#ffffff）；**rampart 私有顶罩**=own #105010/#44ff44、enemy #501010/#ff4444（terrain.js userOwnerColor 分支），**rampart 公有精灵**=playerColor 色板（rampart.metadata.js:5-7 的 userOwnerColor 分支——评审 A-B5 勘误）；本玩家=createRenderer `localPlayerId`。M3 契约落 users 映射后切换官方远程用户色路径并销账 |
| G10 | factory 官方优先级 bug（两处） | factory.metadata.js **:46** `store['power']||0 + store['energy']||0` ⇒ `power || (0+energy) || 0`（powerHeight）；**:175-176** `energy||0 + power||0 < total` ⇒ `energy || power || (0<total)`（hasOther——total>0 恒 true 白条恒显）。container 同形表达式无此 bug（括号齐全） | R3 **保真复刻**两处官方行为（评审 A-B4 补登记 ：176）；官方上游修复后随动 |
| G11 | mineral 未知类型退灰对 | 官方 `COLORS[mineralType]` 未类型即崩（TypeError） | R3 防御性退 O 灰对（engine 不可达态，仅脏数据触发）；官方修复语义后随动 |
| G12 | creep 白资源圆 when 表达式 | 官方 creep.metadata.js:140 `store['energy']\|\|0 + store['power']\|\|0 < resourcesTotal` 因 \|\| 优先级解析为 `store.energy\|\|store.power\|\|(0<total)` ⇒ resourcesTotal>0 恒真（意图应为 energy+power<total） | R4 保真运行时行为=resourcesTotal>0 即显示白圆（specs.ts CREEP_SPEC 注释登记）；官方上游修复后随动 |
| G13 | 悬停高亮环形态 | 官方悬停是信息卡（闭源壳），无高亮环形态可对 | R6 交互辅助面：白描边圆环随悬停格（room.setHighlight），非官方形态——观感自定，登记不宣称对齐；官方信息卡不在渲染器范围（客户端应用层） |

## 7. 切片划分（每片走完整批次循环）

| 片 | 内容 | 出口验收 |
|---|---|---|
| R0 | 脚手架与资产管线：包配置（TS strict/Vitest）、vendor SVG+字体+LICENSE、build-assets/check-fresh、dev harness 骨架（空舞台+相机+HUD）、CI 接线 | atlas 产出+新鲜度校验绿；harness 显示底色房 |
| R1 | 契约与夹具：shared feed.ts v0、kk-cli exporter 子命令、四张夹具、harness 回放控制（播放/暂停/单步/调速/切房）+ differ | 夹具入库；回放可驱动空场景状态机 |
| R2 | core+terrain：viewport 完整（缩放至光标/钳边/拾取接口）、path 合并烘焙（wall/swamp/plain/噪声/swamp 滚动）、层栈+lighting 骨架、出口渲染 | 四张夹具地形与官方观感一致（截图比对） |
| R3 | 静态对象全家：registry+描述符框架、15 typed+Generic（data 解析 tower/rampart/lab/link/terminal/…）、z 表、进度条/填充量/cooldown 全规格、rampart 顶罩、road 连通 | 满配夹具静态帧对齐官方；差量键单测 |
| R4 | creep 与运动：body 环（合并/升序/双向展开/色表）、移动插值+朝向、受击罩/死亡淡出/资源圆/名字、tough/NPC | 50 部件 creep 群夹具观感一致；插值单测 |
| R5 | 动效系统：事件→效果映射全表（咬合/光束束贴图/罩/扇形波/孵化弧/降级闪/safemode/nuke）、say 气泡（合成通道）、常驻动画调度器 | 事件夹具逐动效人工验收；时序分数单测 |
| R6 | RoomVisual+交互+API 冻结：五原语映射（黑盒对齐）、拾取回调/悬停高亮、createRenderer 公共面定型+类型导出 | 官方 visual 用例集观感比对；API d.ts 评审 |
| R7 | 性能与收口：perf 模式实测+归档、内存 soak、文档回写（frontend.md 现状节/roadmap M3 渲染器项勾选）、灰区表清理 | §5 指标达标或降级登记；≥3 路评审收口 |

R5 实施细化与结果见 [R5 动效系统实施方案](f2-renderer-r5.md) 和 [验证记录](../research/notes/renderer-r5-verification-2026-09-30.md)。R5 仍是
一个整体切片；文档中的六步为内部实现顺序，不改变 R0–R7 划分。

## 8. 引擎侧缺口登记（回主序列处置，本批不动引擎既有文件）

1. **actionLog 官方 13 键形态**（坐标+战斗动作；现 u16 位图）——room.rs:76
   注释本就排期 M2 schema 升级；落位后渲染器接第二通道（契约已预留）。
2. **say 事件面**（公开说话需进房间投影）——建议随 actionLog 升级一并。
3. **RoomVisual 收集下发通道**——M3 投影面（kk-backend），渲染器先行消费契约。
4. **2B 对象转正**（Generic→typed）后投影自动变全，渲染器零改动验收。

## 9. 风险与坑

- **WebGPU 滤镜/混合面缺口**（G2）：harness 首屏显示当前后端；降级路径先行。
- **SVG 栅格化保真**（滤镜/发光在 resvg 与浏览器渲染差）：R0 逐张比对登记。
- **pnpm allowBuilds 纪律**：resvg-js 是 prebuilt napi（无构建脚本）预计免审批；
  若实际触发审批则评估换 sharp 前先问用户。
- **与引擎批并发**：只在 worktree 提交；不碰 crates 既有文件；exporter 只读
  公开 API。主序列（M2B 宏化押 world_json/snapshot）与本批唯一共享点是**读取**
  其输出形状——契约 v0 若随 M2B 演进，feed 薄适配层吸收。
- **夹具确定性**：exporter 世界必须无墙钟/随机源（TickInput 纪律同源）；断言
  两次导出逐字节一致。
- 工具坑沿用：zsh `===` 分隔符 glob、commit subject ≤350、跑套件显式 cd。

## 10. 与既有验收体系的隔离

- 前端验收（Vitest/类型检查/harness 截图/perf 模式）与引擎验收（棘轮/bot/
  逐字节对账）零交集；根 `pnpm run check` 的 `pnpm -r run test` 自动拾取
  renderer 包测试，无 Rust 侧任何门槛变化。
- 本批不产生 screeps-ok/棘轮影响（纯前端）；不改 Cargo 依赖。

## 11. 决策点决议（2026-09-30 用户拍板，五点全按建议）

1. **性能指标数值**：按 §5 表冻结为验收（60fps 最坏忙房 / 对账 <2ms /
   10k tick 内存稳态 / <1s 首帧；本机 Apple Silicon + Chrome 为记录基准，
   WebGL 回退路径跑通记数不设门）。
2. **lighting 保真路线**：真离屏 pass 优先；WebGPU 能力缺口时降级
   "仅 ADD 亮斑、无全局压暗"并登记灰区（G2），不强制全平台真复刻。
3. **徽标 v0**：纯色圆退化（官方拉取失败退化形态同款），灰区挂账 G3，
   M3 账号系统落地徽章参数后升级参数化绘制。
4. **exporter 落点**：kk-cli 新增只读子命令（新文件 + main 分发表一行，
   仅调用公开 API；本批引擎树唯一增量触点）。
5. **切片粒度**：R0-R7 八片照走，每片独立走完整批次循环；R3/R5 不再细分
   （内部按规格表逐项机械复刻，评审走清单核对）。

## 12. 执行记录（随批追加）

- 2026-09-30：方案定稿（本节首条）；R0 起实施由新会话在 worktree 接手。
- 2026-09-30：**R0 脚手架与资产管线完成**。包配置（TS strict+noUncheckedIndexedAccess/
  Vitest 5/tsc --noEmit）；vendor 117 SVG+9 PNG+ISC LICENSE+Roboto woff2×2（来源
  README 登记出处）；build-assets.mjs（resvg 栅格化 @1x/@2x→maxrects 2048² 打包
  →每页 spritesheet JSON+PNG，@1x 1 页/@2x 2 页，重跑逐字节一致已验证）；
  check-assets-fresh.mjs（正例/篡改/多文件三路验证）挂 test 链最前；src 骨架
  （createRenderer/相机与相机数学纯函数/底色房/atlas 加载，公共面零 Pixi 类型）；
  dev harness（底色房+相机+HUD）。出口验收：18 测全绿、类型检查过、`pnpm -r run
  test` 自动拾取、harness 截图底色房显示 ✓（WebGPU 60fps；WebGL 回退路径渲染
  一致——后台面板 RAF 节流致 fps 采样读 1，R7 perf 模式出真数）。
  **实施期漂移/决策登记**：① pixi.js 取 8.21.0（方案"当前 8.19.x"为写作时观察，
  按 §1.2「v8 最新稳定」拍板意图取当前最新）；② Roboto 许可实为 OFL-1.1（方案
  §1.4 写 Apache-2.0 是旧版 Roboto 时代观察；fontsource 5.3.0 现行分发即 OFL-1.1，
  许可原文随附）；③ ~~栅格化尺寸=SVG 内在 viewBox 尺寸（与官方采样一致，7 个小写
  `viewbox` 文件构建期归一化、vendor 源不动；密度覆盖表 RASTER_OVERRIDE 预留）；
  @2x 恰为 @1x 整 2 倍~~（**已废弃，前提错误**——正确策略见下方清偿记录①：
  浏览器语义 width/height 属性优先，viewBox 仅兜底；RASTER_OVERRIDE 覆盖表保留）；④ 9 张程序底纹 PNG 不进 atlas（TilingSprite/mask 需独立
  纹理），运行时直读 assets/svg/；⑤ 页 PNG 用 node 内置 zlib 手写编码器合成
  （resvg 出 RGBA 原始像素，零新原生依赖）；⑥ createRenderer 增加 `preference`
  选项（webgpu/webgl，§5 双后端记数用，harness `?backend=webgl` 切换）。
  评审：3 路 subagent 并行（官方规格/契约面/性能对策）。
- 2026-09-30（同日）：**R0 评审清偿**。2 阻塞+5 建议+若干备忘，逐条处置并经
  实测复核：
  ① **[阻塞·评审A] 栅格化采样策略改为浏览器语义**——评审实测发现官方渲染器
  经浏览器解码 SVG，内在尺寸取 width/height 属性（80/117 带属性，60 个属性与
  viewBox 分歧——宽或高任一不等 ±0.01 容差，如 factory 属性 200 vs viewBox
  41.2；含 8 个单引号属性文件与
  1 个 px 单位文件 extension-border200），且小写 `viewbox`（严格 XML 被忽略）
  有语义；主会话复算证实。修复：intrinsicSize 属性优先（单双引号+px 单位均
  认）→ viewBox 兜底，删除 viewbox 归一化（resvg 原生即严格 XML=浏览器语义），
  manifest 记录实际渲染尺寸；atlas 重生成（@1x 1 页/@2x 3 页），目检通过，
  重跑逐字节一致复验通过。最终取径 80 属性+37 viewBox+0 兜底，评审 A 独立
  复算 117/117 逐文件一致。原 §12③「与官方采样一致」的前提（viewBox 优先）系
  错误断言，以本条更正。评审 A 清偿复审通过（附两条尾巴已顺手清：px 单位
  正则与计数勘误——分歧数以本会话可复现口径 60 为准，评审初数 57 与复审数
  66 均系不同引号/口径下的中间值）。
  ② **[阻塞·评审B] atlas.ts 直通纹理路径 `../svg/`→`svg/`**（与自身路径契约
  矛盾，harness 根基址掩盖）。
  ③ [建议] createRenderer 失败路径 try/catch→kernel.destroy() 再抛（不再泄漏
  canvas/ticker/监听）；destroy 幂等化；级联销毁改 `app.destroy(true,{children:
  true})`（v8 语义：不动 Assets 共享纹理）；删 destroy 中 removeChild 死代码。
  ④ [建议] renderer.width 注释纠错（v8 中与 screen 同为逻辑像素，非"物理像素
  语义"）；typecheck 接入 test 链（check:assets && typecheck && vitest）；
  harness resize 改 rAF 延迟 fit（ResizePlugin rAF 时序竞态）；endDrag 加
  hasPointerCapture 守卫；deltaMode 换算加注释；manifest fetch 查 response.ok；
  atlas 页与直通纹理并行加载；@types/node 显式 devDep。
  ⑤ 测试补强（评审A建议）：单测重解析源文件断言 manifest 栅格尺寸一致
  （策略漂移单测层现形）、wOF2 魔数、许可内容（ISC/OFL）断言。19 测全绿。
  ⑥ 登记项（后续片处置）：STANDALONE 纹理 resolution=1 语义（R2 TilingSprite
  需显式 scale）；camera.state getter 暂无使用方（R2 拾取复核）；package.json
  exports/files 字段 R6 冻结时补；「场景无脏挂起 ticker」R2+ 层栈静态化后评估；
  PNG 产物绑定构建机 zlib 版本（跨机重建会触发 freshness 强制重建，纪律自洽）；
  官方 mineral 符号字体声明为 `'Roboto, serif'`（R3 注意）；pnpm-lock.yaml 为
  与主树共享的根文件（本批 +千行依赖条目），与引擎批合流时留意冲突面。
- 2026-09-30：**R1 契约与夹具完成**。shared 包实装：feed.ts 契约 v0（形状按
  e7adf47 world_json 投影取证——主键是 `id` 非 `_id`、结构类 type:"structure"+
  structureType 二级类型、事件 `{"event","objectId","data"}`；typed 字段经索引
  签名透传，R3 描述符收窄；运行时守卫 isSnapshot/isDelta/isRoomEvent；TS 源
  直发包，零运行时依赖）；kk-cli 新增 `export-fixtures` 只读子命令（新文件
  export_fixtures.rs + main 分发表一行 + **kk-cli Cargo.toml 增 kk-core/
  kk-schema 两行 workspace 依赖**——超出"新文件+分发表一行"字面范围的点，
  因构建对象面必需，特此登记）；四张脚本世界夹具（plains/standard/swamp-walls/
  dense-edge，settle_tests Fixture 同款摆法，固定 seed+固定 TickInput 墙钟
  注入 ADR-013 决策 5，空意图零 V8 推进 30 tick），两次导出逐字节一致实测；
  differ（src/feed/differ.ts，world_json ticks→snapshot/deltas，纯函数零运行时
  依赖，node 原生 TS 直跑）+ scripts/build-fixtures.mjs 生成入库（每夹具 1 快照
  +30 增量，全部非空——aging/decay/spawning 自然差量）；renderer feed 状态机
  （FeedStore：快照置换/增量对账/缺房与时间回退 fail-fast）+ RendererHandle.feed
  公共面扩展（非破坏性，冻结前）；harness 回放控制（1-4 切夹具/空格/./+-
  调速/F），浏览器实测全过（播放/暂停/单步/2×/4×/末尾 end 态不越界/切夹具
  重置）。验收：shared 4 测+renderer 36 测绿；cargo clippy -p kk-cli 零告警、
  cargo test -p kk-cli 5 测绿（确定性+时间序）；compat 套件不受影响（本片
  构建的 target/debug/kkscreeps 使其 e2e 两例由 skip 转 pass，属正向）。
  实施决策：① exporter 不注入玩家 intents（R1 静态世界+自然差量足够；运动/
  战斗夹具随 R4/R5 在同一文件扩展）；② 契约不改写 world_json 字段名（id/room
  直通，M3 投影对齐时统一再议 _id 映射）；③ 夹具 31 条全量入库（~184KB 总量，
  可接受）；④ differ 的对象变化判定用 JSON 序列化比较（夹具工具口径；渲染
  运行时字段级脏标记在描述符层互不替代）。
- 2026-09-30（同日）：**R1 评审清偿**。3 路评审零阻塞、12 建议全清偿：
  ① 夹具数据（评审 A）：swamp-walls 四格路改到经地形码核实的真沼泽格且
  hits=hitsMax=25000（原先 1/4 在沼泽且 3 格 hits>hitsMax 引擎不可达态）；
  standard 孵化任务自洽（need_time=10≡spawnTime−0）并补同格同名孵化雏形
  （t10 实测出生：任务清+spawning=false 出生面——R3/R5 孵化动态取材）；
  Generic tower/rampart data 按官方渲染消费面补键（user 归属键、塔
  storeCapacityResource、rampart isPublic，owner 为 kk 投影口径冗余键，R3
  登记 user/owner 映射）。② 契约文档（评审 A/B）：取证头掉落资源更正为
  type:"resource"（原误写 "energy"）、structureType 单向蕴含说明（spawn/
  constructionSite 也带）、pos 投影附带字段登记、EVENT 码与官方 1..12 相等
  的实测结论写入注释。③ feed 公共面（评审 B）：FeedStore 退为实现细节，
  新增 FeedView 接口（RoomFeed+summary/objects/terrain 读面），
  RendererHandle.feed 与导出面均用接口。④ harness（评审 B/C）：回放末态
  stopTimer（不留空转 interval）、stepGuarded 兜底（fail-fast 抛错→停机+
  HUD 报错行，不留异常循环）、F 文案改"视角复位"并注明重按数字键重放。
  ⑤ 工具（评审 B/C）：build-fixtures 无条件增量 cargo build（防过期二进制
  静默产陈旧夹具）、非空增量计数哨兵修正（原恒真）、spawn error 打印、
  readdirSync 归顶部导入。⑥ 登记项：M1 路磨损语义=被踩才衰减（movement.rs
  next_decay_time 递减），空意图夹具无路 decay/移除动态——留待 R4 运动意图
  夹具；四张夹具零 events（EVENT 通道现仅合成单测覆盖，R5 补战斗/事件夹具）；
  applySnapshot 保留 visuals 的 R6 拍板点（时间回退残留 + 房卸载 API 缺位）；
  shared 内部引用 `./feed.js` 后缀在 node 直跑下不做 .js→.ts 重映射（现仅
  type-only import 故安全，R7 文档回写记一笔）；"两次导出逐字节一致"Rust
  测试覆盖 5 tick、30 tick 全量由本会话手跑两次 diff 实证。清偿后全链复绿
  （shared 4+renderer 36+kk-cli 5 测；clippy 零告警）。三路清偿复审全过，
  尾巴随手清：§12 补记三条——plains 无 controller 时 source 容量 t1 被
  KEEPER 规则归一 3000→4000（引擎 faithful 行为，读夹具勿误判 bug）；
  isSnapshot 守卫暂不校验 objects 数组（v0 最小集，M3 网络入口收紧点）；
  契约 time 为 number（夹具 0..30 无碍，真实 shard tick ~1e7 距 2^53 三个
  数量级，M3 接 u64 time 在 feed 适配层留精度备注）。Generic「data 透传」
  措辞更正为「data 投影期解析展开」（实测夹具无嵌套 data 键）；
  dev/main.ts 头注 F 文案同步。
- 2026-09-30：**R2 core+terrain 完成**。path 合并算法移植（src/core/path-merge.ts，
  官方 pathHelper.js 逐行忠实移植）——**保真性硬证据：官方原版 stub-md5 直跑
  对照 12 用例（单格/横竖条/2×2/对角×diagConnect/L形/十字/整行）逐字节一致**；
  官方 quirks 保真（2×2 正方块拆双子路径、十字拆三子路径、N=4 下 (0,0) 底角
  圆弧）。地形烘焙（src/scenes/terrain.ts）：合并 path → Path2D → canvas 2D
  烘焙 2048² → 纹理——**实施决策：canvas Path2D 替代官方运行时 SVG data-URI**
  （§2.4 坑规避：无异步 img 解码，hash 门控重绘）；SVG 语义映射 paint-order:
  stroke=先描后填、BlurFilter 黑影=canvas filter blur（G7 盯梢）、官方
  tileScale=8×sizeRatio 的 canvas 等价式。层栈落地（terrain→ground→objects→
  lighting display→effects 静态五层）；lighting 真离屏 pass（G2：内容容器
  离屏渲 RT、MULTIPLY 精灵回场景、世界对齐仅 dirty 重渲——官方 filterArea
  屏幕对齐的等价语义，pan/zoom 零成本）；viewport 完整（钳边两态实测：缩小
  态居中锁定/放大态贴边钉住；pickAt 几何拾取接口）；exit-* 独立栅格化产物
  （standalone/、manifest 新段、TilingSprite wrap 需求）；feed→scene 接线
  （FeedStore.onChange → setTerrain hash 门控重建）。实测：四夹具地形截图
  （plain 底纹/边界墙圆角/swamp 合并轮廓+滚纹噪声+散点/dense-edge 出口缺口
  +箭头纹理+墙簇）、WebGPU+WebGL 双后端 `lighting: real` 60fps。修复：切房
  空集清理 bug（无沼泽房残留旧沼泽精灵——hash 门控把空集当未变跳过）。
  实施决策登记：① 重建门控哈希用 FNV-1a 替代官方 md5（缓存键语义等价，
  哈希值不出现在视觉面）；② exit 条不进 atlas（独立纹理 wrap 需求）→
  **清偿期更正**：exit 在 atlas 帧表保留（打包面完整性/单测口径），standalone
  为额外栅格化双份，32-64² 量级 VRAM 可忽略；
  ③ 拾取 R2 仅几何换算（对象命中 R3 描述符）；④ lighting RT 世界对齐
  （非官方屏幕对齐 filterArea）；⑤ swamp 滚动时基=ticker 帧数（60fps 基准，
  deltaMS/16.667）。验收：51 测绿（path-merge 11/camera 13 含钳边/assets 10
  含 standalone/differ 5/room-feed 5/fixtures 7）。
- 2026-09-30（同日）：**R2 评审清偿**。3 路评审 7 阻塞+8 建议，全清偿：
  ① [阻塞·A] lighting 离屏 RT 缩放缺失（content 世界尺度恒等变换渲 2048² RT
  → 成员裁剪在世界 [0,2048]² 且放大 2.44×，155+ 面墙无光影）——渲染前临时
  content.scale.set(renderSize/view)；复验四边墙光影全幅。
  ② [阻塞·A] wall bump 空间换算错（drawScale 混用 user/device 空间=8 vs 正确
  8×sizeRatio；循环上界误写 renderSize 只铺 41% 画布）——修为 8×sizeRatio+
  上界 view，推导入注释。
  ③ [阻塞·A] pickAt 半格系统性错位——官方几何实为**格中心 (x*cell,y*cell)、
  范围 ±cell/2**（对象锚点 x*CELL 与地形 path 经 -cell/2 精灵偏移后同相位，
  _all.metadata.js:12 取证），Math.floor(world/cell) 改 Math.round；
  room.ts 头注几何描述同步更正（原注释把 path 空间当世界空间）。
  ④ [阻塞·A→撤回] exit 条 @2x 渲染半尺寸——**评审 A 复审撤回误报**（v8
  TilingSprite setUvs 用 texture.frame.width 计算节距：@2x 64×(100/64)=100 与
  官方 32×(100/32)=100 覆盖恒等，几何与 resolution 无关，@2x 仅纹素更密）；
  清偿期的 source.resolution 设置属无效代码已撤销，注释改真实语义。
  ⑤ [阻塞·B/C] 共享纹理误毁双案：clearSwampSprites 无差别 destroy(texture)
  连杀共享 noise2 源（切出沼泽再切回即渲染损坏）+ TerrainStage.destroy 连杀
  Texture.WHITE/atlas 共享源——分类销毁（共享纹理只拆树；烘焙私有 canvas
  纹理才带 texture 销毁）。
  ⑥ [阻塞·B] RendererHandle.feed 类型回归（R2 重写 index.ts 时退回实现类
  FeedStore）——修回 FeedView 接口。
  ⑦ 实证新 bug（C6 备忘预言）：清场先毁 mask（overlay）后毁 masked（noise）
  时 v8 MaskFilter 读 null.destroyed 直接抛错打断渲染帧（往返 swamp→plains
  全黑复现）——清场先摘 mask 再销毁。
  ⑧ [建议] setTerrain 返回 wallRebuilt，RoomScene 仅重建时 markDirty（每 tick
  增量不再触发 2048² 离屏重渲）；floor 层序照官方（mask 先 ground 后）；
  lighting degraded catch 补 console.warn 留痕；lighting.destroy 补 texture
  销毁对称；白罩烘一次共享（省 1/6 烘焙面）；白罩/亮填描边入 spec 表。
  ⑨ [建议·A6→G8] swamp 白罩层序照抄官方 zIndex 序致 v8 color-mask 失效
  （二分实验定位：层序因素、共享纹理无辜）——取 base→罩→noise 功能正确序，
  新灰区条目 G8；[A7] 官方 lighting 三档（normal/low/disabled）仅实现 normal
  登记为 R7 收口项；[S2] §12 决策②措辞更正：exit 在 atlas 帧表保留（打包面
  完整性），standalone 为额外栅格化双份（32-64² 量级可忽略）。
  清偿后：51 测绿+typecheck+文档链接过；harness 复验（墙光影全幅/swamp 往返
  swamp→plains→swamp 零错误零黑屏/mask 剪裁正常/lighting real 双后端）。
- 2026-09-30（同日）：**R2 清偿复审二轮**。B 路全过；A/C 各抓到清偿期新引入
  回归与一项误报，二轮清偿：
  ① [新阻塞·A/C 共识] clearSwampSprites 清偿时把原 `.filter()` 快照迭代改成
  活数组 for...of——destroy→removeFromParent 同步 splice 跳格（pixi 实类复现，
  每轮残留 ov0+n0：持已毁共享纹理留树+噪声摘罩裸奔）——修回快照迭代。
  ② [G8 二轮定案] 一轮的「官方 zIndex 序致 mask 失效」系跳格 bug 污染的观察
  （stale 噪声满房裸奔同形）；修复后官方层序 mask 剪裁完全正常——层序回归
  官方保真，灰区 G8 销账（观察-污染-更正全程记录本条）。
  ③ [A4 撤回] exit @2x「半尺寸」系评审 A 误报（frame×tileScale 覆盖恒等），
  其撤销已复核；清偿注释错误断言（「texture.width 已是 32」）更正为真实语义。
  ④ [A/C vs B 矛盾裁定] 共享 overlayTexture 双 destroy 安全性——主会话 pixi
  实类实测：二次 destroy 经 _source null 守卫为 no-op（事件只发一次），B/C
  正确，无需所有权标记。
  ⑤ M2 用例名更正（hash 断言由 freshness 承担）。
- 2026-09-30：**R3 静态对象全家完成**。架构三段：**纯逻辑半件**（part-decl.ts
  契约/PartEnv/valuesEqual 值相等门控；z-table.ts 31 项 zIndex 全表+类型键推导；
  player-color.ts G9 色板；specs.ts 全类型规格常量表 file:line 取证；
  parts-naturals/structures.ts 28 组部件声明（id/keys/values 纯函数）——
  Vitest 零 pixi 直采）× **pixi 半件**（joins-naturals/structures.ts 按 partId
  的 create/apply；draw-helpers.ts 官方 object.js/sprite.js/circle.js/
  siteProgress.js 语义→v8 映射：width/height=scale、sprite 默认 anchor .5、
  pivot 除以 scale、blendMode 1=ADD、lighting 成员 SCREEN）× **registry 装配**
  （registry.ts：PART_DECL_TABLE×joins×Z_TABLE×PART_LAYERS 构建期强校验——
  join 缺件/层键漂移即抛错）。ObjectsStage 对账器：id 索引视图池（移除拆树、
  快照迭代纪律）、部件懒创建+值相等门控（比官方 props 深比较更强：键不变值变
  亦捕获；keys 保留为单测审计面）、slot/lighting/effects 三层路由。附属绘制：
  road 连通（road.js 四向几何移植，ground 层，格集变化全量重绘）、rampart
  私有顶罩（terrain.js:183-247 按 user 分组合并 path 烘焙，fill×0.3/stroke 25
  → G9 色板，α0.4 ADD 挂 effects 首位）、constructedWall 格并入墙烘焙（官方
  wall path 过滤含 constructedWall——TerrainStage 增格集并集入参，extras 引用
  相等门控零成本）。
  **实施决策**：① lighting 成员所有权二分（addMember owned 默认/共享纹理成员
  只 detach）——对象 glow 持共享 standalone 纹理，clearMembers 误毁即 R2 评审⑤
  同族事故，LightingPass.destroy 改先清 owned 再无纹理拆树；② 填充条官方
  pivot.y=高语义经 object.js 除以 scale 推导=锚点底边等价（anchor(0.5,1)）；
  ③ tower rotatable+能量条并件（官方条为炮塔子件随炮转，跨件挂载并入一件，
  anchor(.5,0)+pivot(0,32/scale) 官方混合语义照抄）；④ blink 模板 R3 冻结于
  可见相位（max(from,to)），R5 接管时序（constructionSite 0.8/controller 状态圈
  0.4/红闪 0.2/_isDisabled 0.5）；⑤ extractor cooldown 自转、塔自转、observer
  眼游走、portal/keeperLair 呼吸波、glow flicker 全部 R5（静态帧=规格初相）；
  ⑥ spawn 名字 v0 只显示己方（官方 showMyNames/showEnemyNames 门控的 demo
  配置同款），开关面 R6；⑦ flag 名字文字 R6（showFlagsNames 门控）；
  ⑧ creep/powerCreep R4、nuke R5——registry 未注册类型告警一次跳过不崩溃；
  ⑨ spawn.resourceScale 官方无 min 钳制（link/extension 有）逐件保真；
  ⑩ resourceCircle 契约消费 `state[resourceType] ?? state.amount`（kk 投影
  amount 键，R1 登记的差异消费面）；ownerOf=user??owner（R1 登记的映射点）。
  **官拆勘误**：§2「全部文字在 effects 层」对 mineral 符号不成立——源码 text
  处理器无 layer 字段（mineral.metadata.js:78-95），随 objects 层 z=2 槽位。
  **合成夹具**：showcase.json（40 对象/26 类型：G5 家族+双私有顶罩组+公有
  rampart+敌我双色+NPC 塔+_isDisabled+flag），scripts/build-showcase-fixture.mjs
  确定性生成入库；fixtures.test 扩展（exporter 四张 31-tick 纪律与 showcase
  3-tick 分治+类型覆盖断言）。**公共面新增（冻结前，R6 评审）**：
  CreateRendererOptions.localPlayerId、RendererHandle.setCameraState（纯数据）、
  handle.__room 运行时诊断字段；harness `?view=x,y,zoom` 钉住相机（截图纪律：
  标签激活触发 resize→fitRoom 复位——R0 登记的竞态族，钉住监听注册序在
  fitRoom 之后同帧覆盖）、`?player=` 切视角、@font-face Roboto（vite
  publicDir=/，字体 URL /fonts/——首版误写 /assets/fonts/ 触发 SPA fallback
  返回 HTML 致字体 error，已修）。验收：122 测绿（z 表 8/玩家色 5/规格值函数
  46/差量门控 3/既有 60）+tsc+harness 六夹具×双后端（WebGPU/WebGL 均
  `lighting: real` 60fps）截图逐项核对（z 序/双色/顶罩合并轮廓/条形堆叠/
  扇形进度/罗盘等级环/冷却闪/文字 Roboto 绿字黑描边/路连通对角四边形/
  并墙暗块+光影）。清偿记录见下条。
- 2026-09-30（同日）：**R3 评审清偿**。3 路评审（A 官方规格/B 契约 API/C 性能
  v8）：A 7 阻塞、B 零阻塞 7 建议、C 1 阻塞 8 建议。阻塞断言先经主会话实测
  复核（官方常量表在 demo/node_modules @screeps/common@2.15.5 实读取证）：
  ① [阻塞·A] terminal 容量 3000→**300000**、invaderCore decayTicks 50000→
  **75000**（constants.js:332/:849；specs 注释同正）。
  ② [阻塞·A] factory 补 internalBorder 件（rectangle 50² 0x555555，
  factory.metadata.js:161-170——首版漏件）。
  ③ [阻塞·A] factory hasOther 优先级 quirk 保真（:175-176 `energy||power||
  (0<total)` ⇒ total>0 恒 true）——并入 G10 行（:46 与 :176 两处）。
  ④ [阻塞·A] rampart 公有精灵 tint 改 playerColor 色板（rampart.metadata.js:5-7
  userOwnerColor 分支；原误用顶罩的 0x44ff44/0xff4444 非 uOC 分支）——G9 行
  勘误补记。
  ⑤ [阻塞·A] tower NPC 塔无能量条（官方 when: isNotNpc，tower.metadata.js:8,144
  ——values npc→height 0 + apply 双保险）。
  ⑥ [阻塞·A] 掉落资源 lighting 圆改资源色——官方 payload tint:0xFFFFFF 被
  resourceCircle.js:50-56 重建 payload 静默丢弃，运行时实为资源色（quirk
  登记 specs 注释；我方保真运行时行为）。
  ⑦ [阻塞·C] lighting 值驱动成员不置脏→2048² RT 陈旧：reconcileParts 对
  lighting 层成员值变化/隐藏显式 markDirty（官方每帧渲染层的 dirty 门控等价
  物）；防退化配套=radiusLighting 整数量化（唯一连续量，亚像素变化被值门控
  压制，防逐 tick 离屏重渲）。
  ⑧ [建议·A] _isDisabled 公共件扩全表（官方 _all 前置合并全部类型，
  World.js:36-53）；controller enemy 圈双份叠加（官方 :85-116 两份同参
  processor）；controller ring 有效色 0x000000（circle.js 只认 `stroke` 键、
  payload strokeColor 不被读取的官方 quirk，specs 注释登记）；siteProgress
  扇形补同色 1px 描边（siteProgress.js:42）；extension border 档改精确相等
  判定（:37,48,59 无匹配档无边框件；size 阈值判定不变）；spawn badge G9
  随动注释；CONTROLLER_LEVELS 删 index 8（官方表只到 7）。
  ⑨ [建议·B] keys 声明改"审计待官方 metadata 入库后补测"（原注释失实）；
  __room defineProperty 不可枚举+destroy 置空（防 spread 拖树/悬挂引用，R6
  裁定去留不变）；localPlayerId 补 R6 标注；resourcesTotal/store 读数加
  num 守卫（防脏值字符串拼接）；LIGHTING_MEMBER_BLEND 单源 specs；
  rampartGroups 排序 localeCompare→码点比较（夹具确定性）；测试死代码清理。
  ⑩ [建议·C] syncWalls 双引用门控（地形码+extras 引用均未变短路——含
  constructedWall 房间每 tick 零拷贝零 FNV）；destroyView 对 Text 精确销毁
  （style/texture/textureSource——v8 需显式标志，否则等 GCSystem 60s 兜底）；
  renderSize 单源 TERRAIN_SPEC；badge 色单源 SPAWN_BADGE_COLOR；tower 枪体
  Sprite 子件改 Container 包裹（v8 Sprite allowChildren=false deprecation，
  R5 自转旋转容器）；.name→.label 与 getChildByLabel 统一（v8 deprecation
  别名）；'screen' 字面量单源；60fps 帧基准常量 frameBaseMs 单源。
  ⑪ [登记] §2.4"dispose 仅在房卸载"措辞修订：现语义=id 对账复用+成员变更
  才拆树（同类型 detached 池待 R7 perf 实测后定）；apply 不校验 id↔typeKey
  漂移（契约稳定假设，M3 防御点）；顶罩 2048² 全幅 VRAM 按 user 组线性
  （bbox 裁剪留 R7）；resourcesTotal 分母 `||1` 与 SOURCE_ENERGY_SIZE 等就地
  常量的边界口径=公式级常量入 specs、表达式内防御性单位量可就地命名。
  清偿后：122 测绿+tsc+全链回归绿；harness 复验（factory 内衬/条序、
  extension 边框、terminal、nuker、controller 状态圈、showcase 全景）。
  **清偿复审**：28/29 项核销正确；抓到一处清偿引入的真回归——tower gunWrap
  pivot 双重除缩放（wrap scale=1 时 net 偏移应为 world 32 直值，且条几何
  应为官方局部空间×scaleY）——修复（pivot 直值+bar 四参数×scaleY）后定点
  复试 **pass**（123 测绿，量化复核 gun top −32/条满高 59.9/条宽 80.8 全回
  官方值）；另顺手清：lighting 成员位移路径补 markDirty（R4 运动前置防御）、
  rotatable 块注释残留、terminal 300000 常量测试锚。
- 2026-09-30：**R4 creep 与运动完成**。四段落地：**纯逻辑半件**（body-ring.ts
  creepBuildBody.js 逐行移植——过滤 tough/carry/hits≤0、同类型合并、升序、
  非 move 自头顶/move 自正下双向镜像展开、弧长=PART_ANGLE×hits/100，常量
  入 CREEP_SPEC.ring；anim/tweens.ts 官方动效语义的解析式等价——Ease(MoveTo)
  ≡ lerp(start,target,easeInOutQuad(t/T))、RotateTo 最短角线性 0.2 tick、
  FadeOut 线性、罩关键帧 0→0.5@1/4→0@1、TweenManager 按真实帧时间推进
  〔数据断流动画照跑〕、cancel 不 finish 不触发 onEnd）× **声明/pixi 半件**
  （parts-creeps.ts：main 收敛黑圆/body环/徽标/资源圆三层/tough/NPC——
  官方全部是 mainContainer 子件随其旋转；mask/glow/glowNpc lighting 层、
  name effects 层；白圆 when 表达式 || 优先级 quirk 登记 G12；资源圆半径
  cap=0→min 钳 1 官方 Infinity 同语义；powerCreep 全件〔className-lvlN
  纹理 tint 0xcc3d3e、lighting 同纹理、红 glow、texture-light 死代码登记〕；
  joins-creeps.ts 逐段 arc→stroke——v8 GraphicsContext.stroke 每次克隆活动
  路径开新路径已源码取证）× **运动系统**（ObjectsStage 内置 tween 编排：
  位移 easeInOutQuad 插值 tickDuration、朝向最短角 0.2 tick、同向移动不重
  触发〔官方 rotation calc props 门控等价〕、overlay 件〔lighting/effects〕
  逐帧随动+markDirty、EVENT_ATTACK/HEAL→data.targetId→三色罩〔同 tick
  攻+疗=黄〕、死亡 FadeOut(tickDuration/2) 后销毁〔dying 表托管+id 复用
  防御〕）× **事件通道**（FeedStore.onChange 携 FeedChange{objects,time,
  events,kind}——快照 kind 驱动场景全量重建）。**实施决策**：① 快照语义=
  全量重建（跨快照位置跳变不得做补间——切夹具/重放的幻影移动+旋转棘轮
  根因，验收期实测抓出后修复）；② id↔typeKey 漂移防御提前落位（R3 §12 ⑪
  的 M3 防御点——各夹具世界 id 空间独立都从 1 起编，同 id 撞不同类型必须
  拆树重建，实测 storage 部件叠 creep 后修复）；③ creep/powerCreep 同受
  _all 合并（disabled 件追加，World.js:36-53 前置合并语义）；④ PartEnv 增
  controller 摘要（safemode α 判定需跨对象状态，ObjectsStage 每 apply 扫描
  首个 controller〔官方 objects.find 同语义〕）；⑤ tickDurationMs 入
  CreateRendererOptions（G6 契约参数化，R6 评审去留标注）；⑥ 名字沿 R3
  spawn 先例只显示己方；⑦ 罩/淡出对 lighting 成员逐帧 markDirty（RT 陈旧
  防御沿 R3 C-B1 模式）。**夹具**：exporter 新增 creeps（R1 §12 决策①登记的
  本文件扩展点——意图脚本走位×2/近战×2/治疗×2，victimA 第 4 tick 死亡=
  removedIds→淡出取材、victimB 攻+疗同 tick=黄罩取材、tank 纯绿罩；意图对
  死亡目标续发由结算链静默拒；Rust 测试断言位移/死亡/事件面，两次导出
  逐字节一致）；showcase 扩 creep 形态谱系 15 件（50 部件满环×2/混色/掉血/
  tough 存活+全灭/NPC/孵化/资源圆三层/敌我 powerCreep/safemode 联动——
  (5,5) controller safeMode 1000：状态圈转橙+p2 creep α0.5 为官方行为）。
  **验收**：179 测绿（body-ring 14/tweens 17/creep-values 18/motion 对账 6/
  既有 124）+tsc+`pnpm -r run test` 全链绿+kk-cli cargo test/clippy 零告警；
  harness 截图逐项核对（环谱系/资源圆三层/powerCreep 红 glow/safemode 变暗/
  三色罩/淡出两帧递减/插值中段位置+move 弧随向旋转〔连拍取证〕），WebGPU+
  WebGL 双后端 `lighting: real` 56-63fps。清偿记录见下条。
- 2026-09-30（同日）：**R4 评审清偿**。3 路评审（A 官方规格/B 契约 API/
  C 性能 v8）：A 3 阻塞+5 建议、B 1 阻塞+7 建议、C 零阻塞+6 建议+R7 测量
  点清单。逐条处置：
  ① [阻塞·A-B1/B建议3] 受击罩类型门——官方 cover 出自 creepActions，仅
  creep/powerCreep/tower/lab/link/invaderCore 声明；我方原对任意 view 命中
  即挂罩（rampart 承伤会红闪）。修：applyCoverEvents 按 view.motion 门控
  （R5 光束落位时随表扩）。
  ② [阻塞·A-B2] 罩时序 ×0.9——官方三处 call-site（creepActions.js:300/
  303/305）传 tickDuration*0.9，峰值 0.225td、总长 0.9td；〔官拆〕§3 的
  "1/4 tick" 系漏看 call-site 系数。修：CREEP_SPEC.cover.durationScale=0.9
  +triggerCover 时长乘子+specs 注释勘误+测试锚（225/675ms）。
  ③ [阻塞·A-B3] safemode α 传导——官方 mask/glow/NPC glow/childCover 均
  是 mainContainer 子件随其 α 暗化；我方独立 overlay 恒 α1。修：mask/glow/
  glowNpc/powerCreep light+glow 的 values 带 alpha 键（safeModeAlpha 值），
  join apply 写 holder.alpha；罩以 main 当前 α 作乘数。
  ④ [阻塞·B-B1] powerCreep light 件永不可见——值面发 `visible: 1`
  （number）而 vBool 判 `=== true`，join 层恒隐藏且被测试锚固化。修：值面
  改 boolean（全库 'visible' 键约定）+锚更正+join 层冒烟断言（motion 测试
  powerCreep light/glow visible=true——补 join 半件无直接测试的盲区）。
  ⑤ [建议·A-S1] 运动重触发引证更正并登记偏差：官方 metadata.actions 重
  触发=finishActions 吸附旧终点再起新段（GameObject.js:139-146）；原引证
  moveTo.js:38-41 系无 metadata 引用的死 processor。我方 cancel 保当前位
  平滑续走，登记为有意偏差（影响仅限动画中途新位移）——tweens.ts 头注+
  beginMove 注释+本条。
  ⑥ [建议·A-S2] 官方运行时 z 序振荡 quirk 登记：creepBuildBody/circle 重
  跑 destroy+尾部 addChild（object.js:39-43,112-114）——掉血后弧环+tough
  跳到资源圆上、store 变化翻回；我方固定声明序=官方"掉血后"稳态——joins
  头注登记。
  ⑦ [建议·A-S3] 罩挂点/重触发差异登记：官方 childCover 挂 mainContainer
  （随转）+重触发叠加不取消（creepActions.js:80-83 TODO）；我方 effects 层
  独立节点（cover.svg 纯圆→不随转零观感）+重触发取消——triggerCover 注释。
  ⑧ [建议·B] EVENT 常量落点登记（shared 生成管线落地前的本地替身，R5 随
  管线处置）；Rust victimA 断言化简（去恒真冗余）；setTerrain kind='delta'
  语义注释；tickDurationMS 缺省 1000 单源 DEFAULT_TICK_DURATION_MS；
  controllerSummary 调用点成本注释；FeedChange.objects 拷贝语义注释。
  ⑨ [建议·C] TweenManager.update 免快照迭代（Map 迭代容忍迭代中 delete+
  守卫，省每帧 ~100 项数组分配）；resolveTypeKey 微缓存（(type,structureType)
  键空间有限，消每 tick 每 structure 拼串）；RoomScene.update 换序（objects
  先于 lighting——运动置脏同帧进 RT，消一帧滞后）；TweenManager cancel 清
  pending（防补录复活）；arc→stroke 的 getLastPoint NaN 前导 quirk 注释
  （实测 benign 勿"修复"）。
  ⑩ [不采纳·C建议3] syncRoad 键串每 tick 拼接——量级小（路格数少且 hash
  门控在外层），维持现状。
  ⑪ R7 perf 测量点清单（评审 C 8 条：lighting RT 重渲耗时/dirty 比例、
  apply 对账分项、双后端帧率基线含 50 creep 全移对比、TweenManager 规模、
  快照重建单帧尖峰、值门控触发计数、10k tick 内存曲线含 dying/overlays、
  大步进 deltaMS 恢复）——随 R5 记录移交。
  清偿后：180 测绿（+1 powerCreep light 冒烟）+tsc+全链回归绿+kk-cli cargo
  test/clippy 零告警。清偿复审见下条。
- 2026-09-30（同日）：**R4 清偿复审（二轮）**。评审 A 回复审：B1/B2 核销、
  S1-S4 登记全核销、B/C 并行项全核销无回归；B3 核销但抓到清偿引入的轻量
  新回归——**淡出×safemode 亮度跳变**（beginFade 对 overlay 绝对写 α，敌方
  safemode 视图〔mask/glow 已暗化至 0.5〕死亡首帧跳亮回 ≈1 再衰减；官方=
  层级 α 连乘）。二轮清偿：① fade setter 改**捕捉各 overlay 底 α 作乘法**
  （root 恒 1 绝对写——官方 FadeOut 同位），罩 tween 于淡出开始时取消（一并
  消 coverTween/fadeTween 对 cover.alpha 的末写者冲突，罩冻结当前 α 随淡出
  衰减）；② 补评审 A 要求的两条回归锚——罩类型门（constructedWall 承伤
  不挂罩）与 safemode 死亡不跳亮（motion 测试 +2）；③ 底账勘误随手清——
  〔官拆〕§3 罩时序（0.225td 达峰/0.9td 总长）与 §7 重触发（finishActions）
  两处旧文更正，防后续评审再被绊。清偿后 182 测绿+tsc。
- 2026-09-30（同日）：**R4 清偿复审（C 路）+ 尾清**。评审 C 复审：五条采纳
  项+一条不采纳登记全核销、A/B 清偿性能面无新影响（特别论证 safemode α
  传导被值门控拦截——常态零 pixi 写，边界跨越 tick 恰 1 次 RT 重渲）、R7
  移交完整、**零新阻塞**；两条建议级遗留就地清：① resolveTypeKey 缓存键
  改用 state 自有 structureType 串（首版 `s|${x}` 模板串作键仍逐次分配，
  未达成宣称——二轮更正后命中路径零分配）；② 补「迭代内 run→同帧 cancel
  →不经 pending 复活」测试锚（tweens.test +1）。B 路复审因提供方配额受限
  两次中断、重发中；其阻塞项 B-B1 的修复已经 A（"顺带修了 visible:1→
  boolean"）与 C（"永隐藏真 bug 已修为 boolean+冒烟锚，处置正确"）两路
  复审交叉核销。清偿后 183 测绿+tsc+全链回归绿。
- 2026-09-30（同日）：**R4 清偿复审（B 路，第三次重发后返回）**：B-B1 及
  七条建议全部核销、清偿未引入契约/公共面回归、无新阻塞——B 路复审通过
  （冒烟断言经真实全链驱动、旧缺陷在此必挂的承重锚认定；alpha 键扩散的
  值域纯净性确认；淡出乘法化/微缓存的契约面无影响确认）。**三路清偿复审
  全部通过，R4 收口。**
- 2026-09-30（同日）：**R4 后稳定性修复与架构整理**（R5 尚未开始）。独立审查
  确认的 7 项问题已修复：① renderer 销毁时释放独立纹理/源/位图，spritesheet
  页改按 URL 租约共享、最后一个持有者卸载；并发加载失败等待在途任务落定再
  回滚，初始化失败与重复 destroy 共用释放路径；② lighting 显式 render Matrix
  加半格平移，使 RT 原点对应世界 (-cell/2,-cell/2)，不修改 content 局部变换；
  ③ 动画输入改用未经 minFPS 截断的 ticker.elapsedMS；④ 部件 null 停用使 prev
  失效，恢复时重新 apply 与置脏 lighting；⑤ link 能量按 50px×比例设尺寸，
  不再覆盖原始纹理尺寸归一化；⑥ ResizeObserver 观察容器、renderer resize
  同步相机钳边/投影，拾取使用同一视口；⑦ 空墙/空沼泽同样先做 hash 门控，
  防等价空地形重复清场/RT 重渲。
  架构整理：`OverlayGroup` 集中位移/淡出/lighting dirty；快照清除瞬态而复用
  同 id/type 节点（修订 R4「快照全量重建」实施决策，保留位置即时跳转语义）；
  `AnimationClock` 统一场景时间，公共面增加 `setPlaybackRate(rate)`（0 暂停、
  默认 1、R6 冻结时评审），harness 倍率同步作用于推送和动画。顺带修复罩动画
  中途 safemode 改变时仍使用触发时透明度的问题。销毁同时取消 feed 订阅，
  防已销毁场景被保留句柄重新触达。
  验证：新增 23 项回归（共享页双实例/卸载中重挂载/失败回滚/重复销毁、公共
  ticker 长帧与变速暂停、坐标边缘对齐、resize 钳边与拾取、隐藏恢复、快照
  节点身份/瞬态清理/类型变化、跨层随动与 safemode），renderer 206 测+tsc+
  资产新鲜度绿，shared 4 测绿，Vite harness 生产构建通过。WebGPU/WebGL
  人工验证 showcase、地形切换、快照重放，WebGPU creep 4× 回放到末态，
  900×600 视口保持缩放/世界锚点，双后端 lighting: real、无 error 日志。
  地形切换仍有 Pixi BindGroup「纹理销毁时仍被 shader 绑定」warning，未观察
  到渲染中断，保留 R7 跟踪；未执行性能预算/10k tick 内存 soak，不据此声称
  R7 验收完成。本次为单代理修复验证，未补做三路 subagent 评审。

- 2026-09-30：**R5 动效系统实现与验证完成（含灰区）**。输入贯通 events/actionLogs/says，事件及子类型常量统一到 shared 并对 Rust 数值校验；纯解析层维护目标前态/显式坐标/controller 唯一目标及双通道去重。独立 EffectsStage/AmbientScheduler 在单一动画时钟上实现动作全家族、常驻、observer/deposit、填充补间、say 和 nuke；Position/PropertyChannels 避免位移和 alpha 抢写，房间级效果完成回收、快照取消、跨层 dirty 和私有文字精确释放。
  真实 actions 夹具增加六类事件及工地完工边界；effects/lifecycle/pressure 显式合成。发现本分支 rangedAttack/rangedMassAttack 没有结算处理器，登记 G5，未扩大修改引擎。验证修复：缺帧 Container 被按 Sprite 更新的崩溃；lighting EMPTY→RT 后实际输出尺寸失控（v8 setSize 不保留纹理更换前目标尺寸）；ADD/EaseOutQuad/reaction glow/say 轮廓与隐私；harness seek 历史动作叠积。
  renderer 256 测+shared 4 测+exporter 4 测、类型检查/新鲜度/确定性重生/构建/文档检查通过。WebGPU/WebGL 固定动作关键帧与回收、核爆、长帧、回放倍率及压力重播验证；官方 UMD 同数据参考对照。1080p/M4/50 creep/4× 的 objects CPU p95 为 0.5/0.6ms，lighting 提交 p95 两后端 0.5ms；非 GPU 完整预算，R7 未完成。BindGroup warning 创建栈指向 Pixi 批次纹理缓存，资源销毁会清空绑定，无自有悬挂引用证据，保留 R7。单代理完成三方面复核，未执行三路 subagent 评审。详细方法、证据和近似见验证记录；下一阶段 R6。

- 2026-09-30：**R5 三路评审与清偿收口**。主会话三路 subagent 补齐 §7 收口评审（官方规格保真/契约与生命周期/测试夹具与验证报告实测复核）：自动检查全部复跑通过（27 生成文件重生哈希一致、artifact 46 件与 HUD 采样对上、压力数字逐字一致），R4 语义零回归（motion 门控收窄为 HIT_COVER_TYPES 六类=官方 creepActions 声明域）。**阻塞 2 项清偿**：tower 治疗事件（rangedHeal）补瞄准/flare/光照脉冲（一行修+测试）；验证记录 PWR 16–18 表述勘误（实测可渲染，G5 行同步订正）。**建议落地 8 项**：invaderCore TTL 环去闪烁（官方仅 constructionSite 有 blinking）、actionLog 通道补 previous 回退、私有 say 显式判空、bite 缺失计 skipped、诊断统计 privateTexts→texts、rotateTween 补 onEnd、exporter 确定性测试对齐 30 tick 入库全长、HUD 增 canvas 分辨率行。**差异登记 5 项**：flare3（_all hasEffect power 顶罩）未实现、孵化 once 语义有意偏离官方 quirk、填充补间白名单宽于官方、say 无像素对齐、目标侧工作罩 owner=来源的取消语义。**主会话逐行复审清偿增量**：发现并修复清偿引入的一处隐私回归（player=null 即 `localPlayerId` 缺省时私有 say 被错误放行，已改回全滤并加测试锁定，详见验证记录「复审与返工」）。**用户实测再发现层序偏差并修复**：名字 overlay 盖住 say 气泡与官方相反（官方 effects 层插入序=气泡盖名字）——apply 末尾重挂 finite-effects 根到顶，层序测试+stash 负向验证+harness 截图确认（详见验证记录「层序修复」）。终态 renderer 261 测+kk-cli 7 测+fmt/clippy/文档链接绿；R5 关账，下一阶段 R6。
- 2026-09-30：**R6 RoomVisual+交互+API 冻结完成（含两轮评审与外部复审仲裁）**。
  实施方案 [f2-renderer-r6.md](f2-renderer-r6.md)（§7 决策点 D1–D7 用户拍板全按
  建议）。实现 `a7af796`：五原语渲染（shared VisualStyle 官方名 1:1+isRoomVisual
  守卫、纯逻辑 visual-style/pixi visual-stage 两半件、同引用短路全量重建、Text
  四标志精确销毁）；FeedStore.setVisuals 补通知+FeedChange.visuals 原引用+快照
  清空（D2）+FeedView.visuals/roomNames；活动房间 ActiveRoomController（D3，非
  活动房零渲染写入/切房快照语义重建/离房事件不补播）+harness ?multi=1 八房常驻
  双回放；拾取/悬停（D4，PickResult objectId 数据面命中 z 序优先、onPick/onHover
  多订阅、高亮环）；fontsBaseUrl 库内 FontFace 预载+getStats().fonts 三态（D5，
  G7 字体责任落库）；__room 移除+advanceAnimation/onFrameTiming（D6）；vite lib
  ESM+tsc d.ts+「公共面 import 闭包零 pixi」守卫（D7）。三路评审（A 官方规格/
  B 契约生命周期/C 测试实测）清偿 `4aa2a81`：1 阻塞（dash 间隙宽度感知——round
  端帽默认宽粘连、夹具假阴性）+18 建议全清，二轮复审 13/13 核销通过（守卫盲区
  三处+A-J4 锚记 R7 加固）。**外部复审（GPT）仲裁清偿 `83088f2`**：官方默认值
  取证对象错误成立——docs.screeps.com 整页锚点抓取四次（主会话×2/评审 A/C 各
  ×1）集体命中同页 Game.map.visual 表（radius 10/strokeWidth 0.5），权威源
  screeps/docs 仓库 api/source/RoomVisual.md 实为 radius 0.15/strokeWidth
  0.1/text font 分形态/padding 0.3/opacity 1.0（MapVisual.md 考古坐实来源；
  **纪律沉淀：官方文档取证走 GitHub 仓库源，不用整页站点锚点抓取**——r6 方案
  §3.1 勘误注）。同批清偿：基础 web 颜色（命名色 148 键/rgb(a)/
  hsl(a)）、poly 折线开放描边、文字垂直锚官方明载（背景 middle/默认基线，底边
  近似登记 G1）、有限几何双防御（MAX_DASH_SEGMENTS=4096——实测 hypot(1e308,
  1e308) 有限致切段 OOM 比报告所指死循环更深）、悬停失效机制（数据/切房/相机
  变化重估）、指针手势纪律（pointerId/最大位移/lostpointercapture）、fonts 永
  不 reject、零推进真 no-op+计时每帧刷新、PickResult.room/readonly/./assets/*
  导出/回调政策 TSDoc；报告三项与首轮清偿重叠（已在 4aa2a81）。R5 移交契约语
  义（目标罩 owner=来源取消、visuals 全量替换、离房不补播、say 隐私等）随冻结
  面落 d.ts TSDoc。**灰区表更新：G1 落地收窄+G7 字体责任+G13 新增（高亮环）+
  G6 冻结**。验证终态：renderer 322 测（+61）+shared 4+kk-cli 7 绿；tsc/闭包
  守卫构建（公共面 7 文件零 pixi）/文档链接/资产重生哈希一致/两包构建；浏览器
  双后端 53 条用例新默认值观感逐原语核对、多房 Tab 隔离（ambient 随房 13→3）、
  悬停/拾取/advanceAnimation 实测；R7 性能预算/内存 soak 未做不宣称。下一片
  R7（性能与收口）。

- 2026-09-30：**R6 完整二次外部复审清偿**。上一轮默认值仲裁成立，双后端
  主用例观感通过；后续定向复审发现 B1–B5 边界残留：CSS 数字/角度 token、
  离开与拖拽状态的悬停重估、相机多指/capture 状态、文字描边 alpha、字体
  部分失败重试资源累积。本轮逐项修复并补行为回归；详见 r6 方案 §8 末项。
  原增量复审 PASS 不覆盖这些边界，收口须以本轮验证结果为准。


- 2026-10-01：**R7阶段修复与交接，尚未收口**。已批准方案`b0a5dd1`，按用户要求阶段提交
  `821a02f`保存工具/首轮数据/真实Graphics失败回归；随后工作树修复自有context释放、
  有实测依据的rampart bbox裁剪（32个raster用例零差异；纹素节省，不宣称GPU帧提速），renderer362/shared4/kk-cli7与构建守卫通过。
  WebGPU标准10k完整执行，但heap终点较起点+14.97MiB尚未证无增长；首轮GPUpass跨度p95=13.6315ms仍未销账≤8ms。
  用户已批准冻结阈值下如实登记差距与后续选项，并确认G1–G13整表终裁。
  旧交接时三轮矩阵/正式WebGL/压力10k/八点齐数/三路评审/终态文档及收口提交仍需完成；本次阶段评审与文档更新见后续条目，
  见 [R7验证记录](../research/notes/renderer-r7-verification-2026-09-30.md)、[灰区确认](../research/notes/renderer-r7-gray-decisions-2026-09-30.md)
  与 [交接提示词](f2-renderer-r7-handoff-2026-10-01.md)。正常lighting和R6冻结公共API不变；主树/引擎未改。

### §12 R7追加执行记录：2026-10-01短测与真实回放阶段

后续放大黑屏修复（2026-10-01）：v8自动沼泽精灵mask关闭viewport裁剪，放大后全房滤波输入超出设备纹理限额。
现以自有MaskFilter开启视口裁剪并显式释放；实际双后端短GPU各70案例、原始RGBA对照与三次重建检查通过，
详见[缩放修复记录](../research/notes/renderer-zoom-verification-2026-10-01.md)。renderer370/shared4/kk-cli7，
不改冻结公共面、normal光照、灰区终裁或长测暂缓状态。

用户新授权暂缓长时间测试、不再等待方向询问；开展短测驱动修复及官方真实场景。
完成Graphics context释放、顶罩bbox及同组source复用、同tick对象对账门控、
官方null动作哨兵诊断修正；E54S13本地官方回放100tick双后端跑通、独立oracle匹配。
renderer369/shared4/kk-cli7测、两包tsc/构建、fmt/clippy、175资产夹具再生一致，
三路实际评审清偿复审。环境/数值/截图/方法详见[短测报告](../research/notes/renderer-r7-short-verification-2026-10-01.md)。
R7为阶段提交，§5冻结阈值不变，快照仍有≥2ms、GPU≤8ms未新销账，
缓存槽增长与长期稳态仍未解决；不勾选最终收口，不以真实场景替代300/50/100验收。
既有用户确认灰区表终态继续有效，未新增装饰/world/API/引擎改动。

2026-10-01 接续：黑屏修复提交`e7e30db`后，按短数据优化RoomVisual槽位与自有两份绝对坐标
几何缓存、文字背景门控；保留失败/无收益尝试数据、双后端限定场景RGBA零差异。
perf-only checkpoint计数改为采样拷贝，补足短测与重建GPU完成上界。
过程、差距、生命周期与评审见[RoomVisual短测复核](../research/notes/renderer-visual-reuse-verification-2026-10-01.md)。
§5门限和G1–G13既定确认不改，正式矩阵/ticker长帧/10k与最终R7收口仍待办。
