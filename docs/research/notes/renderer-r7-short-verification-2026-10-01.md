# R7 短窗口优化与官方真实回放（2026-10-01）

本阶段按用户新授权暂缓长时间测试，先用短窗口定位可重复的分配/对账问题，
借用用户本地官方 demo 回放搭建完整房间。**没有新增 10k soak，没有宣称内存
整体优秀、长期稳态或 §5 全达标。** 模糊/锯齿专项暂缓，normal 光照、公共 API、
R6 决策及已经用户确认的灰区表不变。本阶段提交接续 `821a02f`，不是 R7 最终收口。

## 环境与可复现方法

- Mac16,12 / Apple M4 / 10 物理核 / 24GiB RAM，macOS 15.7.3 (24G419)。
  AC 电源、80% 电量且不充电为测试后读取，未连续监测电源/温度。
- Codex 内置 Chromium 154。WebGPU：Apple Metal-3；WebGL2：ANGLE Metal Apple M4。
  临时 viewport 1920×1080、DPR=1，实际 CSS/canvas/backend/visibility 随各 raw 记录。
  不把这些单轮诊断当独占机器的正式性能验收；并行 demo 会影响绝对数值。
- CPU：满配合成 300 对象（含 50 creep）/100 visual/全地形，40 次同 id 快照，
  配套 visual 替换；RAF 30 帧预热、180 帧取样，全部 creep 移动、4×。
  `onFrameTiming` 的 objects 仅 `ObjectsStage.update`，lighting 仅离屏提交；
  `cpuRenderMs`=scene+main CPU 提交，**不是 GPU 完成时间**；fullCpu 含 feed 等本帧工作。
- 内存压力：100 tick 预热+100 tick 正式，每 tick 15 个手动动画/绘制步并等待 GPU 完成。
  tick 100/200 为同一压力周期相位，另取 tick150；每点五次 heap 读数、间隔100ms。
  无强制 GC，`performance.memory` 只是自然采样近似值，不能推断 retained heap。
  资源计数覆盖 views/dying/overlays/TweenManager/finite effects/lighting、自有纹理、
  Pixi managed slots（含 null）、Graphics slots/活项、BigPool、批次 BindGroup。
  RGBA8 纹素字节估算不含 MSAA、buffers、驱动驻留，不能称实际 VRAM。
- 分位数：本短诊断表用 nearest-rank `ceil(p*N)-1`，p50 中位数；保留全部样本，
  不挑最佳值。少量样本不同 p95 索引会差 0.1ms，不据此销账冻结阈值。
- [环境、源码/数据 SHA](renderer-r7-short-data-2026-10-01/environment.json)、
  [可复算汇总](renderer-r7-short-data-2026-10-01/summary.json)。源码 hash 是最终实现，
  历史 before 阶段按文件名/下列变更说明识别，未冒称已重建历史源码 hash。
  短跑未全程记录所有 console/visibility 事件；RAF raw 有 invalidVisibility/deviceLost。

复现入口（在 renderer 包目录）：`rtk proxy pnpm exec vite --host 127.0.0.1 --port 5175 --mode perf`，
打开 `/short.html?profile=1` 点 CPU 按钮；资源测试打开 `/short.html` 点资源按钮，
完成后点销毁检查。WebGL 加 `&backend=webgl`（无其他 query 时用 `?backend=webgl`）。
官方真实短跑用 `/short.html?fixture=official-room`，WebGL 再加 backend，点击完整回放。
官方模式初始化即为真实快照、原 player、2500ms，没有合成房间预热/100 visual。
销毁后按钮禁用，刷新重新建立实例；初始化监听安装完成前按钮禁用。

## 数据驱动的修复与优化

1. 保留上一阶段 Graphics 精确释放修复及顶罩 bbox 裁剪：独立 Graphics 级联销毁
   `context:true`，不波及共享 atlas。此前 32 个 CPU raster 案例零差异；40 用户组
   source 纹素估算 787.41→147.86MiB，但旧 GPU p95 无改善，仍只登记内存收益。
2. 同 tick visual/terrain 通知不再重复对象对账。按 room/time 门控；snapshot/切房
   强制处理，失败 retry 保留 snapshot 语义。3 个新增回归覆盖移动不中断、同时间
   快照即时重置、切房和失败重试；不改 FeedView/public API。
3. 顶罩同用户组重画时复用 Canvas/TextureSource/Texture/Sprite，动态 frame 同步
   宽高/UV；仍按旧 changed-group append 层序。私有 canvas `Texture.from(...,true)`
   跳过全局资源 cache，组删除仍真正销毁自有 source。
4. 真实回放揭示 `effects.skipped=11965` 是官方无动作 null 哨兵被计为非法动作，
   不是丢失11965条实际动效。修正仅诊断：undefined/null 均忽略；false/0/[]/非法对象
   仍 skipped；有效对象照常生成 commands/hits。新增测试逐分支覆盖，旧 raw 保留。

