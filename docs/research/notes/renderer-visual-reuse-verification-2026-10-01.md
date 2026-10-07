# RoomVisual 缓存与短窗口性能复核（2026-10-01）

本轮接续已提交的放大黑屏修复 `e7e30db`，按用户授权继续优化，仍暂缓新的长时间测试。
主要处理每 tick 全量 RoomVisual 销毁重建造成的 Graphics UID/cache 槽位新增。
本报告不是 R7 最终验收：冻结的快照 `<2ms`、GPU 渲染 `≤8ms` 尚有差距，
短窗口未证明 10k tick heap 无增长。normal 光照、公共 API、R6 D1–D7 与灰区终裁不变。

## 1. 环境与复现

- 工作树 `.worktrees/renderer`，主树与官方 checkout 未改；基线 `e7e30db`。
- Mac16,12 / Apple M4 / 24 GiB；Chromium 154，WebGPU Metal-3 与 WebGL ANGLE，
  实际 adapter/driver、时间、visibility、字体和后端记录在每份 JSON 的 startup/environment。
- CSS 1920×1080；perf 专用 `resolution=1` 在初始化后把 canvas 改为物理 1920×1080。
  浏览器 DPR 仍为 2，初始化已选择 @2x atlas。它不是 DPR1 的冷启动验收。
- 满配为已有合成夹具 300 对象、50 creep、100 visual、全类型地形；50 creep 全移含攻击，
  `lighting=real`、fonts=loaded。逻辑对象数与合并 road 后的 view 数分别登记。
- [`environment.json`](renderer-visual-reuse-data-2026-10-01/environment.json) 保存硬件、源码哈希与限制；
  [`原始数据目录`](renderer-visual-reuse-data-2026-10-01/) 保留所有试验和回归，未择优删轮。

在 renderer 包启动 `pnpm exec vite --mode perf --host 127.0.0.1 --port 5175`，
打开 `/short.html?profile=1&resolution=1`，WebGL 加 `backend=webgl`。
通过页面按钮运行、从 `#result` 保存 JSON。GPU 原生时间戳加 `gpu=timestamp`；
无分项 wrapper 的 CPU 三轮入口不传 `profile=1`。

CPU 窗口每轮 40 次同 id 快照，加 30 帧预热后 180 帧 4× RAF；快照后恢复 100 visual，
snapshot 和 visual 分列，并报合计。RAF 的 visual 更新只在 tick 出现，非 180 次更新。
短矩阵每档 30 帧预热+90 帧；objects 仅 ObjectsStage.update，lighting 是离屏提交，
CPU 和 GPU 完成上界/时间戳分列，沿用 R5 口径，不使用 HUD fps 代替耗时。

压力链 100 tick 预热+100 tick 测量、每 tick 15 步，包含 dying、攻击、say 与 overlays；
tick 100/200 同为周期 phase0。每个 heap 窗口取 5 次、间隔 100ms，不强制 GC；
纹素按 RGBA8 估算，不含 GPU buffer/驱动分配。Pixi 自然 GC 可重建去除 hash 空槽，
因此缓存槽计数不保证单调，特别是运行超过默认 30 秒 GC 周期的 WebGL 链。

## 2. 实现与取舍

`VisualStage` 保留全量替换和同数组引用短路；按原输入槽位复用同类型节点。
变类、失效、缩短即时销毁，不保留 detached view 池。每个形状最多两份私有
GraphicsContext，以完整解析后几何/样式值为键做 LRU：命中切换，未命中重绘已有 context，
移除时先脱 Graphics 监听再销毁两份。上限仅指 context 份数，复杂路径 GPU buffer 的高水位未证明。

