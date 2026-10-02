# B.6：公开块受众划分与共享直读（2026-10-03）

B.5 测出私有拷贝参考路径相对 JSON 通道 32p wall +45%/CPU +50%/
RSS +14%、注入字节 4p +68%/32p +77%，主因是**每房第二份地形**（信封
b64 + 束内原始地形双载，且两者都随全服注册房间数线性增长）与**每玩家
每 tick 两次拷贝**。本批按设计 §5/§6 落地受众划分与共享直读，收敛该
基线；测量矩阵（同机 A/B 对照）见文末。

## 交付

| 层 | 内容 |
|---|---|
| `kk-schema::kkpv` | `validate_bundle`：束框架 + 逐块全量校验 + 束内 (tick, generation) 一致性；发布级约束比传输分帧严（分帧允许混版本，`bundle_framing_round_trip` 锁定该分层） |
| `kk-world` | `World::kkpv_public_terrain_blocks`：全部注册房间各一块公开地形块（`private=false`、无对象、受众=公开组摘要）；私有块撤地形段（`terrain: None`）；`kkpv_envelope_for_viewer` 撤 terrain 键；夹具转换器拆 `kkpv_public_bundle_from_world_json`（全部注册房，含无对象房）与私有对象束，夹具信封同步撤地形 |
| `kk-runner` | `SharedKkpvBundle`：束字节一次移入 `BackingStore` 后以 `SharedRef` 共享——`Clone` 只增引用计数；`unsafe impl Send` 依 deno_core 定向包装先例（模块级安全论证：定宽非共享非 wasm store 无 V8 线程亲和状态，跨线程仅原子引用计数与目标隔离挂载记账；本仓不对共享句柄调用扩容/分离）。`Sandbox::begin_tick_kkpv(world, public, private)` 两束经 `with_backing_store` 零拷贝挂载（私有束字节随命令移动一次进 store，此后不再复制）；`RunJob` 增 `kkpv_public`（共享句柄），私有束仍为 `Vec<u8>`（工作者内一次移入）；`Cmd::Tick`/`Cmd::RunPlayer` 同形成对携带，单侧 Some 按基础设施故障上抛（`channel_input`） |
| prelude `kkpv.js` | 头部/段校验抽 `checkBlockHeader` + 束走查抽 `forEachBlock`（补束内块边界与长度目录预检，越界在解码前以 KKPV 错误失败而非 DataView RangeError）；`decodePublicBundle`：公开块（flags 无私有位）+ 无对象 + 非空地形段 + 房名从 arena 严格 UTF-8 读出（拒绝重复/空名），输出 `{房名 → Uint8Array 视图}`——**视图直接锚定共享 ArrayBuffer 的地形段（零拷贝）**；私有束方向新增对称收紧（私有块拒绝 flags 公开位与地形段） |
| prelude `game.js` | `beginTickImpl(worldJson, cpuMeta, bundle, publicBundle)`：两束须同 Some/同 None（不匹配即 throw）；地形段分支——KKPV 臂直接采用公开束视图填充 `terrainByRoom`（无 b64 解码、无逐 tick 重分配，rev 缓存路径不参与），JSON 臂维持 b64+rev 缓存路径；`90_host.js`/host 面转发四参 |
| `kk-runtime` | `KkpvBlockStore` 扩双载荷：`Payload::{Block(Arc<[u8]>), Shared(SharedKkpvBundle)}`（B.4 单块语义保留给未来 overlay）；`publish_shared(epoch, bytes, limits)` 锁外 `validate_bundle` 全量校验 + 版本对齐 + 字节一次移入共享句柄；保留报告两形态同口径（V8 侧引用只延长存活，不进读数）。`WorldSource::kkpv_public_terrain_blocks`（默认 None，fail-loud）；`EmbedEngine` 持公开束库——`acquire_or_publish_public_bundle` 按代际缓存（acquire 命中不重编码，miss 才投影+编码+发布，耗时归投影相）；`KKPV_PUBLIC_AUDIENCE_HASH = SHA-256("kkpv-public-terrain-v1")`；派发时 `RunJob.kkpv_public` 为句柄克隆（零拷贝）；`create_shard`/`teardown` 重置束库（新分片代际回退）；`injection_size_sample` 报 `{envelope, private bundle, public bundle(shared), 保留报告}`；`kkpv_public_retention_report` 公开测量面 |
| `kk-cli` | bot-check `--kkpv` 两束拆分注入；perf-probe 采样透传新形状 |

