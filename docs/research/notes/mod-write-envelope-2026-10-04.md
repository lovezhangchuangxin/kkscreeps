# Mod 拥有值写入封套（2026-10-04）

承接 `5564528` 的成对采集工具。此前写入直接计时表明反序列化是密集链的主要成本，本片去掉固定传输封套的两层临时 JSON Map，保留开放候选 JSON 与原领域校验。

## 机制

ChainHost.write 接收 ChainWrite，直接拥有 state 和可选 target_fields，runtime 在字段校验后移交 state。serde visitor 沿用 serde_v8 的动态 MapAccess，不改为 deserialize_struct：后者按固定字段顺序访问、可以读取继承属性、忽略未知成员，会改变现有可观察行为。根和 target 未使用的成员仍完整转成 Value 后丢弃，保留 getter、转换失败及深度限制；非对象封套仍无候选成员，其数组元素仍完整转换。缺失 state 为 null、缺失 fields 为不写目标，显式 fields:null 仍按原领域规则不改字段。

读取、SDK 的写入载荷、能力核验及六读六写次数保持原机制；仍先完成转换，再重新检查故障，最后字段校验与 namespace 写入。write_from_v8 阶段包含本片转换与边界，不升级诊断 v3 的计数口径。

## 额外发现并修复的 P1

固定 serde_v8 0.321.0 的 ValueType::from_v8 对 Symbol panic，而非返回 Err。原 Value 路径在差分测试直接 panic；整槽写入的受控子进程进一步复现 non-unwinding panic / aborting，即可捕获的 JS 错误变成进程终止。

转换段现在单独 catch_unwind，归一为 UnsupportedType 转换错误，随后沿原路径留存整槽故障；此时没有 OpState/候选借用。只捕获同步反序列化中的 Rust unwind，不格式化 panic 载荷，也不让 unwind 穿过 V8 op 边界。故障阻止 host 写回及候选发布，realm 毒化并按原配置重建。Rust 默认 panic hook 的 stderr 仍可能出现；没有修改全局 hook。这不是 V8 内存隔离或通用 panic 恢复，也不能在外部 panic=abort 构建中提供捕获。

## 验证范围

十五个 JSON 形状（含重复键）与原 Value 提取结果逐值比较。二十九个 V8 输入按新/旧转换对照候选值、错误与 getter/Proxy 调用顺序；差分参考对旧转换也加同一 unwind 边界，以使 Symbol 归一后继续比较，原未经保护的 abort 另由子进程日志保留。输入覆盖自有/继承成员、undefined、Map、非对象/数组、TypedArray、未知字段 getter/BigInt/Symbol、循环及深度错误。

整槽子进程新增 Symbol 故障，诊断关闭/开启均须可返回错误、保留故障、无 host 写入、无 after/native 且能力销毁。runtime JSON/KKPV 增加 state、根未知成员及 target 未知成员三种 Symbol 路径，getter 先运行 native、外层捕获错误时，世界/time/generation/namespace 仍回滚，同输入重试再次失败且无副作用。原 CPU/墙钟、别名刷新、权威字段写面及完整链测试继续运行。

release 单对象故障探针另核验旧二进制以 SIGABRT（−6）退出，新二进制以普通转换错误（1）退出，均未产生成功报表。

## 测量与结论

全部采集串行运行，与构建/测试/兼容/bot 分开；包、预算、二进制与各窗口对象摘要均由成对工具核验。保留四份完整原始数据：

- [完整链无插桩](mod-write-envelope-plain-2026-10-04.json)：预热 200、采集 1000、五轮 10 窗口。
- [转换阶段诊断](mod-write-envelope-profile-2026-10-04.json)：预热 50、采集 200、三轮 12 窗口，含单独的无插桩窗口。
- [无 Mod/空包短窗口](mod-write-envelope-native-2026-10-04.json)：预热 100、采集 500、三轮 12 窗口。
- [无 Mod/空包扩大窗口](mod-write-envelope-native-long-2026-10-04.json)：预热 500、采集 3000、五轮 20 窗口。

| 完整链指标 | 旧端独立中位数 | 新端独立中位数 | 每轮新/旧倍率中位数 |
| --- | ---: | ---: | ---: |
| P50（ms） | 27.335 | 26.194 | 0.96054 |
| P99（ms） | 28.991 | 28.385 | 1.00392 |
| CPU（core-ms/tick） | 28.318 | 27.192 | 0.96344 |

完整链五轮 P50/CPU 每对均降低，成对中位数分别降低约 3.95%/3.66%。各轮 P99 为旧端 28.991 / 28.390 / 31.126 / 28.274 / 43.263 ms，新端 27.100 / 29.033 / 27.277 / 28.385 / 51.114 ms；第五轮两端抬升，新端更慢。独立 P99 中位数降低不等同于成对改善：成对倍率中位数反而高约 0.39%，不能宣称尾延迟改善或零回归。

诊断 host_write 为 2.785 → 1.863 ms/tick，op_write 为 12.121 → 11.159；write_from_v8 为 8.778 → 8.754，read_to_v8 为 4.564 → 4.584。主要变化落在直接成员访问、拆封与写回的宿主边界，未观测到明确的 V8 反序列化下降。阶段相互包含，不能相加；本片不是将转换瓶颈全部消除。两端均为诊断 v3，每窗口两个转换阶段各 480000 次，op 错误为零。

原生/空包短窗口出现明显波动：原生 CPU 成对倍率 1.05542、P99 2.35733；空包 CPU 0.97850。因无 Mod 路径尚有疑问，扩大到 500/3000/五轮，没有丢弃短窗口。长窗口原生 P50/CPU 成对倍率为 0.99880 / 1.00575，P99 为 0.97763，未重现原生短窗口高尾延迟；空包 P50/CPU 为 1.02746 / 1.02095、P99 为 0.99278，仍需跟踪约 2% 的差异。当前桌面负载未受控，不能把追加采集当作短窗口无效，也不能归因全部差异或给出整体零性能回归结论。

## 最终验证

[验证与构建指纹](mod-write-envelope-validation-2026-10-04.json) 保存全部报告及源文件摘要、release Symbol 探针和日志摘要。全量检查：563 个 Rust、19 个 JavaScript 通过，9 个 Rust ignored，fmt/Clippy 通过；Python 全套 27 测通过。release 兼容棘轮 1854 passed / 153 failed / 907 skipped，零转红/转绿。TooAngel/Overmind/WASM 均完成 2000/2000/10 tick，无硬中断、惩罚跳过或 missing API；Overmind 既有 1 个 console-error tick、TooAngel 既有路径错误日志保留。

本片完成候选封套与进程 abort 清偿，观测到完整链中位成本下降；真实玩家闭环、稳定基准机的尾延迟与无 Mod/空包零性能回归均不在本次达标结论内。