Text 更新内容/位置，完整样式变化时替换并销毁旧独占 TextStyle；背景按实际宽高、anchor、
padding、颜色/alpha/opacity 门控，关背景时释放 context 并清 key。Text 删除仍走四标志。
键只使用 resolver 输出，未把脏数据里的循环对象或额外字段序列化为绘制条件。
对应原语和 CSS 默认值未改。Pixi 的 style setter 只脱旧 listener，CanvasTextPipe 按旧 text key
归还纹理引用；取证采用 GitHub contents API 与本地固定 8.21.0 源码，见
[AbstractText](https://github.com/pixijs/pixijs/blob/v8.21.0/src/scene/text/AbstractText.ts)、
[GraphicsContext](https://github.com/pixijs/pixijs/blob/v8.21.0/src/scene/graphics/shared/GraphicsContext.ts)。

perf-only `counts().partCounts` 改为逐 checkpoint 拷贝，修复旧 checkpoint 在最终 JSON
序列化时被后来计数覆盖的问题。历史 before/v1 的该嵌套计数不能用于时间差分；
标量计数、GPU/CPU原始记录仍有效。公共构建继续消除 perf bridge。

### 探索过程与保留证据

- v1：只复用节点、每 tick clear 重绘。WG 第一轮 RAF render/full CPU p95 从
  4.3/5.8ms 变为 6.9/9.8ms；该轮还重叠本地验证命令，保留为有噪声诊断。
  GL 隔离 ABBA 提交 p95 1.1→2.1ms，不能称整体提速。
- v2：尝试局部坐标+节点平移。WG 原始 RGBA 共 43 个通道变化、max 45/255，
  已撤回，不以肉眼近似替代保真。
- v3：保留原绝对坐标，两份几何缓存与背景 gate。WG 隔离 ABBA update p95
  0.8→0.3ms、submit 1.6→0.8ms、完成上界 8.9→7.6ms。
  ABBA 输入只有两态交替，不能扩大为所有动态几何都加速。

ABBA 为 rebuild/reuse/reuse/rebuild 四臂，每臂 20 预热+60 样本，100 visual 渲入
独立 640² RT；rebuild 臂先清空整批，reuse 臂保留节点，合并各 120 样本。
这是同一新绘制实现的策略对照，历史旧源码的真实前后 JSON 另行保留。
持续不同坐标的 unique ABBA 用于补证缓存未命中成本，不能替代像素金标准。
另试miss时GraphicsContext.unload：GL p95 submit1.8→1.9ms、完成16.7→16.8ms，
无稳定完成收益，已撤回；原始`webgl-visual-miss-unload-experiment.json`保留。

## 3. 视觉与生命周期证据

双后端各 8 项 ordered GPU readback：100 visual 三次 A→B→A、透明文字+独立描边/背景、
默认样式回落、背景恢复、text→rect、空数组。640²、resolution1，
每项保存 1,638,400 字节 RGBA 的 gzip/base64 和 SHA256；原实现与 v3 逐字节零差异。
覆盖五原语、缓存命中、文字更新和清空；不扩大为所有视口/DPR2、第三份几何逐出的 GPU 像素、
重叠 alpha 层序、脏输入 GPU 或与官方全房逐像素等价。

新增行为回归覆盖无效槽位与顺序、变类/缩短、同原语被修改的新数组、样式/背景重置、
两份 LRU 命中/逐出、40 次不同几何的份数上限、绘制失败后的两份释放与下一批恢复。
节点身份断言不依赖 Pixi 活 children 数组。官方真实回放没有 visual，仅补证其他层不回归。

## 4. 实测汇总与 R5 八点状态

[`summary.json`](renderer-visual-reuse-data-2026-10-01/summary.json)、
[`memory-windows.csv`](renderer-visual-reuse-data-2026-10-01/memory-windows.csv) 和
[`pixel-differences.json`](renderer-visual-reuse-data-2026-10-01/pixel-differences.json)
由 `python3 packages/renderer/scripts/summarize-visual-reuse.py docs/research/notes/renderer-visual-reuse-data-2026-10-01`
重新生成：47份诊断结果、105次未强制GC heap采样、16对实际解压像素。
所有统计为 nearest-rank p95，不插值；下列短证据不等于正式验收。

| 隔离100 visual策略对照 | update p95 rebuild→reuse | CPU submit p95 | 含等待完成上界p95 |
| --- | --- | --- | --- |
| WG 两态交替 | 0.8→0.3ms | 1.6→0.8ms | 8.9→7.6ms |
| WG 连续新坐标 | 0.7→0.4ms | 1.5→1.4ms | 8.9→8.2ms |
| GL 两态交替（第一轮） | 1.3→1.1ms | 2.3→2.2ms | 17.8→24.8ms |
| GL 两态交替（fresh复测） | 1.1→0.6ms | 1.7→1.1ms | 20.1→24.4ms |
| GL 连续新坐标 | 1.0→0.7ms | 1.8→2.1ms | 16.6→16.8ms |

WebGL缓存miss的update节省与submit增加约0.3ms相抵，不能称持续几何明显提速。
两态完成上界恶化在复测仍出现；GL使用fenceSync/clientWaitSync零timeout与setTimeout(0)轮询，
此上界包含timer nesting/clamping与调度，但尚未归因，不能写成“只是timer噪声”。
本轮保留理由是可复核的节点/UID分配减少、WG数据和限定CPU收益，非双后端整体GPU提速。
不额外增大缓存份数掩盖差距；全房GPU瓶颈仍待后续分析。

| 点 | 已获得的短窗口证据 | 仍未完成的验收范围 |
| --- | --- | --- |
| P1 RT/dirty | WG v3 静态/1×全移/4×全移均 dirty90/90；lighting CPU p95各0.2ms，max0.4/0.4/0.3ms | GPU pass未单独归因到lighting；三轮正式完整矩阵 |
| P2 apply分项 | profile桥记录feed map/copy、notify、terrain/objects/visual、reset、road/cover/parts/ambient；快照与恢复visual分列 | 不把分项 wrapper 数据冒充无探针预算；更细未被单独包裹的内部步骤不虚填 |
| P3 双后端全移 | 满配与双倍率短矩阵；无分项wrapper三轮，WG snapshot≥2ms为6/2/0次、GL为10/6/3次 | 正式三轮双后端全部矩阵与冷启动 |
| P4 Tween规模 | WG静态峰3、50全移峰103；update p95静态/1×/4×为0.2/0.3/0.2ms | 完整时序与所有取消来源分解 |
| P5 重建尖峰 | 双后端各30次独立id全量重建及30次地形重建，CPU/含等待尖峰见下表；每次换id创建/销毁各240 view | 短窗口并非三轮全部矩阵；旧20次pilot完整保留 |
| P6 值门控 | WG三档结束equal=2151/4302/10038，125个type/part计数分解，checkpoint不再别名覆盖 | 停用→恢复及lighting失效来源全矩阵 |
| P7 内存 | 匹配phase100/200的100tick窗口、heap5采样、纹理/source、dying/overlay、池与清空/destroy | 用户暂缓新10k，短测不是稳态证明 |
| P8 大步进 | 双后端0/16.667/250/1000/10000/60000ms，50creep位置/旋转/alpha/overlay终态与小步一致 | 手动公共advanceAnimation，不冒充真实ticker长帧/后台stall |

WG原生 GPU全pass跨度 p95/max=13.500416/14.221312ms，较8ms差5.500416/6.221312ms；
串行完成上界 p95/max12.7/13.9ms。时间戳跨度/相加/完成上界各自含义不同，
不能相互替代、不能将跨度与CPU简单相加；upload/copy不在render-pass时间戳范围。
CPU三轮快照 p95=2.2/1.8/1.5ms、max2.7/2.2/1.7ms，120样本共8次≥2ms，
完整CPU p95=6.4/5.2/4.6ms；RAF间隔p95=16.8/18.2/18.4ms。保留暖机/调度波动，
不以第三轮单独达标宣称全部预算通过。GL三轮snapshot p95/max分别2.4/3.8、2.0/2.2、
2.0/2.1ms，120次有19次≥2ms；fullCPU p95为5.6/5.3/5.4ms、RAF间隔p95为17.3/17.6/17.1ms。
WebGL按原方案只记数不设门。startup来自有缓存、初始化后resize的诊断页，不销账正式首帧。

| 30次重建（p95/max，ms） | snapshot CPU | 下一帧CPU提交 | 下一帧完成上界 | snapshot+visual+完成上界 |
| --- | --- | --- | --- | --- |
| WG 换id | 5.1/5.9 | 8.7/9.8 | 36.3/47.7 | 40.1/54.3 |
| WG 换地形 | 3.8/4.2 | 8.2/8.3 | 35.8/36.4 | 39.2/40.1 |
| GL 换id | 6.1/7.7 | 9.4/50.7 | 54.9/65.4 | 60.5/74.2 |
| GL 换地形 | 2.9/3.1 | 8.6/9.0 | 43.2/44.6 | 46.6/46.8 |

road合并使300逻辑对象对应240 view；换地形每次view创建/销毁为0。
这些重建窗口带profile分项并串行等待GPU，不能混成RAF稳定帧预算。
匹配CPU链的同id快照WG/GL p95=2.5/2.8ms，其中Objects apply total=2.2/2.4ms，
parts=1.7/1.7ms、reset=0.3/0.2ms、road=0.1/0.1ms、cover=0.1/0.1ms；
各分项独立p95不可相加。feed map/copy、notify与各apply记录在summary分组，未测内部步骤不填0。
GL静态/1×/4×全移lighting CPU p95=0.2/0.3/0.3ms，objects=0.4/0.5/0.4ms，
Tween峰值3/103/103、update p95=0.2/0.4/0.3ms；两后端dirty均每档90/90。
静态scene仍有环境动画，并非完全无dirty。悬停短测未证明pick为主要瓶颈，未扩大到格索引改造。

## 5. 内存限制、warning与下一步

最终v3采用与before相同fresh→CPU→压力链，phase100/200匹配；WG槽位增量10085→85，
少10000个Graphics UID槽（约99.2%），与100个Graphics/tick×100tick对应。
GL原链出现缓存槽下降（16219→4759→9819槽，与自动GC机制一致但未记录GC事件），不把原净差冒充新增量；最终GL也是5079→5164（+85）。
这是分配/槽位减少，不是减少活跃节点或VRAM：两份几何缓存使匹配phase活context从639变719（+80）。

| 最终匹配资源窗口 | tick100→150→200 | before同phase资源 |
| --- | --- | --- |
| WG live contexts | 719→684→719 | 639→604→639 |
| GL live contexts | 719→684→719 | 639→604→639 |
| WG sources/估算纹素bytes | 85/187752264，各窗相同 | 85/187752264 |
| GL sources/估算纹素bytes | 83/175263560，各窗相同 | 83/175263560 |
| WG heap各窗5样本min–max MiB | 97.964–97.967 / 82.858–82.863 / 91.018–91.024 | 不作跨运行heap归因 |
| GL heap各窗5样本min–max MiB | 76.044–76.050 / 91.289–91.293 / 91.756–91.761 | 不作跨运行heap归因 |
| WG BindGroups/null slots | 141/26→145/34→151/44 | 窗口仍+10/+18 |
| GL BindGroups/null slots | 14/0，各窗相同 | 14/0 |

短窗中WG heap出现下降（未追踪GC事件）、GL heap上升；两者都不证明长期稳态。
ABBA之后另跑的v3压力窗口含RT池残留，125 sources/204455752 bytes不能与旧85 sources直接比VRAM。
其清空finite/dying/overlay均0，lighting members/privateSources回到1/1（地形基底）；destroy后0/0，
Graphics/context槽均0、独立租约0，仅全局WHITE/EMPTY两源8字节。
最终匹配链也复核上述回落：clear仍有1个地形Graphics context、13个初始化资源独立租约，
仅destroy后归零；feed仍由harness持有1房/300对象，feedPoolLeases=null表示此feed没有池接口，
不能把null写成0或称feed已销毁。clear后有限效果active/nodes/texts、dying/overlays均0。
官方312对象首帧画面另存[`official-room-final.png`](renderer-visual-reuse-data-2026-10-01/official-room-final.png)，
这是普通demo/真实回放画面；完整100tick回放与原版比较沿用
[上一阶段记录](renderer-r7-short-verification-2026-10-01.md)，不冒称本轮重跑。

BindGroup warning保持原日志与资源跟踪；cache中的null slots增长继续登记，
未把warning屏蔽、未将所有缓存风险销账。已归因的自有资源释放与Pixi批次缓存回调分开。
WG两份profile日志各取到500条warning、0 error，500是捕获上限而非总数；
GL profile链2条warning、0 error，fresh ABBA复测0条。原始日志完整保留在本次捕获范围内。
heap无forcedGC且窗口短，不能称内存无增长或全局内存“优秀”。

下一步优先保持冻结阈值复查整房GPU pass及快照长尾；新的lighting质量档、hover格索引、
detached对象池与新视觉特性未增加。原灰区确认仍有效，G3/G4/G5引擎侧缺口继续挂账。
长测和正式收口按用户暂缓状态保留，不勾选R7/M3最终验收。

## 6. 验证与评审记录

- renderer 376测（较黑屏提交370新增6测）、shared4测、kk-cli6 unit+1 integration全绿；
  renderer/shared tsc、shared JS+d.ts及renderer ESM+d.ts构建通过。
- cargo fmt --all --check、cargo clippy --workspace --all-targets -- -D warnings通过。
- 公共d.ts递归闭包10文件零Pixi，含动态import/子路径/裸包侧门守卫；perf桥不进入公共面。
- 六条资产/夹具生成链重跑，175文件前后SHA256逐字节一致，见
  [`regeneration.json`](renderer-visual-reuse-data-2026-10-01/regeneration.json)。官方checkout仅只读输入。
- perf模式Vite生产构建通过（输出/tmp，诊断页仍经双后端浏览器实际执行）；
  既有大chunk提示记录为harness打包提示，不更改性能门限。
- 86份仓库Markdown的357个本地链接目标均存在（不冒称外网或heading锚点验证），git diff --check通过；
  三路终审分别复核性能数值/资源、生命周期异常、
  像素与灰区措辞。终审结论追加于本节，阶段提交不冒称最终R7收口。


终审三路均通过，无阻断：

| 评审 | 独立复核与清偿 |
| --- | --- |
| lifecycle_review | 两份context所有权、监听脱离、异常释放/恢复及40次唯一几何数量上限；建议异常测试已补并通过 |
| memory_review | 独立复算47结果/105采样/16像素对、7源码hash、P1–P6、30次重建与匹配资源；补unique/GL复测及GC因果措辞 |
| replay_review | 16对解压RGBA零差异、撤回v2、表格/源码hash/复现汇总、本地链接与六文档；冻结面/灰区/暂停长测表述一致 |

未发现需要重新打开用户方向决策的变更；主树、官方checkout和引擎源保持未改。
