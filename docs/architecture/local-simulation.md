# 本地场景模拟

`kkscreeps sim run` 使用 `kk-runtime::EmbedEngine` 加载场景、运行玩家模块和推进真实结算。每轮所有玩家读取同一已提交视图，结果经现有协调器提交。当前实现 JSON v1 场景和默认玩法；没有 Mod 包加载、检查点恢复、热更新、观战或 seed 选项。

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run \
  --scenario compat/fixtures/sim-economy/scenario.json \
  --ticks 2000 --report /tmp/sim-economy.json
```

`--ticks` 默认 1000，必须大于零；`--concurrency` 默认 2，可选 1–32；`--kkpv` 选择二进制玩家投影，默认 JSON。路径相对当前目录定位场景，场景里的模块目录相对场景所在目录定位。报告必须使用 `.json` 后缀，且不能与场景文件相同。

## 场景输入

完整示例见[经济夹具](../../compat/fixtures/sim-economy/scenario.json)。场景的顶层、shard、bot、对象声明和断言声明拒绝未知字段；房间和对象 spec 复用已有嵌入接口的字段校验及默认值。

| 字段 | 含义 |
|---|---|
| `version` | 当前仅接受整数 1 |
| `shard.players` | 非空且 handle 唯一的玩家列表，支持字符串或现有 PlayerSpec 对象 |
| `shard.rooms` | 现有 RoomSpec 列表；包括 name、owner、rcl、地形等字段 |
| `bots` | 每个玩家必须恰有一个声明：player、modules、entry（默认 main）、memory（默认空对象） |
| `objects` | 可省略；按顺序放置 alias、room、kind、spec，alias 必须非空且唯一 |
| `assertions` | 可省略；最终状态的 JSON Pointer、op 和 value |

模块加载沿用[玩家沙箱](sandbox.md)的模块规则，入口必须是 JS 模块并导出 `loop` 函数；每轮调用一次，返回值不作为断言。此入口不等待 Promise。模块顶层代码在首轮执行，而非场景初始化时执行。

对象 alias 用于宿主报告和断言，不增加玩家可见的命名 API。初始化使用真实引擎放置接口，不运行隐藏 tick；初始 Memory 必须是对象，并遵守 RawMemory 的 UTF-16 长度限制。模块或 Memory 不能在待重试轮次中被替换。控制器初始位置沿用现有 RoomSpec 的 `(1,1)`。

## 状态与断言

初始和最终观察都读取已提交状态，包含 `time`、`players`、`objects`、`rooms`、`world`。常用指针如下：

| 指针示例 | 读取内容 |
|---|---|
| `/players/bot/gcl` | 实际玩家 GCL 累计点数，包含等级所需的累计基数 |
| `/players/bot/memory/loops` | 解析后的玩家 Memory；非法 JSON 时为 null |
| `/objects/container/store/energy` | alias 对应对象当前能量 |
| `/rooms/W1N1/controller/progress` | 房间控制器进度 |

alias 或玩家名中的 `/`、`~` 分别转义为 `~1`、`~0`。对象死亡后 alias 对应 null，`missing` 同时接受缺失指针和 null；`exists` 要求值存在且非 null。

`eq` 比较最终 JSON 值，缺失指针不能与 null 相等；`gte` 检查最终数值下限；`delta_gte` 检查最终值减初始值的下限。整数比较和差值保持精度；混合浮点比较拒绝把超出 JS 安全整数范围的整数转为浮点。差值超出 JSON 的 64 位整数范围时，以十进制字符串报告，判断仍使用精确整数。

没有断言的场景仅检查执行成功和完成轮数。经济夹具至少运行 500 tick，要求实际升级、孵化替补及旧 creep 死亡，不能通过少运行几轮获得同等验收结论。

## 报告与失败

报告记录场景字节、模块内容及 CLI 二进制 SHA-256、模块目录与清单、投影通道、并发配置、请求/尝试/已提交轮数、失败轮次、各玩家计数、断言及初始/最终状态。控制台和错误各保留前 20 条。当前运行时控制台仅提供文本，捕获异常后打印日志不会自动判为执行失败；开始执行后的 CPU 硬中断按现有协议记录为 runtime 错误，未进入求值的轮次另计。

`players` 中的 GCL 累计点数、Memory 和 CPU 桶来自实际 PlayerState；GCL 不是玩家 `Game.gcl` 的等级/进度对象。`world` 是宿主诊断投影，其公共 gcl/cpu 字段不是玩家状态。报告不保存完整运行状态，也不能作为检查点恢复。

玩家异常或硬中断使场景失败，但当前轮次仍可能提交合法指令和 Memory，报告必须保留实际已提交状态。基础设施或结算错误停止在最后已提交轮次；此工具不自动重试。失败轮重试契约见[tick 契约](tick-contract.md)。

| 退出码 | 含义 |
|---|---|
| 0 | 完成全部轮次、玩家执行成功且所有断言通过 |
| 2 | 玩家/运行失败或断言失败；报告保留失败详情与已提交状态 |
| 1 | 场景、模块、初始化或报告 I/O 失败；能写报告时 status 为 setup_failed |

CLI 参数解析失败沿用 Clap 的退出码 2，不写报告。报告通过同目录临时文件写入后重命名，配置失败也会替换旧报告，避免误读上次成功结果；非法报告路径直接拒绝。该操作不是完整世界持久化协议。

`run_wall_ms` 仅包含推进循环，不含加载、指纹计算、最终状态观察和报告写入。`phases` 使用运行时已有的累计阶段计时，是诊断数据，不能直接作为正式跨实现基准。玩家 Date、Math.random、CPU 查询保持生产行为，因此固定场景和源码不保证任意 bot 完全确定；复现要求见[开发套件设计](../designs/dev-kit.md)。

本批验收和已知边界见[交付记录](../research/notes/sim-run-2026-10-04.md)。
