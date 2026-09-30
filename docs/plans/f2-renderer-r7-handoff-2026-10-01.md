# F2 R7 交接提示词（2026-10-01）

> 最新状态（2026-10-01，短测阶段提交前）：用户已恢复额度，并明确暂缓长时间测试，
> 授权自主修复/短测优化/借用官方真实回放、不再询问。以下旧交接路线仅作历史；
> 不应直接恢复长测。当前renderer369/shared4/kk-cli7全绿，175生成文件hash一致，
> 实际三路评审清偿复审通过。新增同tick门控、顶罩source复用、官方100tick数据与
> null哨兵诊断修正；短测不足以证明全局内存优秀/10k稳态，Pixi缓存槽增长仍登记。
> 权威后续入口：[短测报告](../research/notes/renderer-r7-short-verification-2026-10-01.md)。
> 原R7冻结阈值/API/R6/灰区确认不变，阶段提交不冒称R7最终收口。


下面内容可直接交给接手 AI。用户因当前会话额度要求尽快交接；不是取消 R7。

---

你是 kkscreeps F2 renderer R7 的接手执行者。继续当前实现和实测，完成 R0–R6 同款批次收口。
**先核查现场，不要从头重做或覆盖未提交修复。**

## 工作区、提交与授权

- 唯一工作目录：`/Users/keqing/Desktop/projects/games/kkscreeps/.worktrees/renderer`，分支 `f2-renderer`。
  主树有引擎未提交改动，禁止修改主树/引擎既有文件。本轮没有引擎/exporter扩展。
- 所有 shell 命令前缀 `rtk`，原始执行用 `rtk proxy`；每次命令显式工作目录。
  开始读 `/Users/keqing/.codex/RTK.md`。每次 Edit 后立即 git diff核对。
- 原始起点 `01558f0`；方案提交 `b0a5dd1`；当前HEAD **`821a02f`**，subject
  `renderer: R7 首轮性能测量与 Graphics 生命周期缺陷复现`。
  821a02f有工具、首轮证据和旧实现失败的真实Graphics回归，故那个提交有一个刻意新增红测。
  **当前未提交修复已使全套362测全绿**，不要把提交态红测与工作树态混淆。
- 用户已批准方案全部推荐，授权开发；D4用户批准冻结阈值下完成所有实测并如实登记差距/降级。
  用户随后要求“先提交这一轮任务，然后修复bug，优化性能”：821a02f是已完成的阶段提交；
  当前bug修复/优化/文档与新增数据尚未提交，最终收口仍需一笔独立提交。
- lighting仅normal，low/disabled不补；公共createRenderer API/d.ts形态、R6 D1–D7不重开。
- **G1–G13终裁已逐条建议后获用户“按整表推荐确认”**，不必再问同一问题；具体见灰区文件。
  G2按定义销账，但防御降级未触发的事实继续写明。性能/内存不达标不得用G2销账替代。
- 新方向/范围扩增仍须用户拍板。现有生命周期修复及有数据的性能优化已授权，无需重复权限确认。

## 开始必读（沿原顺序）

1. `docs/plans/f2-renderer.md`：§5冻结指标，§7R7，§12R5/R6移交与新增R7阶段记录。
2. `docs/plans/f2-renderer-r6.md`：最新§8执行记录末尾，不把旧322测当基线。
3. `docs/research/notes/renderer-r5-verification-2026-09-30.md`：“性能冒烟”与八测量点。
4. `docs/architecture/frontend.md`，`docs/roadmap.md`。
5. `docs/plans/f2-renderer-r7.md`：已批准方案，末尾当前决策记录。
6. `docs/research/notes/renderer-r7-verification-2026-09-30.md`：最新“2026-10-01交接状态”。
7. `docs/research/notes/renderer-r7-gray-decisions-2026-09-30.md`：已确认终裁。

## 已实现、未提交的修复/优化

