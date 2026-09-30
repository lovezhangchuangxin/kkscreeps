# 官方渲染器（screeps/renderer）逐文件拆解笔记

> 取证源：本地克隆 `/Users/keqing/Desktop/projects/sources/screeps/renderer`
> （HEAD `a2db4a7`，PixiJS 7.4.2 + @pixi/layers 2.0，见 `engine/package.json`）。
> 本文是 F2 渲染器批（[方案](../../plans/f2-renderer.md)）"视觉对官方"的参照规格底账：
> 每条结论附 renderer 根相对路径证据。关键常量已经主会话抽查复核（body 环
> 常量/分层表/ISC 许可逐字比对通过）。2026-09-30。

## 0. 架构与「数据驱动」机制

`GameRenderer.applyState(state, tickDuration)` → `World` 跑全局 preprocessor
（`setBadgeUrls`、`terrain`）→ 按 `_id`（缺省 `` `${room}:${type}:${x}:${y}` ``）
diff 增删，每对象一个 `GameObject`/root `Container` → 按 metadata 跑
`calculations → actions → processors`（`engine/src/lib/World.js:134-182`、
`engine/src/lib/GameObject.js:72-202`）。

metadata 每对象一份 `metadata/src/objects/<type>.metadata.js`，四类声明：

1. `calculations`：派生值（可声明 `props: ['store']`，仅依赖变化时重算，结果进 `calcs` 缓存）；
2. `processors`：视觉部件，`type` 选处理器（`sprite/circle/draw/text/container/creepBuildBody/...`），`payload` 传参，`when/until/once/props` 管生命周期；
3. `actions`：补间动画声明（`ScaleTo/AlphaTo/RotateTo/Repeat/Sequence...`），参数可以是表达式；
4. `data`：首帧一次性赋值（如坐标 `x: {$state:'x', koef: CELL_SIZE}`）。

表达式引擎：任何 `$` 开头的键都是表达式——`$state/$calc/$rel/$processorParam/$random`
加算术逻辑原语（`engine/src/lib/expressions/`，`GameRenderer.compileMetadata`
一次性预编译为闭包，`engine/src/lib/GameRenderer.js:15-51`）。90% 视觉规格是
纯数据（私服图形 mod 的基础）。

## 1. 美术资产与许可

- `metadata/images/`：126 文件 = 117 SVG + 9 PNG。PNG 全是程序性底纹
  （`glow.png`、`flare1-3.png`、`noise1/2.png`、`ground.png`、`ground-mask.png`、
  `creep-mask.png`）。
- **许可（LICENSE.txt）**：ISC，Copyright (c) 2016 Artem Chivchalov
  <contact@screeps.com>。商用/修改/再分发均允许，保留版权与许可声明即可。
- 命名规律（权威映射 `demo/src/config/resourceMap.js`）：`<structure>.svg` +
  `-border`（玩家色描边）/`-highlight`（cooldown 闪显）/`-lvl0..N`/
  `-rotatable`/`-npc`。
- creep body **无部件贴图**（Graphics.arc 程序绘制）；仅 `tough.svg`（中心叠层）、
  `creep-npc.svg`、`bodyPartBar.svg`（备用）。spawn 是纯 Graphics 圆+徽标，无贴图。
  terrain **无贴图**（程序生成，§6）；出口 `exit-top/bottom/left/right.svg`。
  进度条/资源条/说话气泡全部程序绘制；`rectangle.svg` 白色万能矩形靠 tint。

## 2. 图层与 z 序（最易做错：rampart 是顶层罩）

全局 5 层（底→顶，`metadata/src/index.js:52-130`）：

```
terrain → wallGraffiti → objects(isDefault) → lighting → effects
```

lighting 层挂 MULTIPLY 的 PassThroughFilter + 0x808080 环境光矩形，display
事件把成员改 SCREEN 混合（`metadata/src/index.js:92-127`）——官方"光照感"的
全部秘密。子节点可 `parentLayer` 路由到其他全局层（打破树形 z）。

**objects 层内 zIndex 全表**（各 `metadata/src/objects/<type>.metadata.js` 顶层
`zIndex`；落到 rootContainer，`GameObject.js:28`）：

| z | 对象 | z | 对象 |
|---|---|---|---|
| -1 | road | 8 | spawn |
| 0 | rampart(public 叠加)、constructedWall、extractor | 9 | link |
| 1 | energy、deposit | 10 | observer |
| 2 | source、mineral | 11 | powerBank |
| 3 | portal、keeperLair | 12 | powerSpawn |
| 4 | controller、container | 13 | tower、powerCreep |
| 5 | tombstone、ruin | 15 | lab |
| 6 | **creep** | 16 | terminal |
| 7 | extension、factory、storage、nuker | 17 | invaderCore / 18 | constructionSite |

