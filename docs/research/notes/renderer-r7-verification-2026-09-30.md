# F2 R7 验证记录（进行中）

状态：方案提交 `b0a5dd1` 后获准开发；本文件是阶段证据，**不是收口报告**。
三轮矩阵、完整 10k soak、三路评审和灰区终裁尚未完成，不能据此宣称预算达标。
方案见 [R7](../../plans/f2-renderer-r7.md)，冻结指标见 [上位方案 §5](../../plans/f2-renderer.md)。

## 环境与口径

- 工作树 `.worktrees/renderer/`，`f2-renderer`；实现起点 `01558f0`，方案 HEAD `b0a5dd1`。
- MacBook Air Mac16,12，Apple M4，10 CPU/10 GPU，24GB；macOS 15.7.3（24G419）。
- Chrome 154.0.8037.58，有界独立 profile、前台页面；起采时接 AC、80% 电量、不充电。
- Node 24.11.0、pnpm 12.4.2、Pixi 8.21.0、TypeScript 7.0.2；Vite `--mode perf`、端口 5174。
- CDP device metrics：CSS/物理 canvas 均 1920×1080，DPR/Pixi resolution=1。
- 合成满配输入：300 对象（含 50 个各 50 body part 的 creep）、100 visual（五原语各 20）、地形码 0/1/2/4。
  来源是 `showcase.json` 的合成字段模板；没有读取/修改引擎或扩展 exporter。
- 沿用 `onFrameTiming`：objects 只计 `ObjectsStage.update`；lighting 只计离屏提交，不含 GPU 等待。
  全场景 CPU、RAF 区间、GPU 完成保守上界及原生时间戳分列；不以 HUD fps 代替耗时。
- 原生时间戳在真实 Pixi command encoder 的每个 render pass 首尾采集。M4 的 pass 时间区间存在重叠，
  保留原始 ns；pass 时长之和不能当作串行帧时长。跨度取所有 timestamp 的 max−min；上传/拷贝覆盖仍需单独核验。
- 分位按 nearest-rank 计算，保留 max 和超限次数；串行 GPU 完成上界含 CPU/调度等待，不能名为纯 GPU。
- 初始字体 URL 错误、主 CPU 计时分类错误的试采均保留并标无效/试采；Pixi 会就地填写 render options.target，
  现已改为调用前分类。正常满配路径确认 fonts=loaded、lighting=real、visuals=100。

原始试采与环境说明在 [数据目录](renderer-r7-data-2026-09-30/)；最终报告将按有效版本重跑补齐。

## 阶段性能证据与 D4

WebGPU 同 id 快照（120 次、未开启深度分项探针）：snapshot p95 1.5ms、max 2.3ms，3 次 ≥2ms；
恢复 100 visual 的合计 p95 2.4ms、max 3.5ms。冻结门是 snapshot <2ms，不能用 p95 小于 2ms 代替。

WebGPU 原生时间戳首轮 1800 帧：全 pass 跨度 p50 5.898ms、p95 13.107ms、p99 15.663ms、max 19.071ms；
同轮完整渲染 CPU p95 4.3ms、max 11.2ms。这些是修复前的一轮数据，尚非最终三轮结论。
串行完成上界的 300 帧试采 p95 15.9ms、max 18.4ms，包含调度等待。

2026-09-30 用户已确认 D4 推荐处置：完成全部实测，保持 normal 光照及冻结阈值，按正式差距登记降级和后续选项，
本片不扩增可选优化范围。尚无最终灰区终裁授权。

## 内存异常：Graphics context 未被精确销毁

WebGPU 标准链首次预热 1000 tick 后，双次 `HeapProfiler.collectGarbage`、间隔任务、五个
`Runtime.getHeapUsage` 样本中位数为 **2,572,724,132 bytes**；views=19、BindGroup=127、null slots=16。
随后 CDP 返回 `-32000 Target crashed`。这轮没有完成正式 10k，原采样器未分段落盘，只有已打印 checkpoint0；
其限制登记在 [异常记录](renderer-r7-data-2026-09-30/pre-fix-soak-failure.json)。不宣称稳态，不推算未执行 tick。
采样器现已增加预热/正式窗口分段落盘和五样本未 GC 窗口。

保留链证据：WebGL 1800 RAF 帧修复前基线结束时，GraphicsContextSystem 驻留 **13,434** context，
但房间对象仍为 300、visual 仍为 100。这支持逐次替换遗留 context 的诊断；并非由对象总量增长解释。

官方源码取证走 `gh api contents`，tag `v8.21.0`：