1. **Graphics context泄漏修复**：Pixi8.21传非空destroy options时，只有context:true才销毁context。
   原 `{children:true}`让visual背景/形状、道路、有限特效、hover等context留到PixiGC。
   全部自有级联销毁补`context:true`，保留共享atlas/standalone纹理所有权，不扩大texture销毁。
   所有Graphics均独立创建，源码无context共享/clone。真实回归保存旧context身份验证destroyed=true，已通过。
   7个源文件：visual-stage/room/terrain/core renderer/lighting/effects-stage/objects-stage。
   旧标准1000tick预热约2.57GB后崩溃；修复后1000预热试跑约31MB，正式10k已能完成。
2. **rampart bbox裁剪**：整房2048²每用户组16MiB的测量依据充分；增加rampartRasterBounds，
   bakeCanvas可用整数像素region，原栅格采样相位、描边和房间边缘裁切保持。
   objects-stage烘焙sprite尺寸/坐标随region还原。32个浏览器raster案例逐通道零差异。
   40组场景所有source纹素估算787.41→147.86MiB；600帧同输入GPU p95 14.025→14.549ms，
   **没有帧耗时改善，不可称提速**。保留理由是明确的纹素内存收益，最终probe另有estimatedCoverBytes。
3. 满配夹具增强为敌我25/25、部分掉血、通行移动格、动作双通道同目标、100动态几何/文字，
   来源仍是showcase合成模板；只有renderer生成器变化，未动引擎。新夹具字节变化是预期变更。
4. perf-only probe/harness/driver：profile0预算与profile1分项分离、原始预热帧、actual adapter、
   visibility/deviceLost、源码/夹具SHA环境、feed map/notify/scene与object分项、门控/ValueTween计数、
   activity峰值、标准deadline偏移（含spawning/effects）、压力dying/say/work/terrain/rampart轮换，
   真实空房复位、checkpoint0.001ms排空、soak分段落盘。正常库构建tree-shakes掉perf入口。
   预算原生GPU timestamps是render-pass跨度；Metal pass有重叠，不能把pass和当帧时间。
   上传/copy未覆盖，串行GPU完成时间只称保守上界。
5. d.ts AST闭包守卫在821a02f已提交：动态import type/Pixi子路径/@pixi/裸包转递/未解析fail closed，
   8真实CLI测试。真实公共闭包10文件=7renderer声明+3shared源，不是改公共API。

## 数据与当前结论（不得冒充收口）

全部已完成文件已复制进`docs/research/notes/renderer-r7-data-2026-09-30/`，不只在/tmp。
旧无效字体URL/CPU计时分类试采保留并标无效；不要择优或删慢帧。

- `final-r7-final-wg-standard-soak.json`：WebGPU正式10k标准链完整，1000预热+15动画/绘制步每tick。
  输入是standard.json 30增量确定性扩展为2000tick周期×5，不是引擎原产2000tick。
  双JSGC+五样本median：起点54.51MiB、8000tick52.84MiB、10000tick69.48MiB。
  **终点+14.97MiB，尚未证无增长**；source73/纹素bytes114474608全程一致，BindGroups125全程一致。
  liveGraphics148→149，managed slots含null按Pixi周期清表波动，不能把slots数当livecontext。
  不能只凭null墓碑解释全部heap差距，需归因/追加诊断或诚实降级。
  标准链没有dying/finite，压力10k仍必须执行。
  reset: views/dying/tweens/overlays/finite active/nodes/texts全0、lighting members/privateSources=1/1。
  destroy:上述+Graphics/owned lighting/atlas leases全0，2个全局WHITE/EMPTY源共8bytes。
  feed无池；仓库仍有非活动房300对象是既有可继续使用契约，驻留有界，租约N/A。
- `final-webgpu-{f0,f1-1x,f1-4x,serial,timestamps}-r1.json`：各1800+120帧。
  F1-4×CPUrender p95/max=4.3/10.1ms；RAF p95/max=17.5/17.8ms；objects/lighting p95=.3/.3ms。
  GPUspan p95/max=13.6315/17.8258ms，串行完成上界12.4/19.1ms；冻结render≤8ms仍不能销账。
  cold-r1/r2首帧161.9/144.9ms，还缺第三次。