- **私有 rampart 由 terrain preprocessor 画成整片色区，路由 effects 层
  alpha 0.4 ADD**（`engine/src/lib/processors/terrain.js:216-221`）——半透明色罩
  永远盖在 creep/structure 上；public rampart 用 `rampart.svg` 同在 effects 层
  （`metadata/src/objects/rampart.metadata.js:11-23`）。颜色：own
  `#105010/#44ff44`、enemy `#501010/#ff4444`；否则用户色×0.3 填充+用户色描边 25。
- road 是 objects 层内最低（-1），按邻接连通合并绘制
  （`engine/src/lib/processors/road.js` 的 mm/zm/pm/mz 判断）。
- **全部文字在 effects 层**：名字（`metadata/src/action-templates/displayName.js:4`）、
  say 气泡、旗帜名、mineral 化学符号。
- 同对象内 processor 未指定 zIndex 时按声明序 0,1,2…（`GameObject.js:30-35`）。

## 3. creep 渲染规则

- 坐标：world = `x*CELL_SIZE`（`helpers/mathHelper.js:14`），stage
  `pivot=-CELL_SIZE/2` 补偿到格心（`World.js:66-67`）。
- 自内向外：黑圆 r50 → body 弧环 → 黑圆 r32 → 徽标 r26 → 资源圆；lighting 层
  `creep-mask.png`(100) + `glow`(400, α0.2)（`metadata/src/objects/creep.metadata.js:88-226`）。
- **body 环算法**（`engine/src/lib/processors/creepBuildBody.js`，已复核）：
  `maxParts=50; ANGLE_SHIFT=-π/2; PART_ANGLE=(2π)/50/2; RADIUS=50; LINE_WIDTH=18`；
  过滤 tough/carry/hits≤0，**同类型合并 hits**，合并后按 hits **升序**（BODY_PARTS
  的 order 字段是死代码）；非 move 从头顶向两侧对称展开，move（`backSide:true`）
  从正下对称展开；每部件弧长 = `PART_ANGLE * hits/100`。掉血=弧变短、死亡=弧消失
  （无 tint 无闪烁）。色表：move `0xaab7c5`、work `0xfde574`、attack `0xf72e41`、
  ranged `0x7fa7e5`、heal `0x56cf5e`、claim `0xb99cfb`、carry `0x666666`（不画）。
  任一 tough 存活 → 中心 `tough.svg` 120×120；NPC（user '2'/'3'）整只换 `creep-npc.svg`。
- 朝向：`RotateTo(移动方向, tickDuration*0.2)`。受击：`actionLog.attacked/healed`
  → `cover.svg` ADD 罩 tint 红 `0xFF3333`/绿 `0x2ce328`/黄 `0xFFFF33`，0.225 tick
  达峰（α0.5）、总长 0.9 tick（call-site 传 `tickDuration*0.9`，函数内再 /4、
  3/4——`creepActions.js:149-171,299-306`；本行旧文 "0.25/0.75 tick" 系漏看
  call-site 系数，2026-09-30 R4 评审 A-B2 勘误）。
  死亡：`FadeOut(tickDuration/2)` 后销毁（`engine/src/lib/processors/disappear.js`）。
  敌方 safemode 期间整只 alpha 0.5（`creep.metadata.js:49-62`）。
- say 气泡（`engine/src/lib/processors/say.js`）：圆角矩形 r30+三角尾、黑描边 8px；
  底色私有 `0xcccccc`/公开 `0xdd8888`；Roboto 60px `#111`；宽=文字宽+60、高 100、
  y=-170；effects 层；时长 = actionLog.say 存在的 1 tick。
- 携带资源：中心圆 r=`min(1,total/capacity)*20`，白/红 `0xf41f33`(power)/黄
  `0xffe56d`(energy) 分层。名字：effects 层 Roboto 55px、玩家色、stroke `#111`
  15px、anchor y=1.7。

## 4. 动效清单（全由 actionLog 驱动）

色表（`creepActions.js:13-24`）：default 黄 `0xffe533`、attack 红、heal 绿
`0x2ce328`、rangedAttack 蓝 `0x3c75c7`、reserve 紫 `0xb99cfb`、power 暗红 `0xCC3D3E`。