| CPU 短诊断（ms，p95/max） | 门控前 | 门控后 |
|---|---:|---:|
| 同 id snapshot 对账 | 2.6 / 2.9 | 2.7 / 3.2 |
| 配套 visual 替换 | 1.6 / 1.9 | 0.8 / 0.8 |
| snapshot+visual 合计 | 4.2 / 4.5 | 3.5 / 4.0 |
| RAF CPU render | 6.4 / 8.5 | 4.4 / 5.5 |
| RAF full CPU | 10.7 / 14.7 | 6.6 / 9.8 |

只支持重复对账的局部收益。snapshot 本身未改善且依然有 ≥2ms 样本；RAF render
中位数 2.95→3.2ms、snapshot 后 render max 9.7→14ms，也保留，不以好值替代差值。
未测 GPU 时间，本阶段不能销账 ≤8ms 或完整 60fps。原始
[before](renderer-r7-short-data-2026-10-01/cpu-before.json)、
[after gate](renderer-r7-short-data-2026-10-01/cpu-after-gate.json)。

顶罩复用的四组变化覆盖54→95→48→2048像素边界，再空组删除。双后端
`renderer.extract.pixels(Texture)` GPU 纹理读回逐通道零差异，宽高一致，
Canvas/Source/Sprite 身份连续保持，删除后 Sprite/Source destroyed=true。
这验证纹理上传/更新，**不包含最终 Sprite 变换/SCREEN 合成的整帧像素相等**。
[WebGPU](renderer-r7-short-data-2026-10-01/parity-reuse-webgpu.json) /
[WebGL](renderer-r7-short-data-2026-10-01/parity-reuse-webgl.json)。

## 短窗口内存结论与剩余风险

| 指标（同相位100→200 tick） | WebGPU 复用前 | WebGPU 复用后 | WebGL 复用后 |
|---|---:|---:|---:|
| 活 Source / 估算 MiB | 83 /147.01→同值 | 83 /147.01→同值 | 81 /135.10→同值 |
| 活 Graphics | 639→639 | 639→639 | 639→639 |
| managed texture slots | 94→108 (+14) | 90→96 (+6) | 88→94 (+6) |
| BindGroup 数 | 165→179 (+14) | 164→176 (+12) | 14→14 |
| BindGroup null slots | 34→68 (+34) | 26→44 (+18) | 0→0 |
| Graphics tombstone+活槽 | 10719→20804 | 10719→20804 | 10719→20804 |

复用减少 source 创建/销毁引发的槽和缓存 churn，未消除 Pixi 模块缓存增长；没有
显著帧提速结论。两轮自然 heap 中位数 WG 前77.56→88.52MiB、WG 后107.95→100.46MiB，
WGL94.84→88.77MiB；起点/自然GC不同，**不能宣称净省 heap**。WG约11.64秒、WGL约23.27秒。
两段正式窗口覆盖 dying峰5、overlays367、tweens103、finite active60/nodes185/texts5。
空房 reset：views/dying/tweens/overlays/finite/ambient=0；lighting members/privateSources=1/1
是活 renderer 的 ambient/RT，顶罩=0。BigPool BatchableGraphics2235、RenderData66保持有界。

随后的完整 destroy：自有 textures/Graphics/managed textures/lighting members/privateSources/
finite/ambient/views/tweens/overlays/atlas refs/standalone leases/BigPool均0。剩全局
WHITE/EMPTY两源共8bytes；WG还有101个模块 BindGroup/757null slots，非自有悬挂资源，
不可写全部 Pixi 资源归零。FeedStore是普通JS Map，无租约池；feedPoolLeases=null表示
不适用，不是测得0，reset后的一个空房仍保留在feed，待handle回收。

BindGroup warning保持日志，沿用R5栈追踪的 batch cache/onResourceChange证据；本次
销毁后 bound destroyedSlots=0、资源槽变null而缓存仍在，进一步支持上游缓存归因。
未擅自破坏/销毁 Pixi 缓存组。Graphics tombstones及BindGroup后续可选择向上游报告，
或在用户以后恢复长测时观察GC/实例重建边界。暂不引入私有字段清理或新池。

原始 [压力前](renderer-r7-short-data-2026-10-01/memory-before.json)、
[WG后](renderer-r7-short-data-2026-10-01/memory-after-reuse-webgpu.json)、
[WGL后](renderer-r7-short-data-2026-10-01/memory-after-reuse-webgl.json)、
[WG销毁](renderer-r7-short-data-2026-10-01/destroy-webgpu.json)、
[WGL销毁](renderer-r7-short-data-2026-10-01/destroy-webgl.json)。

## 官方完整场景与视觉观察

