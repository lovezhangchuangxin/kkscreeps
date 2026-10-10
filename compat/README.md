# 兼容性验收

本目录将 screeps-ok 的测试操作转为 `kkscreeps embed` 调用，并提供回归检查与 bot 运行验收。适配器检查玩家可见行为，bot 检查加载和运行情况；两者都不等于完整玩法兼容或生产服务器验收。

## 结构与状态

| 路径 | 用途 |
|---|---|
| `src/adapter.ts` | 导出 `createAdapter`，将测试操作转为引擎调用 |
| `src/rpc.ts` | 管理 embed 子进程，以请求 id 匹配逐行 JSON 响应 |
| `src/capabilities.ts` | 声明允许启用的可选测试组；true 不保证该组全部通过 |
| `src/parity.json` | 登记预期差异和必须跳过的测试；构建时复制到 dist |
| `test/` | 回归分类、脚本输入更新检查、RPC 超时、独立 Node 进程和真实引擎冒烟测试 |
| `scripts/run-suite.sh` | 运行完整上游套件，也可将额外参数传给 Vitest |
| `scripts/ratchet.mjs` | 全量运行后检查通过数、总用例数和逐用例回归 |
| `reports/` | 入库机器报告、回归配置与历史验收记录 |
| `fixtures/` | 手写策略包与场景夹具默认入库（新批次夹具自动被 git 跟踪）；第三方 bot（tooangel/overmind-dist）与合成的 wasm-bot 不入库，由 `scripts/fetch-bots.sh` 按需准备、基线报告自证完整性 |

同一 JavaScript 全局中的适配器实例共享子进程和世界状态，测试应串行使用并调用 `teardown`。它清理世界和玩家线程，保留子进程供后续测试复用。

请求超时会终止整个连接并拒绝其他等待请求，下一次调用启动新进程；旧世界不会自动恢复。`tick` 的 AbortSignal 在每 tick 前检查，不取消正在处理的请求。协议格式见 `crates/kk-cli/src/embed_main.rs`，设计依据见 [ADR-010](../docs/decisions/ADR-010-screeps-ok-adapter-bridge.md)。

## 构建与回归检查

在仓库根目录执行：

```bash
pnpm install
cargo build --release -p kk-cli
bash scripts/setup-screeps-ok.sh
pnpm --filter @kkscreeps/compat run build
pnpm --filter @kkscreeps/compat test
node compat/scripts/ratchet.mjs
```

准备脚本将上游检出到 `.compat/screeps-ok`，版本由其中的 `SCREEPS_OK_PIN` 固定。工具链要求见根 [package.json](../package.json)和 [rust-toolchain.toml](../rust-toolchain.toml)。

适配器默认优先使用 `target/release/kkscreeps`，不存在时回退到 debug；可设置 `KKSCREEPS_BIN` 显式选择。修改 Rust 代码后须重新构建选中的二进制，单独运行兼容脚本仍由调用方负责选择产物；完整 `pnpm run check` 则显式构建当前 debug CLI 并强制使用其绝对路径，CI 复用此入口。显式指定不存在的路径立即报错，不回退。

运行脚本会在 TypeScript 比编译入口新、`parity.json` 比构建副本新或构建副本缺失时重建适配器。这按修改时间判断；需要确认内容同步时应显式执行 build。

只有依赖真实引擎的冒烟用例会在未找到二进制时跳过；其他工具测试仍运行。

### 如何判断回归

