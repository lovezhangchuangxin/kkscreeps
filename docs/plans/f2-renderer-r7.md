# F2 R7：性能验收与整体收口方案

> 状态：**方案已批准，开发/实测中**（2026-09-30）。工作树
> `.worktrees/renderer/`，分支 `f2-renderer`，实现起点 `01558f0`。
> 方案提交 `b0a5dd1` 后用户确认全部推荐并授权开发；用户追加要求先保存阶段任务，
> 已提交 `821a02f`（工具/首轮数据/失败复现），随后修复 bug、优化性能；最终收口另提交。
> 阶段提交不冒充验收完成。
> 上位：[F2 方案](f2-renderer.md) §5/§7/§12；前序终态：
> [R6 方案](f2-renderer-r6.md) 最后一个 §8「执行记录」；方法学与历史数据：
> [R5 验证记录](../research/notes/renderer-r5-verification-2026-09-30.md)；
> 回写对象：[前端架构](../architecture/frontend.md)、[路线图](../roadmap.md)。

## 1. 范围、冻结面与入口复核

R7 做四块：满配房性能预算实测、八条移交测量、10k tick 内存 soak、遗留
销账及文档终清。上位 §5 数值是验收约束，不能放宽：主基准 WebGPU，1080p
满配房 300 对象（**包含** 50 creep）/100 visual/地形全类型，60fps、帧预算
16.6ms、渲染 ≤8ms、单 tick snapshot 对账 <2ms、首帧 <1s；标准房 2000
tick 循环回放 10k tick 内存无增长。WebGL 同场景跑通、如实记数，不设数值门。

不做世界地图（M4）、443 常量完整生成管线、G3 徽标自托管、新视觉特性或
引擎既有文件。建议夹具全部在 renderer 合成，不扩 exporter；若实测发现必须
扩 exporter，先提方向性变更，只读公开 API 并登记唯一修改文件。主树有引擎
未提交改动，**所有命令、文件修改和 git 操作都指定 renderer 工作树**。

R6 D1–D7 和 createRenderer 公共 API/d.ts 形态不重开。新增采样能力放内部
诊断模块、harness 和脚本，不增加公共选项、句柄或 stats 键，不复活 `__room`。
低档光照取舍见 D1；不能把 API 增量当成性能测量的便利手段。

入口实际复跑：renderer **350 测**（25 文件）、shared **4 测**、kk-cli **7 测**
全绿；两 TS 包类型检查、renderer 资产新鲜度、ESM+d.ts 构建及公共面 **7
文件零 pixi**守卫通过。工作树入场/复跑后均干净。这不是 R7 性能或内存证据；
本次未复跑 fmt/clippy、全量资产重生或双后端浏览器验收，留实现出口执行。

## 2. 现状与缺口核实（file:line 均相对本工作树、固定在 01558f0）