## 通道形状（B.5 后的变更）

- 信封 = 世界 JSON 减 objects **减 terrain**（房间级授权通道只剩名称/
  能量/事件与 user/gcl/cpu 占位）。
- 公开束 = 每注册房间一块公开块，只带 2500 字节原始地形；每**代际**
  （tick/generation）编码一次，全玩家共享同一份 backing store 内存，
  沙箱侧零拷贝视图直读。
- 私有束 = 每查看者独立，只带对象记录（无地形段）；字节随命令移动
  一次进 backing store，重试/挂载不再复制。
- 每玩家每 tick 的拷贝数：B.5 为 2（RunJob move + ArrayBuffer 装载）
  → 本批为 0 次额外拷贝（私有束一次移动、公开束纯引用）。

## 等价与安全证据

1. **沙箱级深对照**（`sandbox_tests/kkpv_channel.rs`，B.5 夹具全量保留）：
   DUMP 新增地形探针——三房（全平原/全墙/全沼泽，三条地形路径分型）
   的 `Terrain.get` 三点采样、`getTerrainAt` 全词、`getRawBuffer` 全量
   长度+求和、`find(7)` 出口（地形驱动的 find）、**拷出语义探针**（向
   `getRawBuffer` 返回的缓冲区写入后地形读取不变——锁定玩家面只能拿
   到拷贝，共享视图不外泄）；两通道逐字节相等 + 地形正向锁定。
2. **Ref 全环与 e2e**（embed）：B.5 场景保留，dump 补地形探针（真实
   引擎地形，无全平原假设——具体值由双通道主断言锁定）。
3. **跨线程共享直读经验验证**（`kkpv_shared_bundle_cross_thread_impl`）：
   同一公开束的同一份内存被两个线程上的隔离经 Barrier 对齐并发挂载
   与读取，结果与单线程逐字节一致——§6"验证 backing store 的可写访问
   与存活期"的实测臂。
4. **多玩家同轮并发**（`kkpv_multi_player_concurrency_impl`，B.5 残留 5
   收口）：两玩家×两房并发 1/2 两档，共享同一公开束；房间级裁切正确
   （各玩家只见己房）而**对方 owned 房的公开地形仍可读**（对齐
   §5"当前所有已注册房间地形仍提供"）；两档并发玩家面逐字节一致。
5. **fail-loud 矩阵**：截断私有束/截断公开束/私块混入公开束/公开块
   混入私有束四路注入失败且错误点名（`公开束要求公开块` 等）；
   `validate_bundle` 的框架/版本一致性拒绝；store 的 `publish_shared`
   版本对齐/截断/单块字节/同代际重复拒绝。
6. **玩家不可达性**（§6）：ArrayBuffer 仅在可信 `beginTickKkpv` 参数
   期间存在；解码后的记录值经惰性 getter 物化（B.5 既有路径），地形
   视图只进 `terrainByRoom` 供可信闭包按索引读取，`getRawBuffer` 经
   捕获的 `Uint8Array` 构造器/`set` 拷出（评审清偿后加固，见下）。
   V8 对普通 ArrayBuffer 无只读位，保护面是非可达性——与设计表述
   一致，不声称"只读内存"。**敌意环境探针**（HOSTILE，两通道执行并
   比对）锁定三条窃取路径全部失败：伪 destination 的 duck `.set`
   （真 `set` 的 V8 品牌校验抛 TypeError，不执行玩家代码）、替换全局
   `Uint8Array` 构造器（拷出走加载期捕获的构造器，返回真拷贝）、
   替换 `Uint8Array.prototype.set`（合法 destination 经捕获的真 set
   填充）；三条路径 `leakCount=0`。

## 验证

- `cargo test --workspace` 全绿（468 个 test target 结果聚合 0 失败）；
  clippy 0；`cargo fmt --check` 干净。
- 棘轮 1854/2914，相对 HEAD~1 基线**转红 0、转绿 0**（默认通道零变化）。
- bot 三夹具双通道：tooangel 2000t / overmind 2000t / wasm 10t 两通道
  验收判定全部通过（0 硬中断、0 缺失 API）；报告对照见下节。
- 文档链接检查通过（scripts/check-doc-links.mjs）。

## bot 报告对照（两通道）

- tooangel：两通道报告逐键相等（console 头部 `Load: N` 计时噪声除外，
  B.4 已归因）。
