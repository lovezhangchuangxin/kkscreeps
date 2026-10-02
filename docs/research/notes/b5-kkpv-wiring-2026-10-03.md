# B.5 第一片：KKPV 通道接线与等价（2026-10-03）

B.4 交付了 KKPV v1 编解码原型（`kk-schema::kkpv`）与同源桥，但未接线。
本片把二进制通道接进协调器与沙箱执行链：JSON 参考通道保留且默认
不变，KKPV 通道经 `EmbedEngine::set_kkpv_channel(true)` 或 bot-check
`--kkpv` 显式启用。验收口径是"两通道装配出的玩家可见面一致"——三道
等价测试与 bot 双通道门禁锁定。测量矩阵（链路径开销、JSON/二进制
端到端、复制字节）归 B.5 第二片。

## 交付

| 层 | 内容 |
|---|---|
| `kk-schema::kkpv` | `encode_bundle`：束框架（u32 块数 + 逐块 u32 长度目录 + 顺序块字节），空序列为 4 字节零计数 |
| prelude `30_game/kkpv.js` | 沙箱侧可信解码器：加载期捕获全部内建（DataView 原型方法/Uint8Array/defineProperty/Reflect.apply/fromCharCode/Infinity），玩家运行期改写原型不影响解码；逐块结构校验（头/段互斥/目录长度精确/桶消费精确/保留字节）与逐读取校验（位图/标签/UTF-8 严格性/非有限浮点）；记录字段惰性解码——缺失位不定义属性（与 JSON 缺键同形），首读经 getter 解码并固化为值属性，setter 写入同样固化 |
| prelude game.js | `beginTick` 重构为 `beginTickImpl(worldJson, cpuMeta, bundle)`：bundle 非 null 时 `gw.world.objects` 由 `gw.kkpv.decodeBundle` 解出，其余步骤与 JSON 通道逐语句相同；宿主面新增 `beginTickKkpv`（90_host.js） |
| `kk-runner` | `Sandbox::begin_tick_kkpv[_with_budget]`（看门狗/cpu_json/清理路径与 JSON 通道共用）；`call_host_kkpv_begin` 以（信封字符串、ArrayBuffer、cpu 字符串）混参调用宿主——块束字节经 `new_backing_store_from_vec` 装入新建 ArrayBuffer，构成每 isolate 私有拷贝；`RunJob.kkpv_bundle`、`Cmd::Tick`/`Cmd::RunPlayer` 增 bundle 字段、`PlayerThreads::tick_kkpv`/`run_player_kkpv` |
| `kk-world` | `World::kkpv_envelope_for_viewer`：与 `world_json_for_viewer` 房间级内容同形、不含 objects（复用 room_visible_to/room_energy/terrain_json 同一辅助）；`kkpv_bundle_from_world_json`/`kkpv_envelope_from_world_json`：手搓世界（bot 夹具）转换器——全部字符串保持 Str（无引用推断），id 不成 kk 形态时按字符串稳定合成小整数 |
| `kk-runtime` | `WorldSource::kkpv_projection_for_viewer`（默认 None，启用 KKPV 时 None 即失败不静默回落）；embed dispatch 双通道分支（market 对信封与整世界一致注入，投影失败进 failure 通道）；`KKPV_PROTOTYPE_PLAN_HASH`（SHA-256("kkpv-prototype-v1") 占位）与 `kkpv_audience_hash`（SHA-256(玩家名)） |
| `kk-cli` | `bot-check --kkpv`：prep tick 与主循环都走 KKPV 通道，判定口径与 JSON 通道一致 |

## 通道形状

- 信封 = 世界 JSON 减 `objects`（房间级授权通道：地形 b64+rev、能量、
  事件、market、gcl、cpu 占位都在信封侧）。对象记录 = 每房间一个
  私有块，束化后一次 ArrayBuffer 拷贝进沙箱。
- 对象数组顺序：块的房间序（rooms 迭代序）× 目录 ID 升序，与 JSON
  参考路径的对象遍历序一致（BTreeMap 房 + BTreeMap<ObjId> 对象）。
- 嵌套记录键序：编码端保留条目顺序（与 JSON 参考在 preserve_order
  构建下的插入序一致）——Store 等嵌套对象的键序是玩家可见面，不因
  二进制通道重排（评审清偿项，见下）。记录顶层字段列按键排序，
  原始记录不经玩家面透出，该层键序不构成分歧。

## 等价证据