| 项 | 证据定位 | 已有能力 / R7 缺口 |
|---|---|---|
| 冻结预算与切片 | `docs/plans/f2-renderer.md:219`、`:266`、`:687` | 四项预算与八测量点明确；没有 R7 完整数据 |
| R6 最新终态 | `docs/plans/f2-renderer-r6.md:376`、`:390` | B1–B5 已修复，350 测；旧 322 测不是入口基线 |
| R5 CPU 冒烟 | `docs/research/notes/renderer-r5-verification-2026-09-30.md:69`、`:83` | M4/24GB、1080p、50 creep/4×；两后端 objects p95 0.5/0.6ms、lighting 0.5ms，未含主画面与 GPU 完成等待 |
| 缺完整夹具 | `packages/renderer/scripts/build-effect-fixtures.mjs:81`、`:82`；`packages/renderer/dev/main.ts:58`、`:78` | pressure=50 对象/50 creep/全平原/30 增量；visual-cases=53 visual、空对象房；最大现有 showcase 首态55对象。无300/50/100合并场景 |
| 阶段计时 | `packages/renderer/src/scenes/room.ts:159`、`:169`；`packages/renderer/src/index.ts:407`；`packages/renderer/dev/main.ts:29`、`:45` | onFrameTiming 只含 ObjectsStage.update、lighting 离屏 CPU 提交，playing 时采样至10000；无完整帧/主画面提交/GPU完成数据 |
| 分辨率与自动后端 | `packages/renderer/src/core/renderer.ts:33`、`:38`；`packages/renderer/dev/index.html:19` | stage随视口、DPR钳到2、偏好WebGPU可回退；必须记录逻辑/物理尺寸与**实际**后端，不能仅凭URL归类 |
| 地形全类型 | `packages/shared/src/feed.ts:28`；`packages/renderer/src/scenes/terrain.ts:124`、`:217` | 输入定义含lava=4；当前只按swamp/wall位生成层，lava单独输入无独立视觉，夹具覆盖并登记，不能新增视觉或宣称已绘lava |
| snapshot 与 notify | `packages/renderer/src/feed/room-feed.ts:86`、`:109`；`packages/renderer/src/index.ts:270` | feed置换Map、复制对象数组、同步触发场景对账/地形/visual/hover；<2ms不能只测Map构造 |
| visuals 重建 | `packages/renderer/src/feed/room-feed.ts:134`；`packages/renderer/src/scenes/visual-stage.ts:51`、`:68` | 每次setVisuals复制数组并notify，**再次对象对账**，visual全量拆建；只有跨普通delta的原引用能短路。需单独测100条每tick替换及下一帧文字纹理上传 |
| 节点复用与死亡 | `packages/renderer/src/objects/objects-stage.ts:189`、`:223`、`:262`、`:399`、`:577` | snapshot清瞬态、同id/type复用，缺失销毁；delta死亡进入dying。**无detached视图池**，不能把同id复用描述成已有视图池 |
| feed池/租约 | `packages/renderer/src/feed/room-feed.ts:21`、`:75`、`:109`；`packages/renderer/src/assets/atlas.ts:57`、`:66`、`:88` | feed是Map/数组，无池、无租约；已有租约是atlas URL页users/release。当前feed池租约记N/A（不存在），记录房/对象驻留数；若以后建池才记录容量/借还计数 |
| 部件值门控 | `packages/renderer/src/objects/objects-stage.ts:333`、`:370`、`:373` | 求值后valuesEqual挡join写入；没有累计evaluations/equal/null/write/tween等实测计数 |
| TweenManager | `packages/renderer/src/anim/tweens.ts:180`、`:205`、`:210`；`packages/renderer/src/objects/objects-stage.ts:125`、`:429` | size=active+pending，update无数组快照；规模/峰值/耗时无采样，finite播放器不属于此Map须另计 |
| lighting | `packages/renderer/src/core/lighting.ts:85`、`:107`、`:117`；`packages/renderer/src/core/terrain-spec.ts:8`、`:54` | normal真RT+MULTIPLY；dirty计数是**失效调用次数**，不等于dirty帧数；降级隐藏输出，未见真实触发。没有normal/low/disabled档位开关 |
| hover扫描 | `packages/renderer/src/index.ts:294`、`:306`、`:337`；`packages/renderer/src/feed/pick.ts:17` | 每次命中复制feed对象数组并O(n)扫描；格索引需要前后测量及语义锚，不能直接优化 |
| rampart VRAM | `packages/renderer/src/objects/objects-stage.ts:636`、`:661`、`:668`、`:675`；`packages/renderer/src/core/terrain-spec.ts:59` | 每用户组整房2048² RGBA烘焙私有源；每组基础纹素约16MiB（理论、不等于实际VRAM），随组数线性；bbox裁剪未做 |
| 复位基线 | `packages/renderer/test/objects-stage-effects.test.ts:94`、`:101`；R5验证记录`:80` | 8次单测回放/两轮浏览器资源回落，非10k；pressure末态finite=0，members=101/privateSources=1只适用该夹具，不应硬套满配房 |
| BindGroup warning | R5验证记录`:92`；`packages/renderer/src/core/lighting.ts:70`；`packages/renderer/src/scenes/visual-stage.ts:70` | R5创建栈指向Pixi批次缓存；资源销毁处多，R7仍需复现、绑定清除/缓存回落/后续帧证据，不能只靠历史文字销账 |
| d.ts守卫盲区 | `packages/renderer/scripts/build-lib.mjs:21`、`:33`、`:34`；R6方案`:344` | 正则只认精确pixi.js、单引号from相对路径；import()动态类型、pixi.js子路径、裸包类型侧门未覆盖；相对路径还应按当前声明目录解析 |
| 前端/roadmap陈旧 | `docs/architecture/frontend.md:81`、`:96`、`:98`、`:104`；`docs/roadmap.md:84` | 现状仍写R6未完成、playbackRate待冻结；示例setVisuals少room参数；roadmap M3是整条客户端范围，须拆出renderer子项勾选，不能把整个M3打勾 |

R6 A-J4/手势行为测试原移交项已在最后一次B2/B3清偿补齐（R6方案`:374`、
`:394`）；R7复核既有锚与双后端操作，不重复宣称仍未补测试。真实多指浏览器
验证与行为测试仍分开表述。

## 3. 场景与可复现数据设计

### 3.1 新建满配夹具（建议 renderer 合成，不改 Rust）

新增 `scripts/build-perf-fixtures.mjs` 和 `fixtures/perf-full-room.json`（均在
renderer包内，拍板后创建）。提交provenance/生成器版本/固定seed/源夹具引用/
摘要与SHA256；相同输入两次生成逐字节一致，校验首态与各预算tick恰为
300对象、50 creep、100有效visual，唯一id与房间隔离，不依赖随机数或墙钟。

- 300对象=50 creep+250其他对象。建议组成：road60、extension60、私有
  rampart40（4用户组各10）+公有10、constructedWall20、spawn3、tower6、
  link6、lab10、container10、storage2、terminal2、factory2、powerSpawn1、
  observer1、extractor1、nuker1、controller1、source2、mineral1、deposit1、
  keeperLair1、portal1、constructionSite8。合计250；结构/自然对象按现有
  描述符所需字段给完整数据（natural的type形状沿现有夹具），不把跳过对象凑数。
- 50 creep各50 body部件，含敌我、资源圆、名字、tough及掉血形态。定格和
  **50 creep全移**使用同一数量/镜头/seed；运动路径在可通行地形，边界往返
  不越界。动作来自明确标注的renderer actionLog/says合成通道，不宣称引擎
  能产出；去重与受击罩仍用现有语义。
