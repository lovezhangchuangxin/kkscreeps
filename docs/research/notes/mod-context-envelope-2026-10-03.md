# Mod 类型化读取封套（2026-10-03）

承接 [上下文复制优化](mod-context-copy-2026-10-03.md)。本片基线为上一片最终 release，已经包含字段缓存与拥有值直接装配；不与前两轮不同采集时段的百分比累加。

## 实现与语义边界

上下文根记录的 args/room/state/target/tick 与 target 内的 fields/id 都是固定成员，原来却每次创建动态 JSON Map，再经 serde_v8 的 map 路径转换。本片让 `ChainHost::read` 返回 `ChainContext` / `ChainTarget`，无需构造这两个 Map；serde_v8 0.321.0 的 struct 路径使用静态键（`ser.rs` 的 ObjectSerializer / `keys.rs` 的 v8_struct_key），以 Internalized 字符串让 V8 去重；map 路径则把动态字符串转换成普通键。未使用依赖中仍标为 unused 的 KeyCache。依赖源码来自 Cargo.lock 固定版本。

只有固定封套类型化，开放的 args/state/fields 继续是 Value；不缓存 JS 对象或 namespace state、不减少读写同步次数。字段顺序显式保持原默认 BTreeMap 键序：args/room/state/target/tick 与 fields/id；两条 serde_v8 路径均创建空原型对象。写回仍走原动态 JSON 与领域校验，没有引入 struct 反序列化对继承属性或 getter 的不同访问规则。

诊断输出包装围住真实序列化，新增 read_to_v8 阶段；关闭诊断时不读时钟。op_read 仍包含转换，host_read 在候选读取后结束，不能把所有阶段相加。CLI 诊断报告标为 v2，新阶段必须具有完整读取次数；无版本字段的旧报告严格按 v1 原阶段校验，未知版本拒绝。

新增真实 JS/native 链测试验证初次读取的键序、空原型、成员类型，以及 next 后 tick/room/target.id/args 的权威刷新与 target 引用保留，覆盖 JSON/KKPV。沿用任意 JSON namespace、嵌套 wrap、跨 namespace、别名拆分、本地辅助成员、取消、故障与重试测试。报告门测试先证明未支持新阶段时失败，再核验 v2 阶段缺失、次数/错误/耗时损坏与未知版本不能成功。

## 测量方法

沿用 [交替探针](mod-attribution-probe-2026-10-03.py)，400 link、预热 100 tick、采集 500 tick、三轮共 24 个窗口。原版与诊断原版使用同一份复制优化后的 release；新版本使用最终检查通过的 release。采集期间不并发运行构建、测试或 bot/兼容性套件，允许短文本读取和文档编辑。三个边界的全部对象状态摘要、四个链位计数及各模式包/预算身份必须一致。

桌面机器负载未控制，窗口短，仍限于纯结算；不覆盖玩家、投影、持久化或网络，不设置硬性能门禁。[原始报告](mod-context-envelope-validation-2026-10-03.json) 保存 24 个窗口、摘要及 validation 验证字段。

## 结果与判断

以下为三轮窗口指标的中位数；CPU 为进程所有线程的 core-ms/tick。

| 无插桩完整链 | 优化前 | 优化后 | 降幅 |
| --- | ---: | ---: | ---: |
| 墙钟 P50 | 28.777 ms | 27.236 ms | 5.35% |
| 墙钟 P95 | 29.475 ms | 27.985 ms | 5.05% |
| 墙钟 P99 | 30.935 ms | 28.830 ms | 6.80% |
| CPU/tick | 29.776 core-ms | 28.311 core-ms | 4.92% |

三轮 P50：优化前为 28.823 / 28.686 / 28.777 ms，优化后为 27.236 / 27.355 / 27.228 ms。每轮 P99：优化前为 30.935 / 30.160 / 31.212 ms，优化后为 28.830 / 28.316 / 30.330 ms。native/empty 的 P50 均约 2 ms，本片不从这些短窗口判定它们的确定收益或回归。

诊断窗口的 host_read 为 3.613 → 2.880 ms/tick，op_read 为 10.762 → 9.246 ms/tick，方向与减少固定 Map 构造、使用静态内部化键一致。优化后的 read_to_v8 直接计时为 4.524 ms/tick，每个采集窗口 1200000 次、零 op 错误。旧二进制没有这个阶段，不用旧 op/host 差额当作其转换耗时来作直接比较。

原始对象状态摘要在三个边界一致，链四个计数终值均为 240000；新旧报告分别按 v1/v2 验证。收益相对于复制优化后的基线，没有与前两轮百分比累加。

固定宿主协议使用明确的 Rust 类型，开放玩法载荷保留 JSON，适合现有模块边界；小改动获得稳定方向的局部收益，保留本片实现。诊断中的写 op 仍约 11.942 ms/tick、host_write 约 2.713 ms/tick，下一步先补齐写入转换的直接计时及 getter/错误传播测试，再判断是否优化反序列化。当前差额不等于纯转换成本，不据此更换整链驱动或省略候选同步。

## 验证

`pnpm run check` 通过：559 个 Rust、19 个 JavaScript 测试，8 个 Rust ignored；格式与 workspace/all-targets Clippy 通过。Python 全套 12 测试通过。固定 screeps-ok 提交 `34a630efa42908c117c19c25d2350fa2387bd633` 的棘轮为 1854 passed / 153 failed / 907 skipped，零转红/转绿。

重新构建的同一 release 完成 TooAngel / Overmind / WASM 的 2000 / 2000 / 10 tick，均无硬中断、惩罚跳过或 missing API。TooAngel 仍通过 log 记录既有的 `offsetPosition.lookFor` 错误；Overmind 仍为 1 个 console-error tick，其错误栈在替换 harvest 对象 id 后与 [预算复审基线](mod-budget-review-validation-2026-10-03.json) 一致。其余 log/warning 次数不作逐字一致结论；bot 本身不走 Mod 链，本片链语义由真实链测试覆盖。
