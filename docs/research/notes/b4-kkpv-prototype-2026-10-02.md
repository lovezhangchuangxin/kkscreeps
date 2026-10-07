# B.4 KKPV 二进制投影原型（2026-10-02）

> 切片：[mod-plane-batches §3 B 行](../../plans/archive/mod-plane-batches.md)、
> [domain-projection §8 B.4](../../designs/domain-projection.md)。
> 判据：逐字段/缺失/null 对照；畸形数据拒绝；旧引用/慢工作者；缓冲区与权限测试。
> 基线：f65653e（B.3 收口后）。本片不改任何生产 tick 路径——协调器/沙箱
> 接线与测量归 B.5，因此无 perf A/B；棘轮与 bot 复跑证明默认路径零变化。

## 交付

三个新模块 + 一次行为保持重构，全部为库代码与测试：

| 模块 | 职责 |
|---|---|
| `kk-schema/src/kkpv.rs`（~1080 行含测试） | 版本 1 完整编解码：160 字节头、按 ID 升序对象目录、按类型分桶的列式字段（键目录+缺失/null 位图+定宽列或 arena 描述符）、惰性视图 `KkpvView`、公开/私有块 `merge` |
| `kk-world/src/kkpv.rs`（~560 行含测试） | 权威世界 → 每注册房间一个块输入：可见性与 `world_json` 同源（共用抽取出的三个辅助）、记录 ↔ `KkpvValue` 转换、`Ref` 编码的精确往返、视图物化回 JSON 记录 |
| `kk-runtime/src/kkpv_store.rs`（~300 行含测试） | 不可变块句柄库：按 `RunEpoch` 发布（先解码校验、代际须严格前进）、`acquire` 只命中当前代际、`retention_report` 报告保留代际/字节/句柄数 |
| `world_json.rs` 重构 | 抽取 `room_visible_to` / `object_exported_for` / `ext_entries_json` 为 pub(crate)，JSON 投影与 KKPV 桥共用（"不另写权限表"的结构性落实） |

## 布局实现要点（原型决策）

- 头部 160 字节按设计 §6 逐字段落位；四段（terrain/目录/类型桶/arena）
  块相对，room_name 与变长描述符 arena 相对；reserved 16 字节写零、
  读侧拒绝非零；flags 仅 bit0=私有，未知位拒绝。
- 类型桶：`类型键描述符 + rows + 字段数 + 字段条目(键描述符/kind/填充3)
  + 缺失位图×字段数 + null位图×字段数 + 依序定宽列`。桶序=类型键字典序，
  类型句柄=桶序号；字段键字典序由读侧校验（KeyOrder 拒绝）。
- 列类型 8 种：Bool/Int/Uint/Float/Ref 定宽（1/8/8/8/8），
  Str/Array/Object 为 arena 描述符（8 字节 offset+length）。
- 嵌套载荷用单字节标签自描述（0=null..8=Object）——设计 §6 未规定
  嵌套物理形态，此为原型选定，版本 1 内有效。
- 缺失/null/零/空集合分别编码：缺失与 null 在位图（同行双置位拒绝），
  零是 Int(0) 值、空串/空数组/空记录是长度为 0 的变长值。
- 编码确定性：对象按 ID 排序、桶/字段/记录键按字典序、载荷按桶/字段/行
  序入 arena——同输入不同提交顺序产出相同字节（有测试锁定）。
- 读侧建立视图时校验：魔数/版本/flags/byte_len/header_len/reserved、
  段边界与互斥（含与头部）、目录严格升序与句柄/行关系、桶结构恰好消费、
  键 UTF-8 与升序、位图互斥（全行不限于目录引用行）、Float 列 NaN/Inf、
  Bool 列字节域、变长描述符 arena 边界、全部限额；变长值内的字符串
  在读取时校验 UTF-8（惰性契约），读取后要求载荷恰好消费完。

## 原型登记的简化（B.5/C 前须收敛）

1. **列类型从数据推断**：编码端按字段首个非 null 值推断列类型（全 null
   字段落为 Bool 占位列）。设计意图是列类型来自冻结计划的字段 schema；
   接线时改为计划驱动，推断逻辑退化为防御校验。
2. **桥只产私有块**：公开块/overlay 的内容划分策略（哪些对象/字段对
   受众全体公开）需要投影计划参与，本片以 flags+merge 的机制交付，
   划分策略随 B.5 接线定。当前每查看者每房一块（私有），与 JSON 参考
   完全同形，对照测试因此可逐字段断言。