- 100 visual=五原语各20（其中20文字含背景/描边），包含实线、dash/dotted、
  alpha及浮点坐标，分布覆盖房间可视区域，不能靠透明或离屏原语凑100。
  动态组每tick改变文字/几何并调用setVisuals，静态visual组只作拆解对比。
- 地形长度2500，包含plain/wall/swamp；lava位也纳入输入与计数（见shared
  契约位掩码）。已有渲染器对lava没有独立视觉，不新增视觉，单独登记；terrain
  源标为合成全类型布局，不能把swamp-walls现有[0,1,2]说成覆盖lava。
- 数据主键加`r7-perf-`命名空间；成本对比房用独立房名/id域。snapshot始终
  全量生成当前位置，跨快照即时同步且清瞬态，保留同id/type节点复用机制。

### 3.2 测量矩阵

| 组 | 输入 | 用途 |
|---|---|---|
| F0 | 满配、creep定格、100 visual每tick替换、normal lighting | 非运动成本及ambient/visual基线 |
| F1（主预算） | 同满配、50 creep全移+50去重动作/受击、100 visual每tick替换，1×/4× | 主预算与R5同倍率对比；两倍率都记数，不能择优 |
| F2 | 同id/type重复snapshot；独立id空间换房全拆建；地形切换三条分开 | <2ms对账与snapshot/切房单帧尖峰，不拿复用冒充冷重建 |
| F3 | F1的visual同引用通知 vs 每tick100条全替换 | 分解重建/下一帧文本上传成本，不替代F1预算 |
| H | 300对象固定输入轨迹，按格命中/持续指针移动/数据与相机重估 | hover O(n)热点；输入成本与RAF节奏同时记 |
| R | 私有rampart总数40不变，用户组1/4/16/40，稀疏/集中两布局 | 全房源纹素/估算bytes、重烘焙时间、cache/销毁；不替代主满配夹具 |
| S | 标准房2000-tick循环×5；满配持续delta压力另跑10k | 主soak与dying/overlays/visual私有源压力验证，详见§5 |

所有组同cell=100/固定fitRoom镜头；主预算1080p指物理canvas1920×1080，
记录CSS尺寸/DPR/Pixi resolution。优先用DPR=1的独立浏览器配置获得CSS与
物理均1920×1080；做不到则如实登记并请求处置，不改公共resolution参数或
把3840×2160结果当1080p。补充缩放/离屏用例可以验证剔除，不能取代全房可见。

## 4. 测量方法与八条出数表

### 4.1 采样环境、完整帧与启动

实现时读取本机型号/芯片/内存、OS、浏览器与Pixi版本、实际backend/adapter、
电源与显示刷新率、DPR/分辨率、构建mode、仓库HEAD、夹具hash。R5的
MacBook Air M4/24GB仅作历史来源，不能代替本轮环境实录。浏览器保持前台，
记录visibility与采样时间；隐藏/设备丢失的整轮标无效保留原数据并重跑，不能
丢弃慢帧。性能轮不强制GC，关闭实时HUD更新，记录诊断开销的开启/关闭对照。

主harness扩展 `?smoke=1&perf=1&fixture=perf-full-room&backend=…`：沿用
onFrameTiming，objects=仅ObjectsStage.update，lighting=离屏CPU提交（无
GPU等待、无terrain滚动）。新增内部探针仅在perf构建启用，由harness读取
普通数据；正常lib构建消除探针，公共API与声明形态不变。输出JSONL/CSV原始
样本，时间单位ms，分位使用明确的nearest-rank定义；R5旧HUD的floor索引
历史口径另外注明，不能静默改写历史数字。

每backend/组/倍率预热至少120帧后采至少1800实际渲染帧、重复3轮；无输入
帧也保留，同时标tick到达/visual替换/RT重渲。输出n/p50/p95/p99/max及预算
超限次数。预热样本另存，snapshot/启动/重建尖峰独立测试，不通过预热抹掉。

完整帧分三条证据链，不能用objects+lighting或HUD fps替代：

1. RAF/实际主画面提交时间戳，计算区间fps、帧间隔、长帧/跳帧；记录显示器
   vsync节奏与16.6ms原文预算，60Hz约16.667ms的展示精度不能靠HUD四舍五入
   冒称满足。出原始值并在D4处置，测量噪声不能自动当豁免。
2. CPU：分段统计terrain、objects、lighting、主场景render提交与完整渲染
   CPU总时间；feed推送/visual重建另记，并关联到受影响的下一帧。计时必须
   只包本次提交，不把提交回调误当GPU完成。核对Pixi ticker优先级，确认
   同一帧只提交一次主场景，避免测量器额外render改变负载。
3. GPU：优先取实际提交的timestamp-query/后端timer query或浏览器GPU
   trace，记录支持能力、有效/失效样本与覆盖pass。若缺精确计时，另跑
   **上一帧队列清空→本帧更新+lighting+主画面→GPU完成**的串行端到端采样
   （含CPU/调度等待的保守上界，不能名为纯GPU耗时，也不能拿串行fps替代
   正常RAF）；WebGPU队列完成、WebGL fence的具体接线在本版本源码复核后
   落地。只包主画面而漏lighting/上传的trace不能作≤8ms证据。精确数据
   不可得且上界超预算则登记未证实，停问D4，不能凭阶段CPU宣布完整达标。