[ratchet.json](reports/ratchet.json) 指定通过数下限和总用例数。[kkscreeps.json](reports/kkscreeps.json) 是最近入库的机器报告，工作区报告在运行后会被覆盖。当前验证为 **2905 passed / 9 failed / 0 skipped**，共 2914 例（≈99.7%；跳过已清零；9 失败中 1 例为登记豁免——SHAPE-GAME-002 形状例与 cpuShardLimits 契约互斥）；通过数地板已升至 2905（raid 调度首片 +21、powerBank 域批 +7、边缘环绕+核弹簇 +27 转绿：portal 传送/衰变/跨 shard 目的地、房间状态 getRoomStatus、worldSize 活算与连续距离环绕、launchNuke novice/respawn 门、核爆中心 portal 存活；弃用告警批 +7：DEPRECATED-MAP/PATH/SPAWN/DEDUP，register.deprecated 六触发点与官方逐字同位；tick({random}) 注入批 +7：越界拒收/耗尽回滚/矿物再密化注入门矩阵，randomInjection 翻转；InterShardMemory 本地段+CPU shardLimits 批 +7：六层写回链/检查点 137 键/四错误分支，interShardMemory/cpuShardLimits 翻转，同时豁免 SHAPE-GAME-002 形状互斥 -1；动作记录导出批 +21：captureActionLog 旁路通道/检查点 145 键/creep 13 键+结构+pc 动作载荷，actionLogCapture 翻转，剩余 21 个跳过例全部转绿——99% 检查点达成）。19 个 power 全部接通（OPERATE_OBSERVER 收口）；deposit 采集/冷却/寿命链、observer 限时视野、keeper lair 倒计时/刷怪、普通 Invader 追击/逃离/治疗/路径建筑改向/退场与远程攻击、Source Keeper 巡守/战斗与 invaderCore 倒计时/孵化/坍塌/预定生命周期、stronghold 布防部署（五套 bunker 布局）、raid 生成器（资格门/出口资格/官方组合/阈值重掷）、powerBank 生成与消费者（反射/衰变/废墟/泼洒）已交付；地图生成仍未实现。门禁要求：

1. 报告更新时间改变，并且 JSON 可以解析。
2. 总用例数匹配配置，通过数不低于下限。
3. 基线已通过的用例在本轮不能失败、跳过或缺失，除非被 [ratchet-allow.json](reports/ratchet-allow.json) 显式豁免。

逐用例基线取 `HEAD~1` 的入库报告。PR 合并提交的父提交通常是目标分支，普通 push 的父提交只是上一提交，不一定是上次通过 CI 的状态；读取基线需要至少两层提交历史。豁免不会降低总通过数要求，未命中的豁免仅给出警告。

上游仍有已知失败用例，直接运行 `bash compat/scripts/run-suite.sh` 可能返回非零。不能仅凭套件退出码判断新增回归，应使用上述门禁核对报告。`expected_failures` 的登记也不会自动豁免逐用例回归。

## Bot 运行验收

在仓库根目录执行：

```bash
bash scripts/fetch-bots.sh
target/release/kkscreeps bot-check --path compat/fixtures/tooangel --ticks 2000 --report /tmp/tooangel.json
target/release/kkscreeps bot-check --path compat/fixtures/overmind-dist --ticks 2000 --report /tmp/overmind.json
target/release/kkscreeps bot-check --path compat/fixtures/wasm-bot --ticks 10 --report /tmp/wasm-bot.json
```

TooAngel 使用固定 commit；Overmind 下载固定 release，并复用已经存在的非空文件。下载脚本不核验缓存文件的版本或内容。合成 WASM 夹具检查二进制模块加载与编译，不覆盖完整 WASM 游戏行为。

报告应结合请求/完成 tick 数、中断、跳过和错误内容判断。bot 捕获后用普通日志输出的异常不一定计入 console-error，因此不能把“零 console-error”解释为“没有运行问题”。

## 推进真实世界的场景验收