3. **房间级授权通道不入块**：energyAvailable/events/市场/GCL/RawMemory
   走各自通道（设计 §5），本片块内只有 room_name/terrain/对象。
4. **ext ObjectRef 不转 Ref 编码**：桥只对 `object_json` 确定产生的
   `Tombstone.creepId` 与 `Ruin.structure.id` 做 Ref 转换（两者都来自
   `ObjId(u64).as_str()`，往返无损）；扩展字段的 ObjectRef 值仍是字符串
   ——目录 schema 有类型信息，接线时可精确转换，此处不猜。
5. **audience_hash 由宿主传入**：桥不计算摘要（kk-world 无 sha2 依赖，
   且摘要语义本就归宿主定义）。

## 验证

- 工作区 461 通过、0 失败（+25：kk-schema 18、kk-world 3、kk-runtime 4；
  含清偿后新增的 5 个钉子与 ext 覆盖回归）；
  clippy --workspace --all-targets 0 error；fmt 干净。
- kk-schema 18 项（含清偿后 5 个钉子）：元数据往返、全类型往返、缺失/null/零/空互异、
  确定性（乱序输入同字节、audience 参与字节）、跨类型目录 ID 升序、
  地形往返、编码拒绝 ×9（重复 ID/重复键/类型不一致/id 键/NaN/深度/
  记录重复键/空房名/限额）、头部变异拒绝 ×6、目录变异拒绝 ×6（含
  交换、复制 ID、句柄、行、计数、限额）、段与桶变异拒绝 ×9（重叠/
  越界/桶长收缩/未知 kind/位图双置/NaN 列/bool 字节域）、键序与 arena
  变异 ×5（字段键交换/类型键交换/房名 UTF-8/描述符越界/惰性字符串
  UTF-8 与未知标签）、合并成功 + 身份冲突拒绝 ×6。
- kk-world 3 项：**两查看者（p1/p2）× 全部 16 个内建对象族变体（controller/
  spawn/extension/storage/source/双 creep/link Generic/tombstone/ruin/
  双 resource/双 flag/工地 + 不可见房己方旗）× 逐字段/缺失/null 对照**，
  含 ext 三权限裁切（ServerOnly 双侧不可见、Owner 仅主人）、地形
  base64 同源对照、id 由目录还原；真实形状私有块与公开块合并。
- kk-runtime 4 项：发布校验与句柄、代际前进拒绝 ×2、当前代获取独占、
  **慢工作者保留旧块**（旧句柄内容不变 + 报告列出代际/字节/句柄数 +
  释放后回收）、句柄跨库销毁存活。
- 棘轮：1854 通过/153 既有失败/907 跳过，相对基线零转红零转绿。
- bot 三夹具（B.4 树复跑，正确 CLI 形态 `--path/--ticks/--report`）：
  tooangel 2000/2000（0 终止、0 console error 轮）、overmind 2000/2000
  （0 终止、1 console error 轮）、wasm 10/10（0/0）——门禁指标与
  B.3 记录基线一致。**归因实验**：已提交 bot 报告停留在 M2B P6
  （0702235），与本次复跑的 diff（tooangel 少 `Game.market.getAllOrders`
  2000 次等）是中间多批的累积差异；stash 后在 HEAD（f65653e）重跑
  tooangel 与 B.4 树对比：api_access 键集与逐项计数、stub_calls_top、
  门禁指标完全一致，唯一差异是 tick 1 console 的 `Load: N`——其来源
  是 `Math.round(Game.cpu.getUsed())`（tooangel main.js:17），计时
  噪声。bot 报告文件不随本片提交（沿用 B.2/B.3 惯例，只提交
  kkscreeps.json）。
- 文档链接：192 Markdown、986 本地链接全过。

## 尺寸观察（非性能结论）

对照夹具（15 对象 + 2 房）：p1 块合计 12875 字节、p2 块 13110 字节、
JSON 参考 10965 字节——块比 JSON 大 ~17%。主因：定宽 8 字节列与
嵌套标签载荷（每整数 9 字节）换取消侧零解析，且夹具每桶仅 1–2 行、
桶结构开销占比高。二进制的目标收益是沙箱本地解码免宿主往返（B.5
测量），不是线尺寸；若 B.5 数据显示尺寸敏感，变长整数/共享字符串
是候选优化（版本升级处理）。JSON 的地形是 base64（+33%），块为原始
2500 字节，对象部分差距比总数字显示的更大。