冻结阈值按原值比较，分位用于描述，**不私自将“≤8ms/<2ms/<1s”改成仅p95
达标**。报告保留max/超限与成因，超限或能力未证实先提D4，用户确认降级记录
后才关账；WebGL照样记录但不设数值门。

启动独立冷/热HTTP缓存各10轮，模块加载后createRenderer开始、初始化结束、
atlas/font就绪、首snapshot+100visual、完整首次主画面提交/呈现分别标时；
主门是**开始createRenderer至首个满配画面可见**<1s（含WebGPU初始化、资源
装载、地形/visual与首render）。导航至首帧作为附加值，不能把空画面或Promise
resolve当满配首帧。采用浏览器帧trace/截图核验首呈现；没有呈现证据则仅报
提交时间，不自证<1s。无atlas热预加载、无fonts fallback偷换正常负载。

### 4.2 八条移交点逐项输出（两backend同口径）

| # | 数据与拆分 | 采样/校验 |
|---|---|---|
| P1 lighting RT耗时/dirty比例 | lighting全部帧/实际重渲帧各p50/p95/max；renders、invalidations、dirty帧/总帧、renders/总帧、invalidations/renders | F0/F1；invalidations不是dirty帧，CPU提交与GPU相关pass分列 |
| P2 apply对账分项 | FeedStore映射/数组、notify场景、walls/roads/ramparts/controller、wanted/removal、parts求值/写入、动作解析/ambient、visual、hover；总snapshot/delta/setVisuals | 包住同步公共feed调用直到返回；snapshot总值是<2ms主数，100visual恢复另列并另报合计，防快照清空visual缩小场景；不能以delta代替snapshot |
| P3 双后端含50全移对比 | F0/F1在1×/4×的RAF fps、完整CPU/GPU或完成上界、objects/lighting、掉帧；三轮范围 | normal/lighting=real，计数300/50/100截图佐证；主预算取F1完整结果，不只抄R5数字 |
| P4 TweenManager规模 | active/pending/合计时序、p95/峰值、update耗时、创建/结束/取消计数；finite/ambient分开 | F1/F2；快照/大步进后瞬态回零，ambient可保留 |
| P5 snapshot重建单帧尖峰 | 同id复用、换id全拆建、换地形分别apply时间/下一帧CPU与GPU完成、创建/销毁数量 | 每条至少30次，附首次样本与max；同id位跳无补间，identity断言，地形门控语义保持 |
| P6 值门控触发 | evaluations/null/equal/changed、join调用、ValueTween逐帧write、lighting失效按来源 | F0/F1/相同delta/停用→恢复；按类型/part累计，补证明稳定值无join写；采计数不能改变'visible'布尔值 |
| P7 10k内存曲线 | post-GC heap/未GC窗口、私有Texture/TextureSource唯一数与bytes、nodes/text、dying/overlays、feed驻留、atlas租约 | S组见§5；lighting.privateSources只是ownedMembers+RT推导数，不能代替全树/全实例资源账本 |
| P8 大步进deltaMS恢复 | 0/16.667/250/1000/10000ms单次推进耗时、最终位置/旋转/alpha、finite/dying回收/lighting一致、恢复首帧尖峰 | 手动advanceAnimation与真实ticker长帧分别验证；与等总时长小步终态对照，嵌套effects新建按实际语义解释差异；暂停0真no-op |

内部分项不改变公开回调定义；分项和总值核对，解释嵌套包含关系，不能相加
重复计算。采样器有限容量/在线汇总，不能把10k tick全部快照、纹理或树节点
塞入日志造成测量器自身泄漏。正常构建与带探针构建都跑回归和对照。

## 5. 10k tick 内存 soak（完整执行后才能下结论）

两backend各做主标准房与压力链，先各预热1000 tick（不算入正式10k）。
使用真实Pixi renderer/atlas/font/渲染提交，不用Vitest mock替代浏览器soak。

主链以standard.json的真实首态/30增量为来源，生成确定性的2000-tick循环
适配器：每个原30tick片段重放前用全量snapshot复位，逻辑time连续单调，
对绝对deadline作相同tick偏移；2000边界也做全量复位。循环5次=10000正式
tick，登记每个复位位置和来源，不称其为2000条引擎原生导出记录。

标准链之外再跑满配10k持续delta链：有界id域，固定频率死亡/淡出/新生、
说话替换、工作/攻击overlay、100visual文字重建、rampart用户组/地形变动；
让删除→dying→自然销毁充分发生，**不能每tick snapshot清场掩盖泄漏**。
记录死亡中峰值与回落；检查id复用先收旧dying的防御。

加速soak采用固定逻辑步进：每tick=1000动画ms，拆15步（每步1000/15ms，
对应4×/60Hz每帧动画量），每步实际更新并提交主render；每tick等待已提交
GPU工作完成，再yield，让队列不无限积压。墙钟加速、不报告此链为实测fps；
总计10000次数据tick、150000次正常动画/主render步（checkpoint额外步单列）。
末尾补真实RAF的连续回放/复位检查，防只有手动步进路径过关。禁止一次推进
10000 tick后只render一帧来冒充soak。