`bot-check` 在固定世界执行代码，指令不参与结算。经济进展应使用 `sim run`，在仓库根目录执行：

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-economy/scenario.json --ticks 2000 --report /tmp/sim-economy.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-economy/scenario.json --ticks 2000 --kkpv --report /tmp/sim-economy-kkpv.json
```

夹具包含采集、搬运、升级和寿命到期后孵化替补，最终断言要求 GCL/控制器进度增加、替补出生及旧 creep 消失。模块目录相对场景文件解析，支持预编译的玩家模块与初始 Memory；这不代表 TooAngel、Overmind 已全功能验收。格式、退出码与复现边界见[本地场景模拟](../docs/architecture/local-simulation.md)。

### TooAngel 单房经济与冷恢复

真实 bot 首片使用锁定提交 `87645a0b933360224737cca3b18ae9d112f46fef` 的 TooAngel `src/*.js`。在仓库根目录执行（输出目录须不存在，父目录须已存在）：

```bash
bash scripts/fetch-bots.sh
cargo build --release -p kk-cli
node compat/scripts/bot-gameplay.mjs --out /tmp/tooangel-gameplay
```

可用 `--bin <路径>` 或 `KKSCREEPS_BIN` 显式选择刚构建的 CLI；默认使用 release，不回退旧 debug。`--bot-root` 指向固定提交的 clone。工具逐文件校验 git blob 和源码清单，只复制原版 `src/*.js`，不使用 bot-check 曾留下的根目录扁平副本；额外模块、缺失模块、内容或提交不符均拒绝。`--ticks` 默认 2000、至少 2000；`--split` 默认 1000、须在 1..ticks-1。输出目录内有固定[场景](fixtures/sim-tooangel/scenario.json)、原版模块、[公开观察入口](fixtures/sim-tooangel/acceptance.js)、六份 sim 报告、检查点、输入日志和 `summary.json`。

JSON/KKPV 各运行一次 2000 tick 连跑和一次 1000+1000 tick 跨进程冷恢复；每阶段都启用输入日志与 `--verify-replay`，按记录的玩家结果重放结算并比较完整检查点。直跑与冷恢复之间分别验证同一组业务条件，不比较两次玩家执行的随机命名、CPU 桶或全状态相等。

初始只有 RCL1 房间、300 能量 spawn、两处 source 和 mineral，没有 creep、extension、预制工地或补给。入口不发额外意图、不修改 bot 配置，只在 `Memory.__kkAcceptance` 记录公开事件/出生观察，并捕获锁定 bot 的 main/brain/room/creep 异常日志。最终要求实际采集 ≥1000、转账 ≥200、建造进度 ≥200、升级 ≥200、至少两次已出生观察、一次死亡和死亡后一次出生；同时检查至少两只已出生 creep 存活、extension 建成、RCL≥2、GCL 累计增长 ≥200。事件读取上一拍，报告明确 `eventsThroughTick`，不把最后一轮未读事件计入统计。对多房、战斗、任意 bot 的异常检测和 72 小时稳定性不作承诺。

退出码 0 表示所有运行及业务条件通过；2 表示运行/验收失败（逐项检查和已有失败产物保留）；1 表示参数或输出目录无法建立。失败不复用上次成功报告，也不覆盖既有目录。入库证据见[本批汇总](reports/gameplay-tooangel/summary.json)；它只关闭 TooAngel 单房经济与冷恢复首片，Overmind、战斗、多房与 M2B 最终验收仍待推进。

### 普通 Invader 持续战斗与冷恢复

[场景](fixtures/sim-invader-battle/scenario.json)包含一名优先自疗的护卫与近战/远程/治疗三名 Invader。护卫靠近目标触发近战反击，NPC 自动追击、逃离、攻击和治疗；观察器除记录攻击/治疗事件，还比较实际 hits 与位置变化，要求护卫累计实际受伤 ≥100、NPC 血量实际恢复 ≥100、NPC 移动、近战/远程伤害和治疗发生。事件截至上一拍，不把最后一轮未读事件计入统计。场景为固定 200 tick 单房战斗切片，不是第三方 bot 战斗、多房或完整 M2B 验收。

在仓库根目录执行下面的 Bash 矩阵（输出目录须不存在）：

```bash
cargo build --release -p kk-cli
battle_out=/tmp/invader-battle
mkdir "$battle_out"
for battle_channel in json kkpv; do
  battle_flags=()
  if [ "$battle_channel" = kkpv ]; then battle_flags+=(--kkpv); fi
  target/release/kkscreeps sim run --scenario compat/fixtures/sim-invader-battle/scenario.json \
    --ticks 200 "${battle_flags[@]}" --input-log "$battle_out/$battle_channel-continuous.jsonl" \
    --verify-replay --report "$battle_out/$battle_channel-continuous.json"
  target/release/kkscreeps sim run --scenario compat/fixtures/sim-invader-battle/scenario.json \
    --ticks 100 "${battle_flags[@]}" --input-log "$battle_out/$battle_channel-split.jsonl" \
    --verify-replay --save-checkpoint "$battle_out/$battle_channel.checkpoint.json" \
    --report "$battle_out/$battle_channel-prefix.json"
  target/release/kkscreeps sim run --scenario compat/fixtures/sim-invader-battle/scenario.json \
    --ticks 100 "${battle_flags[@]}" --input-log "$battle_out/$battle_channel-split.jsonl" \
    --verify-replay --load-checkpoint "$battle_out/$battle_channel.checkpoint.json" \
    --report "$battle_out/$battle_channel-resumed.json"
done
```

六阶段逐段回放均比较完整检查点（包含 v6 Invader 路径与 lastMove）。[入库汇总](reports/gameplay-invader/summary.json)记录六阶段报告与业务指标；CLI 门禁同时比较 JSON/KKPV、连续/冷恢复的最终 world 和 Memory。该脚本不读取 Date/随机/CPU，故本固定切片验证上述终态等值；此结论不扩展为任意真实 bot 的独立执行等值。

### TooAngel 防御战斗与冷恢复

真实 bot 战斗首片：锁定提交的 TooAngel 在 RCL4 自家房（spawn/双 source/900 能量塔/安全模式额度 1）对抗敌方脚本坦克（40 部件 25T+10heal+5move，驻守远角自疗，与塔保持 ≥20 格使伤害衰减到 150/tick）。RCL4 的依据：塔自 RCL3 解锁且 RCL4 仍单塔，20 个 extension 为防御者孵化提供能量池；`"safeModeAvailable": 1` 必须显式播种——放置器缺省为 0，缺省则安全模式腿必败；冷恢复截点 260 落在防御腿 394 之前。初始 Memory 预置 `players.enemy.reputation = -100` 使 TooAngel diplomacy 判敌（未知玩家默认友好）。在仓库根目录执行（输出目录须不存在）：

```bash
bash scripts/fetch-bots.sh
cargo build --release -p kk-cli
node compat/scripts/bot-battle.mjs --out /tmp/tooangel-battle
```

模块身份核验复用 [bot-gameplay.mjs](scripts/bot-gameplay.mjs) 的 prepareModules（逐文件 git blob 校验，观察器以 kk_battle 模块身份部署，不混入经济夹具模块）；观察器 [observer.js](fixtures/sim-tooangel-battle/observer.js) 包裹 bot 主循环，统计对敌攻击事件（塔按攻击者 objectId 单独归因；EVENT_ATTACK 的伤害字段是 damage）、敌方承伤与自疗、战斗 creep 出生（首只 defendranged 394 拍）、安全模式激活（399 拍，玩家面 safeMode 为剩余拍数）与经济共存指标。默认 `--ticks 520 --split 260`（`--ticks` 下限 400，`--split` 须为 1..ticks-1）：JSON/KKPV 各一段连跑与一段跨进程冷恢复续跑（前缀段只验通用与 harness 检查，战斗指标在完整窗口段判定），每段 `--verify-replay` 逐位重放并比较完整检查点，观察器与敌方脚本哈希逐段比对。最终断言含场景内 `/objects/raider exists`（坦克存活源于塔 90 发后能量耗尽、防御者净输出不足——安全模式不保护敌方 creep 免伤，反而自 399 拍起禁止其自疗）与 `/rooms/W1N1/controller/owner == bot`；[入库汇总](reports/gameplay-tooangel-battle/summary.json)记录六段报告与指标（attackEvents 181 其中塔 90、敌方承伤 14410/自疗 13584、采集 2080）。

**窗口设计依据（确定性时序）**：防御链三腿（塔自 2 拍攻击、防御者 394 拍出生、安全模式 399 拍激活）全部落在 520 拍内；556 拍处 TooAngel defendmelee 会与敌方坦克同格后触发其自身潜在缺陷崩溃（`PathFinder.search(自,自)` 返回空 path，`search.path[0]` 未定义——官方引擎同样返回空 path，属 bot 潜在缺陷而非 kk 差异；堆叠本身是官方语义：安全模式下房主 creep 可穿行敌方 creep，官方 movement.js `checkObstacleAtXY` 与 kk `check_obstacle_at_xy` 同形）。窗口选在两者之间，验收不触发该崩溃。玩家 CPU 计量为当前线程 CPU 时间（kk-vm cpu_clock.rs，CLOCK_THREAD_CPUTIME_ID，等待与他线程不计费；官方为 hrtime 墙钟——同步执行下两者近似，阻塞等待官方计费而 kk 不计），机器重负载下缓存争用会抬高线程 CPU 时间、防御时序仍会平移（实测并发负载下防御者出生 394→420），指标阈值均为留有裕度的下限；引擎结算确定性由每段逐位回放保证。多房进攻（attackunreserve/Overmind 指令旗标）、bot 对战与长期存活仍是独立切片，不因本片关闭。

### TooAngel 多房进攻与冷恢复

真实 bot 多房进攻首片：锁定提交的 TooAngel 在 W1N1（RCL5、按 bot 自算布局位播种 spawn/存储 150k/20 extension——布局错位会置 misplacedSpawn 使进攻门 isHealthy 永假）对邻房 W2N1 的敌方脚本 reserver（claim+2move 贴控制器持续预留，控制器播种 `reservation{user:enemy}`）发动 attackunreserve 跨房出击。进攻链全程 bot 自主决策：`handleHostileReservedRoom` 判定敌方预留（首见后 1600 拍 `noReservedRoomInterval` 门 + RCL≥5 + isHealthy + spawnIdle 门）→ 排队 attackunreserve → 跨房行军 → 按 CLAIM 部件优先锁定 reserver 击杀。可见性由播种的 bot 方 scout 提供（出生即在 W2N1）。有限世界下该 scout 永久滞留 W2N1，机制有四环：未注册邻房在寻路地形包中为全墙占位，`PathFinder.search` 只返回 incomplete 的房内部分路径（不跨界）；scout 的 memory.path 经 Memory 序列化丢失路径形态，每拍 moveByPath NOT_FOUND→重搜→新路径首步 OK→incompleteCount 清零（25 拍救援永不触发）；停在边缘格时结算期跨房过渡把对象放回原格（未注册目标房）；`getNextRoom` 的 lastSeen 启发式使未注册邻房恒被优先、唯一已注册邻房 W1N1 永不被选中。四环共同维持 W2N1 全窗口可见（有限世界下的官方同构行为）。bot 自产 scout 最早也要 1497 拍（房级 `handleScout` 相位门），brain 层 nextroomer 排队（含 scout）被 `resourceStats` EMA（τ=10000，cpuUsed 初值=limit、cpuIdle=0）在 sim 时限内永假——u=0 理想情况也需 ≥6931 拍，冷恢复还会重置 EMA，故首拍可见性必须播种。Memory 预置 `players.enemy.reputation = -100` 判敌与 scout 的角色记忆（creep memory 即 `Memory.creeps[name]`）。W2N1 须播种 mineral——外房 `updateBasicData` 对无矿控制器房读 `minerals[0].mineralType` 会每拍抛错。在仓库根目录执行（输出目录须不存在）：

```bash
bash scripts/fetch-bots.sh
cargo build --release -p kk-cli
node compat/scripts/bot-offense.mjs --out /tmp/tooangel-offense
```

模块身份核验复用 [bot-gameplay.mjs](scripts/bot-gameplay.mjs) 的 prepareModules（观察器以 kk_offense 模块身份部署）；观察器 [observer.js](fixtures/sim-tooangel-offense/observer.js) 包裹 bot 主循环，统计 W2N1 可见性与敌方预留观察、attackunreserve 出生（按 creep 名前缀）、对 reserver 的攻击事件与击杀（按目标 id 归因；EVENT_ATTACK 的伤害字段是 damage）与经济共存指标。默认 `--ticks 2900 --split 2400`（`--ticks` 下限 2200，`--split` 须为 1..ticks-1；实测击杀落在 1861-2135 拍——标定期 debug 与 release 十二段观测，终版入库 release 六段为 1888-2048；玩家 CPU 墙钟计量下有抖动，截点留裕度使前缀段 `/objects/reserver missing` 断言稳定成立）：JSON/KKPV 各一段连跑与一段跨进程冷恢复续跑（前缀段只验通用与 harness 检查，进攻指标在完整窗口段判定），每段 `--verify-replay` 逐位重放并比较完整检查点，观察器与敌方脚本哈希逐段比对。最终断言含场景内 `/objects/reserver missing`（reserver 被击杀）、`/rooms/W1N1/controller/owner == bot` 与 spawn 存活；[入库汇总](reports/gameplay-tooangel-offense/summary.json)记录六段报告与指标。结构计数（offenseBirths/attackEventsOnReserver/reserverDamage/可见拍数）四段全窗口一致，时序类指标随墙钟抖动、阈值均为留裕度下限；引擎结算确定性由每段逐位回放保证。bot 对战、多房扩张运营（claim/预留自有房）与长期存活仍是独立切片，不因本片关闭。

### TooAngel 长期存活与冷恢复

锁定提交的 TooAngel 在 RCL1 自家房从布局锚点自举，跑满 86400 拍（≥72h 等价）验证长窗存活与跨进程冷恢复（43200 截点）。观察器统计全部有界（孵化名字表、采样数组封顶、按角色前缀归类），阈值按 86400 拍全长 debug 探针标定。在仓库根目录执行（输出目录须不存在）：

```bash
bash scripts/fetch-bots.sh
cargo build --release -p kk-cli
node compat/scripts/bot-survival.mjs --out /tmp/tooangel-survival
```

默认 `--ticks 86400 --split 43200`（`--ticks` 下限 86400，`--split` 须为 1..ticks-1）：JSON/KKPV 各一段连跑与一段跨进程冷恢复续跑，每段 `--verify-replay` 逐位重放并比较完整检查点（v2 日志格式，Memory 以摘要对账），观察器哈希逐段比对。业务断言含控制器等级实际爬升且所有权保持（release 实测 RCL5）、storage 后经济角色上岗、scout 周期出生、采集/升级/建造累计下限、bot Memory 字节远离官方 2MB 上限、零捕获异常零跳帧、公开观察 Memory 跨恢复接续。输入日志 v2（Memory 摘要化）下单段 ~1.5KB/拍、四份日志共约 450MB；86400 拍段 release 实测约 12-14 分钟。[入库汇总](reports/gameplay-tooangel-survival/summary.json)记录六段报告与指标。单房长窗自举不含 bot 对战、多房扩张运营或完整 M2B 验收；KKPV 在单玩家下的通道成本差异见台账 D 切片的双通道 A/B 待办。

Mod 生命周期场景可运行 `target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-tick/scenario.json --ticks 50 --report /tmp/sim-mod-tick.json`。它显式装载官方原型 clockwork，验证两房对象自主更新、玩家类和查询、Mod state 与真实计划身份；没有玩家意图或按官方包 ID 特殊处理。该场景不含对象增删、生产建筑和内容预设。另有 `sim-mod-objects/scenario.json`，使用相同命令运行至少 53 轮，验证 nursery 双房自主生成/到期删除、玩家类/空间查询；报告 mod_objects 包含全部动态 Mod 对象。两场景的 root 都装载 official 的直接子包，尚不是内容预设或显式包选择。

独立 JS 处理器组合可运行 `target/release/kkscreeps mod inspect --root compat/fixtures/mod-composition --report /tmp/composition.json`，使用 `--disable z-gate` 预览依赖闭包。`sim-mod-composition/scenario.json` 至少运行 100 轮，真实玩家核验链序 trace，报告 composition 与实际装配同源。接口和约束见[组合诊断](../docs/architecture/mod-composition.md)。

命名空间资源目录可运行 `target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-resources/scenario.json --ticks 100 --report /tmp/sim-mod-resources.json`。它只选 ore 包，验证玩家 VM 内新资源的提取/转入容器/掉落/拾取闭环与市场、跨房发送的显式拒绝；`scenario-combined.json`（选 beacon 自动带入 ore，须 ≥90 轮）与 `scenario-plain.json`（无 Mod 对照）用相同命令运行。接口和约束见[资源目录](../docs/architecture/mod-resource-catalog.md)。

## 登记差异与跳过

修改 `src/parity.json`，构建后同步到 `dist/parity.json`。`expected_failures` 与 `skips` 均是条目名称到登记对象的映射。`tests` 使用上游测试目录中定义的 catalog ID，不是文件路径或任意标题。

格式示例：

```json
{
  "expected_failures": {
    "example-gap": {
      "tests": ["MOVE-PULL-007"],
      "actual": "当前观察到的行为",
      "expected": "上游要求的行为",
      "why": "差异原因或相关记录"
    }
  },
  "skips": {}
}
```

示例仅说明结构，不表示该用例存在此差异。预期差异仍执行测试；`skips` 在引擎调用前跳过测试，条目必须包含 `tests` 和非空 `why`。具体校验规则以锁定版本的 screeps-ok `scripts/lib/parity.js` 的 `loadParity` 为准。

## 历史证据

| 文档 | 内容 |
|---|---|
| [上游基准复现](reports/m0-screeps-ok-parity.md) | 指定版本下 vanilla 与 xxscreeps 的原始对照结果 |
| [初始适配器基线](reports/m0-screeps-ok-baseline.md) | 项目初期的通过率记录，不代表当前覆盖 |
| [初始 bot 缺失 API 记录](reports/m0-bot-missing-api.md) | 当时的错误与缺失项 |
| [初始代码审阅](reports/m0-code-review.md) | 当时的修复与待办记录 |

历史数字和问题保留其时间、版本与场景，当前结果以重新构建后的报告为准。

### Overmind 持续世界与冷恢复

固定官方发布 `v0.5.2.1` 的 `main.js`，按 SHA-256 验证后原样复制；[场景](fixtures/sim-overmind/scenario.json) 从 RCL1、满能 spawn 和两个 source 启动，无初始 creep 或 extension。初始 Memory 显式声明 creeps/flags/rooms，观察入口不修改 bot 配置或发额外意图。

```bash
cargo build --release -p kk-cli
bash scripts/fetch-bots.sh
KKSCREEPS_BIN="$PWD/target/release/kkscreeps" node compat/scripts/bot-gameplay.mjs --bot overmind --out /tmp/overmind-gameplay-new
```

输出目录须全新。验收与 TooAngel 共用六阶段矩阵：JSON/KKPV 各 2000 tick 连跑、1000 tick 保存及新进程 1000 tick 续跑，每阶段输入回放比较完整检查点；实际出生、死亡后补员、正量采集/搬运/建造/升级、建成 extension、RCL2 与 GCL 增长均设门槛。Overmind 会替换 Memory 以重用解析结果，观察器在原版 loop 后写入当前 Memory，并在完整窗口截获官方 logger 的 FATAL/ERROR/异常输出；不能只检查宿主成功或日志头部。Flag 按官方继承 RoomObject，使原版 ref 补丁及持久任务引用有效。

本片范围是单房经济与寿命/冷恢复，战斗、多房和完整 M2B 验收仍未完成。最新证据见 [Overmind 持续世界汇总](reports/gameplay-overmind/summary.json)。

### 计划级静态对象通行

[公共 JS 声明包](fixtures/mod-static-objects/README.md) 分别放行内建静态障碍/Mod obstacle 建筑/己方障碍工地，或非公开他人 rampart；默认保持原生，两个轴可与墙规则组合。规则贯通默认玩家矩阵与实际移动，NPC 搜索消费同一快照。生物占格、显式 CostMatrix、建造/孵化放置保持原约束；逐房规则尚未实现。机制及双通道/失败重试/冷恢复回归见[移动契约](../docs/architecture/movement.md#静态对象通行声明g02d3-后续)。

### 移动障碍后的链预演

公共 SDK 的 `ctx.obstacle.evaluate()` 只读预演当前追加阻挡后的正式取消链，帮助多个 JS 策略组合时观察交换/跟随/拉拽的级联后果。它只在障碍槽同步窗口有效，不消费候选；继续追加、后续钩子或 decline 会使先前结果失效。使用示例与范围见[障碍场景说明](fixtures/sim-mod-move-obstacle/README.md#障碍后预演当前能力)。

后台服务公共 SDK 示例见[mod-services](fixtures/mod-services/README.md)，作者命令验证独立后台 VM 的异步探针和真实世界/检查点只读输入；它不是网络服务或耐久 outbox 验收。