| 文件 | 内容 SHA | 结论 |
|---|---|---|
| [Graphics.ts](https://github.com/pixijs/pixijs/blob/v8.21.0/src/scene/graphics/shared/Graphics.ts) | `797762a7f54c00703957e748ab4b4bfeacca9274` | destroy 有 options 时，只有 context=true/全 true 才销毁 context；仅 children=true 不满足 |
| [GpuEncoderSystem.ts](https://github.com/pixijs/pixijs/blob/v8.21.0/src/rendering/renderers/gpu/GpuEncoderSystem.ts) | `970823756af8a49ed85d61a270d6e1d0de21c9a9` | 实际 command encoder/各 render pass 的采样接线依据 |

`VisualStage.clear` 使用 `entry.container.destroy({children:true})`；文字背景亦为 Graphics。
新增真实 Pixi 回归保存旧 context 身份，验证替换/销毁后 destroyed=true；旧实现实际失败（不是 mock destroy 参数断言）。
本轮提交时尚未实施该修复，也未把新的失败测试算为既有 350 测回归。
2026-09-30 用户追加授权：“先提交这一轮任务，然后修复bug，优化性能”。据此先保存本轮工具、
数据和失败回归的阶段提交，再实施生命周期修复及有实测依据的性能优化。此授权允许阶段提交，
不代表 R7 已验收；冻结指标、normal 光照、公共 API 和最终灰区拍板要求继续有效。

## 已完成的自验（阶段状态）

- 增加 context 回归前：renderer 361 测通过（原 350 + 守卫 8 + 满配 2 + 夹具枚举 1），shared 4 测通过。
  新增 context 回归在旧实现上失败，故当前整套测试不能宣称全绿。
- tsc 通过；renderer ESM+d.ts 构建通过；AST 闭包守卫覆盖 7 个 renderer 声明 + 3 个 shared 源文件，10 文件零 Pixi。
  深入裸包依赖增加闭包文件数，不改变公共 API。
- kk-cli 7 测通过，`cargo fmt --check` 与 `cargo clippy -p kk-cli --all-targets -- -D warnings` 通过。
- shared 以仅 src 的临时配置在 `/tmp/r7-shared-build` 生成 JS+d.ts；不改变 shared 的源码出口形态。
- 资产和全部真实/合成夹具再生后 173 个文件 SHA-256 全一致（含新满配夹具）；引擎源码未改。
- 三路 subagent 评审尚未启动；最终四文档回写、灰区拍板、收口提交尚未执行。

## 阶段修复与优化（821a02f 后；尚未收口）

- 用户要求的首轮证据提交：`821a02f`；随后把所有拥有自有 Graphics 的级联销毁补为 `context:true`。
  共享 atlas/standalone 的 texture 销毁规则不变。真实 context 回归通过，renderer 362 测全绿，shared 4 测全绿。
- 同标准回放预热 1000 tick 后 GC 堆中位数 31,234,620 bytes；正式试跑 1000 tick 后 35,126,084 bytes。
  对照旧预热约 2.57GB，证明主要累积链已切断；此试跑不能替代 10k。
  试跑 reset 错指非活动房的问题已修正，旧试跑 baselineReset 明确标无效，完整重跑会验证空房基线。
- rampart 顶罩裁剪由实测驱动：固定 40 私有格、1/4/16/40 用户组，修复 context 后整房烘焙
  场景的 source 纹素估算分别为 163.41/211.41/403.41/787.41MiB；裁剪后
  148.82/152.63/155.23/147.86MiB。此列含 Pixi 中间纹理池，最终另记纯顶罩 bytes；不称实际 VRAM。
  保留原合并 path、描边、像素采样栅格及边界裁切，只裁透明空间。32 个浏览器 raster 比较
  （cell=64/100，1024/2048，四角/连通/分离格）逐通道零差异。
- 同输入 600 帧 GPU 跨度 p95：裁剪前 14.025ms、后 14.549ms；完整 CPU p95 4.3/4.4ms。
  不据此声称帧耗时改善。保留裁剪的理由是明确的纹素/重烘焙成本下降；冻结 ≤8ms 仍未销账。
  此比较用初版夹具，两边一致；正式矩阵使用增强敌我/掉血、通行移动格、长动作束和动态几何的最终夹具。
- sampler 增补：实际 backend/adapter、source/fixture SHA、预热原始帧、device-loss/visibility 标记；
  标准回放绝对 deadline 含 spawning/effects，压力链含 dying/say/work/rampart 用户切换与 terrain 更新。
  每 tick 输入后及动画子步采集有限播放器/dying/overlays 峰值；GC checkpoint 附 0.001ms 排空浮点尾量。
  空房复位作用于实际活动房。分项探针仅在 profile=1，原生预算使用 profile=0。


## 2026-10-01 交接状态（用户要求停止开发并交给其他 AI）

代码修复/优化仍未提交；HEAD=`821a02f`，不是 R7 收口。用户已确认 G1–G13 整表，见
[灰区终裁记录](renderer-r7-gray-decisions-2026-09-30.md)。此前“尚无最终灰区授权”是历史阶段状态，现已完成。
三路 subagent 评审尚未启动。已归档当前完整原始输出，交接入口见
[R7 交接提示词](../../plans/f2-renderer-r7-handoff-2026-10-01.md)。

### 已完成完整链与首轮矩阵

WebGPU 标准 soak：1000 tick 预热 + 正式 10000 tick、每 tick 15 次真实动画/绘制提交，
GPU 完成后采样；0.001ms checkpoint 排空、双 JS GC、五个 heap 样本取中位；100 tick 未 GC 窗口另存。
源为 standard.json 30 增量的确定性 2000-tick 拼接周期，5 周期；不是引擎原产 2000 条输入。
最终原始记录：[standard 10k](renderer-r7-data-2026-09-30/final-r7-final-wg-standard-soak.json)。

| 正式 tick | heap median MiB | live Graphics contexts | managed context slots（含 null） |
|---|---:|---:|---:|
| 0 | 54.51 | 148 | 100781 |
| 1000 | 56.58 | 148 | 65148 |
| 2000 | 58.06 | 148 | 30648 |
| 3000 | 61.06 | 148 | 130648 |
| 4000 | 53.96 | 148 | 99648 |
| 5000 | 53.30 | 148 | 69248 |
| 6000 | 50.38 | 148 | 42948 |
| 7000 | 51.06 | 148 | 28049 |
| 8000 | 52.84 | 148 | 448 |
| 9000 | 67.51 | 149 | 100528 |
| 10000 | 69.48 | 149 | 99749 |

BindGroups 全程125、null slots16、destroyed slots0；sources73、source纹素估算114474608bytes全程一致。
空房复位 views/dying/tweens/overlays/finite active/nodes/texts 全0，lighting members=1/privateSources=1；
destroy 后上述及 Graphics/owned lighting/atlas leases 全0，剩余2个全局 WHITE/EMPTY 源共8bytes。
feed 仓库可独立继续使用，仍有非活动满配房300对象；无 feed 池，租约 N/A，不把仓库驻留误称GPU泄漏。
**不能宣称内存无增长**：终点比起点高14.97MiB；中途有回落，但末段抬升尚待归因/追加诊断。
Pixi GCManagedHash unload 后置 null，默认周期清表；slots 不是 live resources。不能只凭这个解释掉全部 heap 差距。
标准链没有触发 dying/finite；压力10k是验证它们的必需补项，尚未执行。

正式增强夹具第一轮（各1800有效帧+120预热，profile=0，visible=true/deviceLost=false/errors=0）：

| 场景 | CPU render p95/max ms | RAF interval p95/max ms | objects p95 / lighting p95 ms |
|---|---|---|---|
| WebGPU F0 4× | 4.1 / 8.0 | 17.4 / 17.8 | 0.2 / 0.2 |
| WebGPU F1 1× | 4.3 / 5.7 | 17.4 / 17.8 | 0.3 / 0.3 |
| WebGPU F1 4× | 4.3 / 10.1 | 17.5 / 17.8 | 0.3 / 0.3 |

同首轮串行完成上界 p95=12.4ms/max=19.1ms；原生 GPU pass 跨度 p95=13.6315ms/max=17.8258ms，
仍不能销账 ≤8ms。cold 首帧两次161.9/144.9ms；不是已完成三轮首帧结论。
当前还缺第二/三轮、全部正式 WebGL、F2/F3/H/R/P8最终组及三条 soak。
原始输出以数据目录 `final-*.json` 保存；没有生成或伪装未完成的 F0-r2 RAF 文件。

交接时 CDP9227连接已拒绝，尝试停采没有执行到页面；矩阵没有继续产生新文件。
后继须先查端口/进程，重新启动独立Chrome/Vite，不假定旧采样仍在进行。
最新自动检查：renderer362/shared4/kk-cli7、tsc、renderer构建/10文件闭包守卫、fmt/clippy通过。
交接整理补完临时副本的 assets/真实fixture/showcase/effects/perf全链，再生173文件SHA全部一致；工作树生成物未被副本验证改写。

## 2026-10-01短测阶段补充

上述“三路评审尚未启动”等为旧阶段时间点。现在已完成实际三路评审与清偿复审，
Graphics修复通过；新增门控/顶罩source复用/真实回放/null诊断，renderer369测全绿。
用户要求暂缓新增长测；最终收口仍未完成。最新方法、原始数据、限制与截图统一见
[短窗口与真实回放报告](renderer-r7-short-verification-2026-10-01.md)，不以它覆盖历史失败/长测证据。