在正式tick 0/1000/…/10000固定相位采样：暂停推送，不先reset，不改变存活
场景；等待GPU完成，在同一自然排空步骤后取资源数与heap。未GC窗口另每
100tick取heap min/median/max。标准链按2000周期相位分组：0/2000/…/10000
相互比较，1000/3000/…/9000相互比较，不混两组拟合；压力生成器设有界
100-tick状态周期，使1000tick checkpoint同相位。checkpoint用浏览器DevTools协议或显式可用的
GC进行双次full GC（间隔两个任务/帧），之后取5次heap样本中位数。记录API、
是否强制GC、延迟、浏览器缓存行为；若当前浏览器无GC/准确heap接口，使用
独立可诊断Chromium实例并记录环境差异，仍不通则停问D4，不用粗糙heap值
自称稳态。GC窗口从性能采样剔离并明确记录，普通回放不强制GC。

账本计**唯一**Texture/TextureSource，共享页只算一次；保留source销毁标记
与弱引用/标量id，不能以强引用计数器把已销毁资源留下。输出基础纹素估算
bytes（标为估算，实际GPU分配能取则另列）、文本源、地形/顶罩源、RT、Pixi
缓存条目/可达链。feed池租约N/A及房/对象数、atlas共享页users随挂载/卸载
回落分列；若D2引入视图池，新增pool capacity/leased/free并验证有界。

checkpoint不reset；标准链每2000tick周期结束另做R5复位基线验证；压力链
中途不加snapshot/reset，正式10k完成后才做复位。两链末尾再空房/destroy
检查：finite active/nodes/texts=0、dying=0、瞬态tweens=0、lighting
members/privateSources回到**该夹具预热后同相位基线**。标准房与满配的常驻
ambient不要求0；空房预期ambient一成员，已分配RT仍可能一源；destroy后
零自有源/租约释放，以内部账本交叉核对，不能只依赖getStats销毁后硬编码0。

归档tick→post-GC heap曲线和资源曲线（CSV/JSON+静态SVG/PNG），每周期
差值与拟合斜率、首末窗口、最大驻留、保留链。预先测同实例等采样/渲染的
空闲控制窗口，以其噪声解释heap抖动；**不另设“允许增长5%”等宽限门**。
判断无增长须同时有同相位存活资源/租约回落、GC后heap无持续上升与无新增
保留链证据；出现增长或测量不确定则登记幅度/斜率/嫌疑所有权并提D4。

## 6. 遗留清偿、守卫与文档

### 6.1 BindGroup warning正式销账所需证据