只读用户 checkout `/Users/keqing/Desktop/projects/sources/screeps/renderer`。
引擎/metadata源码对应HEAD `a2db4a76bb8f4c70e0c2a9e3b7d22a35a8c6b504`，实际借用的
objects/terrain有用户本地修改，**不是宣称录制字节来自该Git提交**：
objects SHA256 `b2efe482fdb0e9f80c6c7a77d325bd39aa8feff1a88f1b8931c5e0ced38b59d0`；
terrain `6efa4d9a45c0e02265b6c44495b17da3a454070494bbbd0d384c6e3ed559380a`。
`build-official-replay.mjs`只读这两个输入、LICENSE及Git revision，输出携带 provenance
与官方许可证；官方树及引擎既有文件零改动。

- E54S13、24242100–24242199共100tick，310–313对象、8–11creep；首312/11、末312/10。
  plain944/wall1355/swamp148/wall|swamp53（互斥代码），8条对象删除。
  这不是300/50/100满配验收夹具，没有100 visual，不能替代预算验收。
- converter严格复现官方浅数组转数字键对象后lodash深merge规则：遗漏保留、nested
  null覆盖、对象null删除、部分body复原。独立官方lodash oracle逐100帧、feed最终
  状态及完整actionLogs全部匹配；原id独立于合成夹具，seek全量恢复防幻影移动。
- 真实runner最终只有1房312feed对象、191views（道路批处理），末198overlays，
  动画结清后finite/tweens/dying=0；WG34source、WGL32source。99次delta各推进
  15×2500/15ms，每tick排空探针数据。WG约3.48秒、WGL约16.38秒；apply p95/max
  1.5/3.3ms与2.9/3.5ms，仅feed CPU对账。首帧原始104.5/2311.4ms均记档，WGL不设门，
  此单轮非冷启动验收，不能冒称四指标全过。
- 初次真实短跑曾保留合成房（2房612对象）、旧null统计；保留`*-pre-isolation.json`
  供审计，已由上述干净模式替代，不用来宣称真实资源基线。
- 官方reference改在tmp将engine/src/index（包含pixi-global）与metadata源码同bundle
  编译，复现官方demo初始化；旧分离dist+单次render出现不完整画面已排除。
  仅启用Pixi render ticker等待异步SVG/blob，未调用GameRenderer.start。动作年龄固定，
  **swamp的Ticker.shared仍随实时时钟动画**，所以这些截图是定tick视觉观察，不是
  全画面黄金帧。若未来量化视觉差异，需先控制共享ticker相位/文字归一化。
- 双方按100cell、2500ms、原player、房中心2450/2450、zoom .2比较；装饰关闭。
  用户3000demo的蓝墙/涂鸦/neon属装饰，原排除范围保留。布局、墙轮廓、道路、
  建筑及回放对象完整可见；不宣称像素等价。沼泽相位/亮度、徽标placeholder(G3)、
  本库public speech与原demo关闭speech、首snapshot无动作输入、采样微差继续登记。
  模糊/锯齿专项按用户暂缓；未新增视觉特性。

[我方WebGPU末tick](renderer-r7-short-data-2026-10-01/real-webgpu-tick99.jpg) /
[我方WebGL末tick（null诊断修正前，画面命令不变）](renderer-r7-short-data-2026-10-01/real-webgl-tick99.jpg) /
[官方同数据末tick](renderer-r7-short-data-2026-10-01/real-official-tick99.jpg)。
真实demo：`http://127.0.0.1:5174/?fixture=official-room&view=2450,2450,0.2`，空格播放、`.`单步。
官方参考：运行prepare脚本返回tmp目录、用Python localhost server服务，query同夹具/tick/view。

## 自验、三路评审与阶段收口

renderer369测（原350零回归）、shared4、kk-cli7；两包tsc、Rust fmt/clippy均通过。
renderer ESM+d.ts构建、AST闭包守卫10文件零Pixi通过；shared仅src临时JS+d.ts构建通过，
不改变其TS源码出口。资产/真实夹具/showcase/effects/perf/官方回放全链重生175文件
SHA一致（含官方LICENSE），[再生清单](renderer-r7-short-data-2026-10-01/regeneration.json)。
文档链接、diff whitespace及harness构建结果在提交前核验。

三路实际subagent只读评审：生命周期（gate重试/私有source更新/层序/删除）通过；
内存与方法学（heap局限、缓存增长、GPU纹理读回口径、null诊断真值表）清偿复审通过；
回放（独立oracle、资源反向映射、2500ms、synthetic预热隔离、shared ticker限制）清偿复审通过。
发现的reference掉落资源漏画、错误tick周期、额外合成visual/房驻留、null伪诊断、
初始化点击竞态均已清偿。hover格索引/detached池仍无证据不做；不得把本阶段作为
原冻结验收与未完成长期矩阵的替代。四份上位文档继续保留R7未最终收口状态。