- `final-r7-final-wg-profile.json`：600帧profile1诊断试采，gate107600/equal70416/changed1584/null35600，
  joinwrites3723、animatedwrites2139；tweencreated4944/ended4043/cancelled901。
  分项inclusive，可嵌套重叠，不能全部相加冒充total。
- `r7-cover-before/after.json`、`r7-gpu-before/after-crop.json`用初版一致夹具作裁剪前后对照；
  正式矩阵使用增强夹具，两者不能混作同输入全场景性能对比。
- `r7-post-context-standard-pilot.json`只有1000试跑，clear误对非活动房的旧baselineReset明确无效；
  后来的正式10k已修正并通过复位，不能使用旧pilot作10k/复位结论。
- 正式WebGL矩阵、三条剩余10k（WGpressure、WGLstandard/pressure）、F2/F3/H/R/P8最终出数尚未执行。
- BindGroup警告未屏蔽。标准链资源destroyedSlots=0、模块缓存groups125稳定、销毁后null slots183，
  堆增长尚未归因，不能据此把所有warning/cache风险直接销账。压力链会持续重烘焙，尤其要查模块cache增长。

## 测量入口与进程现场

- 工具：`packages/renderer/dev/perf.html`、`dev/perf.ts`、`src/perf/{fixture,probe,gpu-timer}.ts`。
  Node24 driver：`scripts/perf-cdp.mjs`；批矩阵：`scripts/run-perf-matrix.mjs`。
- 交接时Vite5174仍返回200；Chrome CDP9227 **ECONNREFUSED**，矩阵停在F0-r2，未落盘这轮，不能当有效样本。
  后继先查端口/相关进程，只操作独立profile；不要操作用户默认Chrome。
  旧工具sessionID不要依赖，它们未必跨会话可用。
- 重开Vite（如未运行）在renderer包：`rtk proxy pnpm exec vite --host 127.0.0.1 --port 5174 --mode perf`。
  确认vite.config.ts root/dev与assets挂载，沿现有启动方式，不改公开API。
- 独立Chrome：`rtk proxy /Applications/Google\ Chrome.app/Contents/MacOS/Google\ Chrome --user-data-dir=/tmp/kkscreeps-r7-chrome --remote-debugging-port=9227 --force-device-scale-factor=1 --window-size=1960,1180 --no-first-run --no-default-browser-check 'http://localhost:5174/perf.html?smoke=1'`。
  headed前台、CDP强制CSS/物理1920×1080/DPR1；用getStats实际backend、lightingreal、fontsloaded、visuals100核验。
- renderer包cwd例：`rtk proxy node scripts/perf-cdp.mjs --action soak --backend webgpu --kind pressure --ticks 10000 --warmup 1000 --reload true --out /tmp/r7-wg-pressure.json`。
  actions:startup/raf/serial/timestamps/snapshots/hover/hoverMove/ramparts/recovery/cropParity/soak。
  timestamps需`--gpu timestamp`；默认profile0，诊断`--profile 1`。raw output自动含环境/源码hash。
- matrix脚本顺序跑全矩阵，默认会重跑已完成项；若续跑应显式安排缺项，不把旧文件存在当当前新采。
  **脚本末尾有特例跳过WGstandard，默认依赖本轮已归档标准链**；复现全部时需补一次该链。
- 采样期间不要改src/dev/fixture/asset触发HMR，不并发重负载测试/多后端。
  可复用已完整首轮，若改功能或关键方法学必须受影响组重跑并保留旧证据。

## 已验与剩余流程

已通过：renderer362（原350零回归）/shared4/kk-cli7、tsc、fmt、clippy-pkkcli-alltargets-Dwarnings，
renderer ESM+d.ts构建+10文件零Pixi守卫、shared仅src临时配置JS+d.ts构建。
交接整理后文档链接85Markdown/300本地链接通过，git diff --check通过。