## 评审与清偿

三路并行评审（编解码正确性 / 桥同源性 / 句柄生命周期+注释规范）：
前两者之外加自查探针，全部 P0=0。

- **自查 ×2（编码端）**：①超长字段/类型键可编码但解不开自身产物
  （往返不对称）——编码端补键长限额检查+测试；②Generic 原生数据
  自带 `ext` 键时桥报重复键，而 JSON 参考是覆盖语义——桥改为覆盖
  （retain 后 push）+ 回归测试 `generic_native_ext_key_is_overwritten_by_catalog`。
- **编解码评审（PASS w/cond）**：P1×2 清偿——解码端补字节/arena/
  地形三项限额（此前只查对象/类型/字段计数，§6"解码前检查限额"
  落空）；嵌套记录键改为解码端镜像校验严格升序（重复键的取舍此前
  落到消费者）。P2 清偿：编码端 arena/长度前缀 u32 截断改 try_from
  防配置性回绕；零长度段 offset 强制为零（编码端同步写 (0,0)）；
  字段条目 3 字节保留位非零拒绝；位图越行尾位强制为零（原互斥检查
  连尾部位一起比，宽严失据）；行占用不变式（每桶每行恰被一个目录项
  引用——重复引用使一行数据以多个 ID 显形，幻影行拒绝，新增
  `DuplicateRowRef`/`UnreferencedRow` 变体）；嵌套容量预分配以剩余
  字节数为硬上界、嵌套深度加 1024 结构硬上限（防病态配置栈溢出）。
  新增测试钉子 ×5：解码三项限额、嵌套重复键（读取期）、变长载荷
  尾字节非恰好消费、arena 缺失+零长段 offset、保留字节+位图尾位+
  行别名。未清偿登记：全 null 字段列类型的跨 tick 抖动已在模块文档
  登记（消费者须以计划字段描述为准）。
- **桥评审（PASS）**：重构等价性、同源性、Ref 转换精确性三镜头
  "未发现"。P2 清偿 ×4：夹具补 ConstructedWall/Road/Mineral/Extractor
  （对象族覆盖 16/16 变体）与 `creep_id: None` 墓碑、不可见房己方
  工地；ext 补浮点字段 `test:ratio=1.5`（Float 往返入对照）；
  `compare_worlds` 补 audience_hash 直接断言；constructionSite 桶键
  （`builtin:constructionSite`）与其扩展治理键（`builtin:{目标结构}`）
  的差异在 `type_key_of` 文档登记（接线时消费者以 structureType 为准）。
- **句柄库评审（PASS w/cond）**：P1 清偿——publish 补块头
  tick/generation 与发布代际一致性校验（+测试）；模块文档 audience_hash
  设计意图改为现状表述（C01）；测试注释"覆盖全部内建对象族"改 16/16
  实数（补齐后声明成立）。P2 清偿：publish 的结构校验移到锁外
  （消除慢工作者 acquire 的队头阻塞），代际前进性在锁内复核；错误
  类型 String → anyhow（crate 惯例）；模块文档对齐现状（结构校验
  边界、限额来源、报告含当前块与 handles 字段、超时拒收具体化、
  中毒策略说明、"代纪"错字）；`RetainedBlock.handles` 补"减库内唯一
  引用、近似快照"语义；`view()` 注释补不可变依据；黑话展开（同形/
  同构/不串行/行 1/2/3）。未清偿登记：`RunEpoch` 派生 `Ord` 归 kk-core
  公共 API 变更，登记为后续（当前仅 store 一处手写字典序）。

清偿后全量复验：461 通过、0 失败（+5 钉子）；clippy/fmt 干净；
棘轮 1854 零转红零转绿；bot 三夹具复跑同基线；文档链接 991 过。

## 限制与后续

- 沙箱侧解码器与包装器（JS）未实现——B.5 接线时按同一布局生成；
  Rust 侧 `KkpvView` 的惰性读取是包装器语义的对照参考。
- `KkpvBlockStore` 未接入协调器发布点；接入时挂在 embed 的 tick 尾
  （与 RunEpoch 同源），消费在 worker 视图装配。
- 公开块受众划分、条件权限、PlayerView 世界级路由（设计 §5 后半）
  归 B.5/C。
- 性能矩阵（编码/getter/枚举、复制字节、CPU/RSS、多查看者共享收益）
  归 B.5，沿用 P2 工具。