| 动作 | 触发键 | 形态 |
|---|---|---|
| 近战 attack | `attack` | 突进咬合：向目标移 10px，1/4 tick 去 3/4 tick 回 |
| rangedAttack | `rangedAttack` | 双层光束（宽 12+12 blur5 ADD），头部行半程→尾部追上，0.6 tick |
| rangedMassAttack | `rangedMassAttack` | 圆 r300 `0x5d80b2` scale0→1+α0.4→0，0.6 tick |
| heal / rangedHeal | `heal`/`rangedHeal` | 近战 bite / 绿光束；目标侧绿 cover 罩 |
| tower 攻/疗/修 | tower 的 `attack/heal/repair` | 粗光束 18+blur5；炮塔 RotateTo(角,0.3s)+flare1；**空闲炮塔持续自转**（`tower.metadata.js:121-141`） |
| build/upgrade/repair | 同名键 | 黄光束+目标格黄 cover（延迟 0.3 tick）；dismantle 无专属动画（仅红罩） |
| harvest | `harvest` | bite+黄 cover |
| spawn 孵化 | `spawning{spawnTime,needTime}` | 进度弧 r50 线宽 10 从 -π/2 顺时针填满+呼吸缩放 1↔1.25（`spawn.metadata.js:32-38,136-164`） |
| transferEnergy | link 的 `transferEnergy` | 黄粗光束 |
| lab 反应 | `runReaction/reverseReaction` | 白细光束×2 双向+反应光（`lab.metadata.js:167-234`） |
| reserve/claim | `reserveController` | 紫 cover 罩 controller |
| power 技能 | `power` | 暗红光束+技能图标罩（`powerInfluence.js`） |
| controller 降级 | `downgradeTime` | 红 200×200 罩按剩余比例闪现（0.2/0.8 tick） |
| safeMode | `safeMode>gameTime` | 橙 `0xffd180` 呼吸圆 r110+glow 扩散波 |
| nuke 未落地 | `landTime` | `nuke.svg` 呼吸+红圆 r110+落点脉冲圈（r600 加速）|
| nuke 落地 | `landTime<=gameTime` | 全屏红矩形 α0.7→0（1 tick）+红 glow scale→20（`nuke.metadata.js:198-233`） |
| portal/keeperLair | 常驻 | 双层呼吸圆循环+光照脉动 |

## 5. 进度条/徽标/数字（关键裁决：无血条、无浮动数字）

- constructionSite：扇形饼（r20 线宽 10 环+扇形填充），玩家色，0.8↔0.3 闪烁，
  无幽灵轮廓（`constructionSite.metadata.js:11-23`、`siteProgress.js:38-48`）。
- controller：8 扇区等级环+白扇形进度（r37）+中央徽标+状态圈（reservation 绿/
  敌方或 upgradeBlocked 红/safemode 橙，r110 α0.05 闪烁）。
- **structure 血条不存在**；全仓仅两处用 hits：body 弧长、constructedWall
  `hits===1` 红闪。
- cooldown：lab/factory=`-highlight` 闪（0.2/0.8 tick）；terminal=arrows 变暗
  α0.1；extractor=自转 4s/圈。
- 填充量（全部平滑补间 over tickDuration）：extension 中心黄圆 r=size*0.32；
  link `link-energy.svg` 容器缩放；storage 竖直三色堆叠条（高 140 宽 110）；tower
  炮管竖条（宽 90 满高 66.7 随炮转）；nuker 三角+G 条；powerSpawn power 弧+energy
  圆；source 中心黄圆角方块∝energy+边框 tint 呼吸。
- **伤害/治疗浮动数字：确认不存在**（全仓文本处理器仅 displayName/say/旗帜名/
  mineral 符号四类，已 grep 全量验证）。
- 徽标：远程拉 `https://screeps.com/api/user/badge-svg?username=…`
  （`engine/src/lib/processors/setBadgeUrls.js`），r=26(creep)/38(spawn)，失败退化
  纯色圆。

## 6. terrain 渲染（path 合并 + 烘焙，非逐格贴图）

`engine/src/lib/processors/terrain.js` + `engine/src/lib/pathHelper.js`：

- wall/swamp/rampart 各生成一条**合并 SVG path**（连通格轮廓合并、外凸直角
  r=50 圆角），编码成 data-URI 纹理；path 仅 md5 变化时重建（`pathHelper.js:48-60`）。
- plain：`0x555555` 纯色矩形+`ground.png` 平铺(scale3,α0.3)+`ground-mask.png`
  (MULTIPLY,scale7,α0.15)。
- swamp：path 填 `#4a501e` α0.4+两层 `noise2.png` 平铺（tint `0x66FF00`、ADD、
  scale 10/14、**反向滚动**、mask 到沼泽形状）。