1. **沙箱级深对照**（`sandbox_tests/kkpv_channel.rs`）：同一夹具世界
   （11 对象族、null/缺失并存、非 ASCII、嵌套 spawning、不可见房）
   双通道注入两沙箱，同一深度快照表达式（通吃遍历 + 各族显式字段
   探针 + find/lookAt + getObjectById，键排序、THROW 探针双向捕获）
   逐字节相等；快照含 needle 检查防空壳假等价。
2. **Ref 解码锁定**（同文件）：手工块锁定列式 Ref（墓碑 creepId）与
   嵌套记录内 Ref（废墟 structure.id）的 kk 前缀十六进制还原——权威
   桥的 Ref 只出现在这两处，夹具转换器不产 Ref，故用手工块直接锁。
3. **EmbedEngine e2e**（`kkpv_channel_e2e_equivalence_impl`）：真实
   分片双通道→可见快照 + harvest 意图返回码 + 结算后世界状态三重
   相等（store.energy=2、source 2998 两侧一致）。
4. **信封对照**（kk-world）：信封与 `world_json_for_viewer` 去 objects
   后逐键相等，两名查看者（可见房裁切）。

## 验证

- `cargo test --workspace` 全绿；clippy 0；`cargo fmt --check` 干净。
- 棘轮 1854/2914，相对基线零转红零转绿（默认通道零变化）。
- bot 三夹具双通道：tooangel 2000t / overmind 2000t / wasm 10t 的
  --kkpv 运行全部通过（0 硬中断、0 缺失 API）；tooangel 与 wasm 报告
  与默认通道逐键相等（console 头部的 `Load: N` 计时噪声除外，B.4 已
  归因）；overmind 的 stub 计数落在其自身同通道运行带宽内——五次采样
  PathFinder.search：默认 8010/8006、KKPV 8009/8004/8007；room.find:114：
  默认 430/428、KKPV 423/428/425（两通道区间交错；overmind 按 CPU 用量
  自适应分支，运行间噪声，非通道差异；判定口径全部一致）。
- 文档链接 194 个 Markdown / 991 本地链接通过。

## 评审与清偿（三路并行，全部清偿后复跑验证）

三路只读评审（解码器正确性 / Rust 接线链路 / 等价测试充分性）合计
1×P0+5×P1 若干 P2，逐条处置：

- **P0 负数 Int 解码错误**（JS readInt"先合成无符号再减 2^64"在
  [2^63,2^64) 区间有一次舍入，-1 解码为 0、-3000 为 -2048）：改为按
  32 位半字补码取负得量值、量值单次正确舍入后取负；BigInt 全域对照
  验证后，夹具补 `fatigue: -3000` 正向锁定。
- **P0 Store 键序通道分歧**（评审实测发现）：deno_core 经特性统一给
  整个工作区启用 serde_json `preserve_order`——JSON 参考通道的嵌套
  对象（store 等）是插入序，而 B.4 的 `encode_nested` 按键排序，两通道
  `Object.keys(creep.store)` 在多资源 store（terminal/矿物）下今日即
  分叉。清偿：编码端保留嵌套条目顺序（去排序、保留重复键拒绝），
  解码端的嵌套键严格升序检查改为重复键拒绝；夹具补多资源 terminal
  + 不排序的 `Object.keys(store)` 探针（"energy","U","XUH2O" 插入序
  锁定）。单独 `-p` 构建不含 deno_core 时 preserve_order 关闭、两通道
  同为字典序——键序耦合由该探针看护，切换构建特性即转红提示。
- **P1 嵌套载荷截断静默错值**：全部标签（int/uint/float/ref 8 字节、
  array/object 的 u32 计数、记录键长）补 end 边界检查；变长字段要求
  恰好消费（尾随字节拒绝）；束总量与块目录精确校验（捕获
  DataView.prototype.byteLength 取总长）。
- **P1 字段键 "id" 可覆盖目录身份 / fail 用未捕获 Error / 嵌套键普通
  赋值遇 "__proto__" 与 JSON.parse 分叉**：分别以字段键拒绝、加载期
  捕获 Error、嵌套键经 defineProperty 定义自有属性清偿。
- **P1 注释错位**（bot_check 新函数插在 home_world 文档与其函数体
  之间）：纯注释复位。
- **P1 e2e 注释超卖（"下一轮视图"未断言）**：补自杀→墓碑轮——权威
  桥把墓碑 creepId 编为 Ref、沙箱解回 kk 串并与死亡 creep id 相等，
  Ref 全环（引擎结算→桥编码→沙箱装配→玩家读取）闭环；结算后第二份
  玩家视图跨通道比对。harvest 世界快照采集点移到自杀前。
