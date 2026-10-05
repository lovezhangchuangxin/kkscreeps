# nursery 官方对象操作原型

这是独立 JS Mod，验证公共 `ctx.objects` 的自主生成、更新和到期删除。server.js 声明发射器与火花，player.js 提供 NurseryEmitter/NurserySpark 类及 FIND/LOOK 查询；没有官方专用 Rust 分支。它是接口原型，尚无资源生产、玩家建造、建筑 trait 或正式内容预设。

发射器从 tick 0 起每 period 轮生成一个火花；新火花从下一轮开始增加 age，到 life 后删除。配置只接受 period（2..100，默认 5）和 life（1..100，默认 3）。权威计数保存在字段和 namespace state；已停止的包保留对象、schema 与 state。

在仓库根目录构建并运行：

```bash
cargo build --release -p kk-cli
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-objects/scenario.json --ticks 53 --report /tmp/nursery.json
target/release/kkscreeps sim run --scenario compat/fixtures/sim-mod-objects/scenario.json --ticks 53 --kkpv --report /tmp/nursery-kkpv.json
```

两房各一个发射器、两玩家零指令。53 轮后 born=22、expired=20，各房一个 age=2 的火花；最后一次玩家读取的是结算前 age=1。2003 轮后 born=802、expired=800，仍各房一个 age=2 的火花。场景至少需运行 53 轮才能通过最终断言。

场景的 mods.root 装载 official 目录直接子包，当前也会装载 clockwork；没有按包选择语法。可将 nursery 复制到普通作者根目录的子目录，并把场景 mods.root 指向该根目录，使用同一装载契约；玩家 fixture 路径仍相对场景解析。省略 mods 不安装本包。

对象接口与恢复契约见[架构文档](../../../docs/architecture/mod-objects.md)，机器证据见[交付记录](../../../docs/research/notes/n1b-mod-objects-2026-10-05.md)。