本次只做源证据复核，尚未下运行时终裁。通过 `gh api contents` 读取
PixiJS `v8.21.0` 的 `src/rendering/renderers/gpu/shader/BindGroup.ts`；与安装
包 `lib/rendering/renderers/gpu/shader/BindGroup.mjs:84–94` 一致：资源销毁
回调会将匹配slot置null、dirty=true，随后warning。这支持R5所述清绑定机制，
**不单独证明我方没有悬挂引用或缓存增长**。
源指针：[BindGroup.ts 固定版本](https://github.com/pixijs/pixijs/blob/v8.21.0/src/rendering/renderers/gpu/shader/BindGroup.ts)。

实现时保留原warning，执行地形替换/visual文本替换/切房/销毁再挂载最小复现，
输出警告次数、创建/触发栈、对应source id、slot清空前后、下一帧真实render
结果与cache驻留。用soak/挂载循环证据判断：若只有Pixi批次cache的正常销毁
通知、slot清空且后续无引用/增长/渲染错误，建议按“上游诊断噪声、非自有
泄漏”正式销账并保留日志；若有我方仍消费源，修释放/提交顺序，再双后端
复测。禁止屏蔽日志、改依赖私有缓存或仅见不到warning就关账。

### 6.2 d.ts闭包守卫加固（明确在本片实现）

使用已安装TypeScript解析声明AST取代正则。本机是7.0.2原生编译器，已核实
其package exports提供`typescript/unstable/sync`会话API与`unstable/ast`，
Program.getSourceFile可读AST；不存在旧`lib/typescript.js`，不能按TS5的
createSourceFile/resolveModuleName直接调用。先做该版本API能力探针，保持
现有依赖，不另引第二套编译器；若无法完整解析则提出方案修订，不退回宽松
正则。从公共index.d.ts出发遍历
import/export/ImportTypeNode/引用指令及其模块解析闭包，按当前文件目录解析
相对路径、单双引号、扩展名与index声明；循环去重、未能解析即fail closed。

- `import('pixi.js').Texture`及动态相对类型必须发现并递归。
- `pixi.js/*`、`@pixi/*`和精确`pixi.js`均拒绝。
- 裸包引用不作为递归终点豁免：解析该包声明/本仓shared源声明闭包继续检查，
  不明外部包/解析失败报错；以临时类型包转出Pixi作侧门负向用例。
- 正例：现有公共7文件、shared零Pixi类型、双引号/嵌套相对/循环引用；
  非公共可达的内部Pixi声明仍可存在，不误把全dist零Pixi当冻结要求。
- 三盲区各有“原守卫能逃过、新守卫拒绝”负向实跑；build链必须调用新守卫，
  不能测试另一个未接线实现。公共API/d.ts shape对照R6基线。

### 6.3 优化证据与范围闸门（D2）

仅先测三候选。为每项提供基线成本/峰值/资源数、负载、因果定位、候选实现
边界与风险后问用户是否做；方案批准不默认批准其中任何一项。批准后同环境
AB或ABBA复测三轮，报告绝对/相对差异与轮间波动；无可重复收益就回退。

| 候选 | 需要的证据 | 若获准的最小边界 |
|---|---|---|
| hover格索引 | H组O(n)+数组复制的占比、每移动/相机/数据重估成本与帧影响 | 内部格→候选索引，保持数据格/z序/id平手、离开/拖拽/切房失效；增量维护成本计入apply |
| detached视图池 | dying/重建分配、创建销毁与GC成本确为热点 | 有界按type池、借还与释放账本；重置prev/tween/overlay/文字/owner，snapshot即时同步语义不改；不默认引入feed池 |
| rampart bbox裁剪 | R组bytes随user线性与实测重烘焙/内存压力 | 仅缩烘焙范围，外扩含描边/抗锯齿，维持坐标、alpha0.4/ADD、码点类排序、hash与所有权；双后端边缘截图防裁掉外环 |

若未获准或没有证据，代码维持原样，报告列数字及留项原因；不能以优化待办
未做来隐去不达标。扩大到其他优化、LOD/档位、改采样口径都先问用户。

### 6.4 四份文档终清与登记

1. `frontend.md`现状改R0–R7终态，修setVisuals(room,v)示例和冻结措辞，
   明确同id复用/视图池实际状态、RoomVisual与交互/字体/API、性能达标或
   降级指针。世界地图/LOD/Vue封装仍为后续目标。
2. `roadmap.md` M3客户端大项拆出“renderer房间观战库（F2 R0–R7）”完成
   子项，附灰区与性能结果；不勾选网络、账号、Vue客户端、M4地图。
3. 上位`f2-renderer.md` §12新增R7完整条目，写八点/soak/BindGroup/守卫/
   优化决策/三路评审与数据入口；§6灰区表仅依用户已确认裁定回写。
4. `f2-renderer-r6.md`执行记录末加R7继承与最终验证记录指针，保留R6历史
   数字及勘误，不追改其已冻结决策。

另建 `docs/research/notes/renderer-r7-verification-2026-09-30.md` 与同名
数据目录（实际采样跨日如实标时间）；采样环境、完整方法、命令/URL/seed/
hash、所有原始数据、曲线、截图/trace索引、差距及复测不能只放本机artifact。
大trace可记录持久证据位置与SHA256，关键原始数值入库，可由评审复算。
R5已有差异（flare3缺实现、孵化quirk、填充白名单、say像素/寿命、owner取消）
逐条映射到终态保真登记，禁止在末片静默丢失或顺手增加视觉。
shared当前TS源直发、`.js`相对引用仅type-only的运行边界也在frontend补记。

## 7. 决策点与灰区逐条裁定建议（均待用户确认）

### 7.1 范围/处置决策

| 决策 | 建议 | 拍板含义 |
|---|---|---|
| D1 lighting normal/low/disabled补否 | **本片保留normal，不补low/disabled**，登记质量档位未实现 | 原三档不是已实现能力；新增档位涉及视觉/范围，且无公开选择API。若要求补，需先取官方仓库源确定内部/harness档位语义，公共API仍不重开；不能拿low/disabled达标替代normal |
| D2 三项优化边界 | **先测，逐项带证据再问是否实施**；无收益回退 | 当前不预批hover索引/视图池/bbox裁剪；不扩大优化清单 |
| D3 灰区终裁 | **逐条按下表建议确认**；G2的最终销账以双后端/soak证据成立为条件 | 此时只批准拟议终态；证据相反先报告并重问，最终收口展示实际G1–G13表供确认 |
| D4 任一指标未达/测量能力缺失 | **阈值保持冻结，逐项列差距/原因/可选后续，停下来问处置** | 不以p95、低档、少visual或短soak凑达标；登记降级需用户确认，可选做已证据驱动优化或保留后续项，不能自动降级关账 |
| D5 夹具/采样设计 | **renderer合成满配+标准循环适配器，生产API不变**，按§3–§5执行 | 合成与真实源分开；完整渲染含GPU完成证据，10k实际tick/渲染，不把历史冒烟当验收 |

### 7.2 G1–G13逐条终裁建议

当前事实来自上位方案`:243–255`及R6最新清偿；以下不是未经确认的灰区表
改写。G1按用户所说“四项黑盒面”归类：文字（含缺省字号）、端帽、线型节奏、
非基础CSS颜色；单独保留已有层位推断，不能因归类而消失。

| 项 | 建议终态与拟回写措辞 | 复核/保留理由 |
|---|---|---|
| G1 | **保留收窄黑盒**：①无背景text底边近似基线（descender微差）及未明载font缺省0.8；②round端帽；③宽度感知dash/dotted节奏；④非基础web色（如color()/lab()/var()）当前回退。另记visual层位在effects上且不受lighting为壳行为推断 | 默认radius/宽度/opacity/padding、坐标、font四形态、基础颜色/HSL token已明载或经浏览器实证，不再挂成未实现；any web format是官方承诺，非基础色是我方覆盖缺口，不能写成官方不支持 |
| G2 | **建议条件销账**：本机WebGPU/WebGL normal真离屏RT+MULTIPLY压暗通过，未发现能力缺口；降级路径作为防御保留 | G2定义是全局压暗能力，不是BindGroup warning/质量档位总桶；需本轮双后端截图/real与输出正确证据，人工注入异常只证明fallback可达，不能称真实后端已降级 |
| G3 | **保留M3缺口**：徽标纯色圆退化、真实图源/账号参数未落位 | 不做自托管，不把v0退化当完整徽标支持 |
| G4 | **保留引擎投影缺口**：says renderer链已实现、隐私/寿命已验证，真实say输入待引擎 | 合成数据不抵真实通道；player=null私有全滤保留 |
| G5 | **保留输入/结算缺口**：六类真实事件、其他合成家族明确列出；远攻/群攻处理器及lab/power/say/nuke真实输入挂账，PWR16–18可渲染，19+缺映射计skipped | `_all.hasEffect flare3`未实现是renderer视觉缺口，**另列保真遗留**，不甩成引擎缺口、不在R7补新视觉 |
| G6 | **保留参数化默认差异**：tickDurationMs缺省1000，仅动画时序；setPlaybackRate保进度，推送归调用方 | D1–D7已冻结；不宣称1000等于官方服务器tick |
| G7 | **保留采样近似，字体责任已解决**：Roboto库预载三规格、loaded/fallback/unset可验证；束端裁切/nuke四档blur/固定seed flicker与初始角仍登记 | 不用字体loaded销掉栅格化/构建期近似；孵化/填充/say等R5偏差逐条保真登记，既定owner契约不重开 |
| G8 | **维持R2已销账**：swamp mask按官方层序正常，原观察由跳格bug污染 | 回归双后端混合地形，不重新挂旧假缺口 |
| G9 | **保留M3数据源缺口**：当前userOwnerColor demo色板、私有顶罩/公有rampart不同色路径、localPlayerId已冻结；users远程用户色映射未有 | 明确不是renderer已经消费真实users.color；不扩大引擎修改 |
| G10 | **维持保真登记**：factory两处官方\|\|优先级行为已复刻，上游变更时随动 | 不在R7按设计意图修官方quirk；复核现有值测试锚 |
| G11 | **维持防御偏差**：未知mineralType退O灰对，正常引擎态不可达 | 保留脏数据防御，不说源码完全一致 |
| G12 | **维持保真登记**：creep资源白圆resourcesTotal>0即显，沿官方优先级quirk | 测试锁当前语义；不顺手改布尔链 |
| G13 | **保留交互辅助差异**：白高亮环非官方形态，官方信息卡属客户端应用层 | 命中/hover契约与形态分别陈述，不承诺官方信息卡或高亮视觉对齐 |

灰区表不是全对齐保证。R4平滑重触发/固定部件序/罩重触发取消、R5各项观感
与输入适配差异也要保留原记录指针，不能在最终“总体完成”措辞中抹掉。

## 8. 分步实现、验证与评审门禁（拍板后执行）

| 步 | 工作 | 出口 / 停点 |
|---|---|---|
| S0 | 本方案提交 | **停等用户确认§7**；本次到此 |
| S1 | 满配生成器/断言/重生hash、perf内部探针与数据导出、d.ts守卫三盲区 | 不改公共API/引擎；完整计数/守卫负例通过，探针开关成本可见 |
| S2 | F0–F3/H/R双后端三轮、冷/热首帧、八点除soak的原始数 | 对任一不达/不可测提交D4分析；D2候选逐项问，未经拍板不优化 |
| S3 | 获准优化→同环境复测、无差异回退；BindGroup复现/所有权证据 | 每项处置及AB数据齐全；方向变化再停问 |
| S4 | 双后端两链10k soak与P7曲线、真实RAF复位/长帧人工验证 | 实际10000 tick计数、heap/GC方法、资源/租约回落，不能仅mock |
| S5 | renderer/shared全测、tsc、kk-cli test、fmt/clippy、资产与构建/链接自验 | 完整自验绿、数据与证据可复算，再送评审 |
| S6 | **3路subagent评审**：A性能方法/环境/数据复算，B优化/生命周期/冻结面，C文档/灰区/范围终裁 | 实际派三路（本会话共4并发，低于用户≤6），不以单代理复核替代；用户终裁不委托agent |
| S7 | 清偿→每路复审（必要时重复）→最终灰区表/降级清单请用户确认 | 所有阻塞核销、无未批准方向变化；修代码后重跑受影响数据及出口检查 |
| S8 | 四文档回写、数据环境方法归档、最后diff/链接检查、单一收口提交 | §9全部齐；subject“前缀：描述”，冒号前≤48字符、总长≤350，提交body附环境/场景/采样方法与报告指针 |

自验命令均以`rtk`前缀、显式工作目录运行：

- `rtk pnpm --filter @kkscreeps/renderer --filter @kkscreeps/shared test`：基线
  350+4零回归、新增另计；renderer test含typecheck/资产新鲜度/lib构建。
- `rtk cargo test -p kk-cli`（7基线）、`rtk cargo fmt --all --check`、
  `rtk cargo clippy -p kk-cli --all-targets -- -D warnings`；只检查不fmt改引擎。
- renderer+shared各自tsc；renderer ESM+d.ts构建+真实闭包守卫。shared当前
  **无build脚本、TS源直发**，为“两包构建”另用现有tsc在临时目录输出src的
  JS+d.ts并验证类型入口（独立构建配置/覆盖noEmit，不改其发布形态）；不能
  将shared typecheck或harness构建冒称第二个库包构建。harness生产构建另验。
- 资产27个既有生成文件重生前后SHA256一致（动态按清单核对实际数量），
  build-fixtures/build-showcase/build-effect及新perf生成器各重生一致；只允许
  新增perf产物，不让旧夹具/资产漂移未登记。任何生成器写入后立即git diff。
- `rtk proxy node scripts/check-doc-links.mjs`、`rtk git diff --check`；两后端
  harness人工验证满配计数、normal lighting=real、移动/死亡/visual/hover/
  快照/切房/大步进恢复，截图附trace时间点。

每次Edit后立即git diff逐行核对只有预期。布尔过滤链改写须列旧/新真值表
（特别player=null）；PartValues.visible为boolean；快照全量当前状态防幻影
移动；children活数组断言节点身份；类排序码点比较不用localeCompare；
transparent alpha=0无层；HSL数字S/L按0–100标尺。出现CSS语义争议用实际
浏览器getComputedStyle仲裁、保留输入/输出；官方文档继续只用GitHub仓库
源`gh api contents`，固定ref/path，不抓整页站点锚点。

## 9. 收口验收与执行记录占位

必须同时满足：四项§5指标各有实测结论（全达标，或差距数字/原因/后续选项
及用户批准的逐项降级）；八测量点齐数；实际10k soak及曲线/GC/资源/租约
证据；BindGroup/三档/三优化/守卫/保真遗留逐条有处置；四文档回写；灰区
G1–G13最终表经用户确认；三路评审清偿复审完成；单一R7收口提交。

记录模板：日期/HEAD/环境/命令与场景hash → 原始数据/报告 → D1–D5及各
优化/降级/灰区确认 → 自验 → 三路评审 → 清偿复测/复审 → 四文档回写 →
最终提交。本次只核实入口与写方案，**没有任何R7指标已达标或内存稳态结论**。


## 9. 当前执行记录（收口时补全）

- `b0a5dd1` 方案提交后用户批准全部推荐并授权开发；lighting 保持 normal，不新增质量档/API。
- D4 数值差距处置用户确认：冻结阈值不变，完成三轮/双后端/10k 后逐项登记，不择优取样。
- 用户追加“先提交这一轮任务，然后修复bug，优化性能”：阶段提交 `821a02f` 保存数据与真实 Graphics 失败回归。
- 已修复自有 Graphics context 精确释放；rampart bbox 裁剪有纹素实测/32 raster 零差异依据。
  GPU 600 帧同输入 p95 没有改善，不宣称帧提速；保留理由是纹素内存节省。
- G1–G13 逐条建议后，用户已整表确认，终态见 [灰区裁定](../research/notes/renderer-r7-gray-decisions-2026-09-30.md)。
- 验证、原始曲线、降级差距、三路评审与清偿结论统一归档 [R7 验证记录](../research/notes/renderer-r7-verification-2026-09-30.md)。


### 2026-10-01 用户授权的短测与真实回放阶段

用户暂缓新的长时间测试，授权无需再询问的持续 bug 修复与有证据的性能优化；
借用官方 checkout 的房间回放只读数据搭建完整真实场景并对比。原 §5 阈值、
公共 API、R6 决策与既有灰区终裁不变。模糊/锯齿专项暂缓，不新增装饰视觉。

本阶段短测采用满配 300/50/100 CPU 对账与压力链 100 tick 预热+100 tick 检查，
同一 100 tick 周期阶段比较资源/池/缓存计数；浏览器无法强制 GC 时 heap 只记录
自然采样，不冒称保留 heap 或 10k 无增长。真实官方回放与满配验收夹具分别登记。
实现/自验后沿用三路实际评审、清偿与复审，作为阶段提交，不冒称 R7 最终收口。

本阶段实现/数据/三路清偿复审/自验见[2026-10-01短测报告](../research/notes/renderer-r7-short-verification-2026-10-01.md)。
369+4+7测试通过，175文件生成链一致；真实回放与预算夹具分开，短测结果不替代§9收口定义。

### 2026-10-01 放大黑屏提交后的 RoomVisual 优化

黑屏修复已提交 `e7e30db`。用户继续授权 renderer 性能/内存开发且保持暂缓长测。
本轮针对实测每tick100个Graphics context UID新增，按槽位复用节点，
使用最多两份私有绝对坐标几何context LRU与文字背景值门控；删除/变类即时释放，
不引入detached对象池或公共API。单纯clear复用的提交回归保留记录，
局部坐标平移因RGBA差异已撤回；最终方案只在已有短数据支持的范围保留。
还修复perf checkpoint嵌套计数的可变引用，补GPU完成上界、快照创建/销毁数和短测入口。

原阈值、gray终裁、normal光照不变。全房GPU与快照长尾、完整三轮矩阵、
真实ticker stall及10k稳态仍不可用短窗口替代；状态见
[RoomVisual短测记录](../research/notes/renderer-visual-reuse-verification-2026-10-01.md)。