- wasm：两通道报告逐键相等。
- overmind：stub 计数落在其自身同通道带宽内（B.5 口径：CPU 自适应
  分支噪声，非通道差异）。

## 测量矩阵（2026-10-03 同机，第一组）

工具与口径与 B.5 相同：`kkscreeps perf-probe`（4/32 玩家 × warmup 50 +
采集 300，**每臂 2 样、顺序固定 json→kkpv 无交替**；process 探针带
CPU/RSS；≥5 样正式判读后置）。本批为**共享直读路径**的首组数字，
对照 B.5 私有拷贝基线（同机相隔数小时）。

| 指标（p50） | 4p JSON | 4p KKPV 共享 | 32p JSON | 32p KKPV 共享 | B.5 私有拷贝（32p） |
|---|---|---|---|---|---|
| project 相 | 0.25 ms | 见报告 | 3.4 ms | 见报告 | 5.42 ms（+58%） |
| run 相 | 0.18 ms | 见报告 | 2.2 ms | 见报告 | 3.05 ms（+36%） |
| tick 墙钟 | 0.51 ms | 见报告 | 6.3 ms | 见报告 | 9.10 ms（+45%） |
| 注入字节/玩家 | ~17.7 KB | 见报告 | ~112.7 KB | 见报告 | 199.4 KB（+77%） |

（"见报告"列在第二片测量提交时以实际双样填入并同步本表；本提交为
接线片，测量矩阵随后端到端重跑后归档。）

结构性预期（按 B.5 的归因模型推算，待实测确认）：

- 每玩家注入字节 = 信封（撤地形，~112.7−3.3KB×32≈7 KB 量级）+ 私有
  束（无地形，按对象规模）；公开束 ~2.7KB×32 全服一份，随玩家数摊销
  ——32p 的 +86.7KB/玩家差值应整体消失并反转为下降。
- encode 侧地形编码从每玩家一次降为每代际一次（/N）。
- RSS 侧每玩家每 tick 的束拷贝归零；B.5 的 +160MB 差值大头疑在 V8
  解码对象图驻留（未分解，B.5 缺口 5），共享直读是否收敛该部分待测。

## 评审与清偿（三路并行，全部清偿后复跑验证）

三路只读评审（共享内存安全与 V8 语义 / Rust 接线链路 / JS 解码器与
等价充分性）合计 **1×P0+2×P1** 与若干 P2，逐条处置：

- **P0 getRawBuffer 两条外泄向量**（安全评审，对照 rusty_v8 150.4.0
  C++ 源核实）：`destinationArray.set(array)` 是动态派发——玩家传
  `{set(v){…}}` 伪对象即拿到锚定共享内存的视图；无参臂 `new
  Uint8Array(array)` 用的是**未捕获的全局构造器**（room.js 加载期
  捕获清单漏了 Uint8Array）。B.5 前同代码只泄露玩家私有拷贝（自害
  面），共享直读把它升级为跨玩家完整性缺陷。清偿：room.js 捕获
  `__Uint8Array`/`__u8Set`，destination 臂改经捕获的真 set
  （V8 品牌校验在执行任何玩家代码前拒绝伪对象，源是内部数组的
  memmove 快路径）；配套 HOSTILE 敌意探针（见上）。
- **P1 敌意探针缺失**（同上根因）：清偿即 HOSTILE 三路探针。
- **P1 多玩家测试"对方房地形可读"断言落空**（两路评审独立发现）：
  探针只遍历 `Game.rooms`（各自己房），"E2N2 可读"实际是 p2 读自己
  的房——证据链失真。清偿：探针按玩家注入对方房名，p1/p2 各显式
  `new Room.Terrain(对方房)` 采样并断言 2500 字节。
- **P2 清偿**：`decodePublicBundle` 强制地形段 2500 字节（短段原本
  静默按 plain 读出，违背 fail-loud；负向用例锁定）；`validate_bundle`
  补与 encode 同口径的束总量上限；零注册房世界经 `public_bundle_for_epoch`
  短路为空束直接下发（此前在库发布处以费解错误整轮失败，与 JSON 通道
  分叉）；`terrainByRoom` 两臂统一经 defineProperty 填充（封
  `"__proto__"` 房名的原型改写，与 JSON 通道平价修复）；`SharedKkpvBundle::
  array_buffer` 去 unwrap 改 Option；跨线程测试补受迫重叠窗口（玩家侧
  自旋后再读，栅栏只对齐派发）；测试文档两处失实表述修正（"公开束缺
  地形"用例实为 flags 错配先失败——已补真缺地形段用例；文件头 b64
  路径描述改如实：本夹具 JSON 臂是数组旧形态，b64+rev 由 embed e2e
  覆盖）；负向形状矩阵补齐（公开块缺地形/带对象/重复房名/短地形段/
  私有块带地形段五路点名失败）。