- **P1 覆盖缺口**：夹具扩至敌方 creep（可见房出现、不入 Game.creeps）、
  敌方旗、不可见房自家工地（入全局集合不建 Game.rooms）、浮点（列
  与嵌套各一）、>2^53 的 u64（progressTotal 2^53+1 两通道同舍入）、
  true 布尔、空串/空数组/空 store、emoji（4 字节 UTF-8 代理对）、
  mineral 族、双房束；新增空束（4 字节零计数）与截断束 fail-loud 场
  景；主断言失败时输出首个差异窗口（两份超长单行 JSON 不可读）。
- **P2 清偿**：块头 tick 与信封 time 交叉校验（防未来混用不同快照的
  信封与块）、嵌套深度上限（与 Rust NESTING_HARD_CAP 同值 1024）、
  位图长度公式去 uint32 回绕、删除未用捕获、投影失败耗时归入投影相
  （不混入执行相）、encode_bundle 注释与 game.js/CLI 的"同形"表述展
  开为具体等同面、kkpv.js 文件头的校验分工表述改为如实（生产链路
  宿主直出不经 Rust 解码端，全量扫描不变式由"生产者即编码器"保证）、
  夹具转换器合成 id 跳过真实 id 值防目录撞号。

## 登记的简化与残留（后续承接）

1. **束内块地形未消费**：信封保留 b64+rev 地形（rev 缓存稳态零解码），
   块的地形段在沙箱侧暂不读取——块原生地形直读待第二片测量后再定。
2. **每 isolate 私有拷贝**：本片即设计文档 §6 的参考路径（共享直读
   不在本片声称范围）；复制字节数与 RSS 影响待测量片量化（每 tick
   每玩家两次拷贝：RunJob move 一次、ArrayBuffer 装载一次）。
3. **计划指纹为原型占位**：`KKPV_PROTOTYPE_PLAN_HASH` 是固定常量；
   冻结计划接入引擎装配后换真实指纹（消费者缺口，B.1 计划尚未接
   生产引擎）。
4. **公开块受众划分未落地**：桥仍只产私有块，每查看者独立编码；
   `KkpvBlockStore` 未接进发布点（私有块随 RunJob 存活已满足本片语
   义）——公开块/overlay 与共享收益测量同批（B.5 第二片或 C）。
5. **多玩家 KKPV 并发未测**：e2e 为单玩家；多玩家同轮（run_players）
   的 KKPV 通道形态与串行一致由通道实现结构保证（每玩家独立信封+
   束），并发专门测试归测量片一起补。
6. **KkpvLimits::default 的限额未经真实规模标定**：玩家可控字符串已
   被上游闸住（名字/签名 100 字符、说话 10 个 UTF-16 码元；市场字符
   串走信封不进块）；仍无上界的入口是 Generic 原始 JSON 与 mod 声明
   的 ext 字符串 max_bytes（自身无全局上限）；max_objects=4096/房无
   创建数量闸（对抗性刷旗可超）。超限以"KKPV 投影失败"整轮 fail-loud
   ——测量片标定。
7. **bot 夹具合成 id**：bot-check 的手搓世界 id 不成 kk 形态时按稳定
   序合成（跳过真实 id 值）；跨通道 id 不一致（JSON 通道 "ctrl1"、
   KKPV 通道 "kk…0001"），单次运行内自洽。块内对象按目录 ID 升序解
   出，与 JSON 数组原序可能不同（依赖顺序的消费者按 id 定位）。
8. **bot 门禁判定方法的已知弱点**：console 折叠把数字串归一为 "#"，
   对"数值对但量级错"的差异不敏感（stub_calls_top 的精确计数部分弥
   补）；overmind 带宽对照每通道样本 2-3 次，<0.5% 的系统性偏移不可
   分辨；KKPV 运行报告未归档入仓（默认通道基线在 compat/reports，
   复现用 `--kkpv` 重跑）。
9. **WorldSource::kkpv_projection_for_viewer 的 None 分支不可达**：
   生产唯一实现者是 World（恒 Some）；fail-loud 路径只有未来接入第二
   种世界源时才会走到，无测试覆盖（结构性不可达）。
10. **手搓世界不做对象级权限过滤**：sandbox 级等价测试的夹具按宿主
    投递原样携带对象（敌方旗也在内）——对象级可见性裁切在权威桥层
    （kk-world 两查看者测试）与 embed 层锁定，两层职责不同。

