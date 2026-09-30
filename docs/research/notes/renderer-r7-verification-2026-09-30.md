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