资产链173文件：原先全链重生一致；增强夹具后本轮在`/tmp/r7-regenerate-renderer`副本完成assets+真实fixture，
交接整理时已补showcase/effects/perf并核对173文件SHA全一致。为了不触发采样HMR使用副本；其node_modules链接renderer包。
`/tmp/r7-regenerate-before.json`是新一轮W资产/夹具hash快照；`/tmp/r7-shared-build-config.json`仅include src。
不要直接命令覆盖shared rootDir造成test旁冒出JS/d.ts（之前已清净，不重犯）。

必须继续：
1. 补全三轮双后端/F2/F3/H/R/P8、剩余三条10k，复核每项采样环境与场景。
2. 八点P1–P8逐项出数：lighting耗时/dirty比例；apply分项；50creep双后端；Tween规模；
   snapshot/rebuild单帧尖峰；值门控计数；10k heap/资源/lease曲线；大delta恢复/小步对照。
   recovery已有plain motion对照但结果还未采；真实ticker stall仍需补验证或明确记录已有行为测试范围。
3. 依据数据决定hover格索引/视图池是否做。尚无这两项优化，建议先测；裁剪有证据可保留。
   优化无收益回退；不得改G10/G12官方quirk或R6冻结政策。
4. 逐项记录§5达标或差距数字/原因/后续选项；本片不放宽指标，不以p95盖过max。
   内存若仍不稳按D4诚实登记，不虚构稳态。BindGroup源码/实际保留链必须附证据，不屏蔽日志。
5. 完整自验后**真实3路subagent评审**（方法学数据/优化生命周期/文档灰区），清偿→复审。
   当前一个agent都没启动，不能补写“已评审”。用户明确要求这三路；总并发≤6（环境若只4槽则3子+主）。
6. 四文档已有阶段指针，最终写终态：上位§12/R6记录/frontend现状/roadmap renderer子项。
   灰区整表已确认，按文件回写，无需重问。roadmap整体M3仍不可勾选。
7. renderer+shared全测、tsc、kk-cli7、fmt/clippy、links、asset全链重生hash一致、两包构建+守卫，
   双后端harness人工核验（目前看过WG图；正式WebGL人工核验还欠）。
8. 单一最终收口提交：subject“前缀：描述”，冒号前≤48字、全长≤350；数据提交body/报告必须写环境与方法。
   用户只要求本次整理交接，**不要把未完成R7的当前工作树直接称收口或丢弃**。

## 防错清单

不用HUDfps冒充CPU/GPU；未100visual不称满配；未10k不称稳态；原始慢帧/无效轮保留。
boolean过滤改写按分支真值表（player=null）；PartValues.visible必须boolean。
夹具独立id域，snapshot全量即时同步防幻影移动；Pixi活children断言用节点身份。
rampart排序码点比较不localeCompare；transparent alpha0无层；HSL数字与百分比同0–100标尺。
官方文档只走GitHub仓库`gh api contents`固定tag/ref，CSS歧义getComputedStyle；不整页锚点抓取。

## 官方与环境证据

MacBook Air Mac16,12/M4 10CPU10GPU/24GB，macOS15.7.3，Chrome154.0.8037.58，Node24.11.0，
Pixi8.21.0、TS7.0.2、pnpm12.4.2；WebGPU adapter apple/metal-3/nonfallback。每轮power/日期/源码hash随raw保存。
GitHub contents已取证Graphics.ts SHA797762a7f54c00703957e748ab4b4bfeacca9274、
GpuEncoderSystem.ts SHA970823756af8a49ed85d61a270d6e1d0de21c9a9、
getTextureBatchBindGroup.ts SHA51601c1db77cfa782e7157291d5b0f1b1165d269。
后者模块cachedGroups按纹理UID组合FNV缓存，BindGroup.onResourceChange销毁资源时清null。
警告栈与原始resource计数留raw；是否增长必须拿压力链核验，不能从历史文字直接推断。