- wall：path 填 `#111` 描边 `#000` 宽 10 → **烘焙进 2048² RenderTexture** +
  `noise1.png` bump(ADD 0.2)；另烘 blur 黑影投 lighting 层 MULTIPLY。
- 出口：`exit-*.svg` TilingSprite 贴四边（ADD α0.5）。

## 7. 移动插值

creep：`MoveTo` + EASE_IN_OUT_QUAD，时长 = `tickDuration`（demo 慢放 2.5s，
`demo/src/components/Canvas.jsx:9`）；位移重触发走 `finishActions` 吸附旧终点
再起新段（`GameObject.js:139-146`、`World.js:322-327`——本行旧文 "cancel
（moveTo.js:38-41）" 系无 metadata 引用的死 processor，2026-09-30 R4 评审
A-S1 勘误）；朝向旋转 0.2 tick。动画按**真实帧时间**更新（`ActionManager.update` 用
`Date.now()` 差值），tick 断流动画照跑；500ms 渲染看门狗（`GameRenderer.js:159-164`）。

## 8. RoomVisual：未能取证

本仓库无任何 RoomVisual 渲染代码（engine/metadata/demo 全量 grep + `main.d.ts`
公开 API 均无）——由官方闭源客户端壳绘制。我方只能黑盒对齐官方行为文档/截图。
扩展点仅 mods decorations（`engine/src/lib/decorations.js`）与自定义 metadata。

## 9. 墙/rampart 受损分级：不存在分段贴图

唯二表达：`constructedWall hits===1 && hitsMax>1` → effects 层红方块 0↔0.2 闪烁
（周期 1.5s，`constructedWall.metadata.js:19-36`）；rampart 无受损视觉。另有
`_isDisabled` 同款红闪（`_all.metadata.js:79-95`）。

## 10. 性能组织（官方的坑与亮点）

- 每游戏对象一个 root Container 直挂 stage；z 序靠 pixi-layers `parentLayer`
  全局层路由；每帧 Layer 排序。
- **无对象池**：processor 重跑即 destroy 重建；文本也如此。
- 光束逐帧 `Graphics.clear()+lineTo` 重画；无粒子批。
- 文本：`PIXI.Text` 每个挂 `Ticker.shared` 回调做反缩放+`getBounds()` 像素对齐
  （`text.js:44-53`、`actionHelper.js:103-123`）——明确性能坑。
- 差量更新：`props` 声明+`_.isEqual` 深比较（`GameObject.js:204-213`）。
- 无限动画（炮塔自转/source 呼吸/portal）永不休眠。
- 亮点：地形 path→SVG→RenderTexture 一次性烘焙+md5 门控。

## 11. 对我方实现的启示

- **可机械复刻**：zIndex 全表与 5 层结构、body 环常量与算法、动效时序分数与
  色表、进度条全部规格、"无血条无浮动数字"裁决、terrain path 合并、road 邻接
  连通画法、文字字体常量（Roboto 55/60px、`#111` 描边 15px）。
- **值得借鉴**：metadata 表驱动+预编译（对应我方"渲染规格静态表"）、props
  声明式差量、层路由思想（对象的光照/特效脱离本体层级）、md5 门控重烘焙。
- **要避开的坑**：destroy/rebuild 风暴（池化）、每文本全局 ticker+每帧
  getBounds（事件驱动）、光束逐帧重画（预生成可拉伸束贴图）、无限动画不暂停、
  运行时生成 SVG data-URI（启动烘焙缓存）。

## R5 实施复核补记（2026-09-30）

官方 a2db4a7 的 Ease 默认 easeOutQuad：spawn 上升、群攻波 scale/alpha 和落地 nuke scale/flash 均需采用；nuke 呼吸显式 easeInOutQuad。
creepActions 的清晰束与 blur 束都在 effects/ADD，lab reactionLight 是 glow 150×150（不是 lab-mineral），say 尾部是圆角轮廓的一部分。
我方明确 actionLog 适配额外支持 attackController/claimController 的咬合/罩，而官方 creepActions 未读取这两键；断流 say 有限寿命、已消失 nuke 前态爆炸、固定种子常驻初相与构建期 blur 近似见 [R5 验证记录](renderer-r5-verification-2026-09-30.md)。

此前 lighting 的 real 状态仅表示 pass 提交成功，不能代替输出几何验收。R5 实际参考帧暴露 EMPTY→RT 纹理更换后 v8 Sprite.setSize 尺寸不保持，已修复并以真实 scale 加强回归；此前观感已对齐的结论以本次固定参考帧结果为准。