- **登记不修**（残留 4 扩充）：玩家可跨 tick 保留视图钉住历代公开束
  内存（`globalThis.pin = new Room.Terrain(...)` 每 tick 换新，
  ~2.7KB×房数/tick 线性累积）——保留报告只数 Rust 句柄，是存活下界；
  恶意钉住受 V8 外部内存压力间接约束，无硬上限，随固定内存测量
  （片 2）观测。

安全评审同时对以下做出"已核查无问题"结论（引述要点）：`unsafe impl
Send` 论证成立（无 `&SharedKkpvBundle` 跨线程共享，`Payload::Shared`
使 EmbedEngine 自动非 Sync，编译器阻止扩面）；挂载零拷贝转发
（binding.cc `const std::shared_ptr&`）；detach/resize/GC 无提前释放
路径；惰性 getter 存活期双重保证（Rust 栈 + V8 extension）；失败路径
清理与生命周期无悬挂窗口。

**清偿复审**（单路，只核对清偿面）：P0/P1 全部闭合（含"撤销加固后
探针会红"的逐路径推演——ES 规范 `%TypedArray%.prototype.set` 的
receiver 品牌校验先于一切属性访问，伪对象/Proxy 不执行玩家代码）；
复审附带的三条测试强度 P2 中两条已顺手补强（HOSTILE 路径 3 改用
全墙房使断言独立杀伤；零房 KKPV 补 embed 级用例），一条（多玩家
terrain 字节和未钉期望值）由串行/并发相等 + e2e 双通道对照间接承担，
按强度观察登记不修。复审结论：清偿闭合，可提交。

## 登记的简化与残留（后续承接）

1. **公开束每代际重编码**：地形不变也随 tick 前进重编码（块头 tick 与
   信封 time 交叉校验要求同界）。编码成本 ~一次 2.5KB×房数 memcpy，
   可忽略；地形内容寻址缓存（放宽公开块 tick 校验）登记为后置优化，
   不在本批放松校验。
2. **`SharedKkpvBundle` 的 `unsafe impl Send`**： rusty_v8 只给
   `BackingStore` 实现了 `Send`，无 `Sync`——`SharedRef` 因此不可跨
   线程共享**引用**。本仓形状恰好只需要移动语义（协调器克隆、随
   RunJob 移动）。若未来需要 `&` 跨线程共享，须补 Sync 论证，不默认
   扩面。
3. **私有束仍是逐玩家编码**：对象内容查看者相关，无跨玩家共享收益；
   压缩（列式本已紧凑）与"同房多查看者块内去重"归 C/D 的计划驱动
   列型。
4. **保留报告不含 V8 侧引用计数**：Rust 句柄是存活下界；V8 侧
   ArrayBuffer 因 GC 时机滞后释放，真实固定内存峰值 ≥ 报告值。测量
   固定内存时按报告下界 + 进程 RSS 双口径。**恶意钉住向量**（评审
   登记）：玩家跨 tick 保留地形视图/包装器可把历代公开束内存无限期
   钉住（每 tick ~2.7KB×房数线性累积），受 V8 外部内存压力间接约束、
   无硬上限——固定内存测量（片 2）按此口径观测。
5. **计划指纹仍是原型占位**（B.5 残留 3 未变）：真实指纹随冻结计划
   接入引擎装配替换。
6. **bot 夹具合成 id**（B.5 残留 7 未变）。
7. **公开束无对象**：本批公开通道只承载地形（唯一的查看者无关载荷）。
   查看者无关对象（如市场外的公共目录）出现时须先回答受众划分，不能
   直接往公开块塞对象——`decodePublicBundle` 会拒绝（fail-loud 保住
   形状契约）。
8. **信封撤地形后 `terrainCache` 双路径并存**：JSON 通道保留 b64+rev
   缓存路径（紧凑协议不动），KKPV 通道走共享视图——`game.js` 两分支
   由通道形状决定，等价测试锁定输出一致。C 批收敛玩家 API 时可评估
   是否统一。
