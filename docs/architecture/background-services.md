# 异步后台服务（G27.a 当前首片）

2026-10-09 已接入 `kk-mod-host::services` 与作者命令 `mod services`。后台服务使用独立线程/V8、有界队列、受控生命周期和有限只读探针，读取宿主传入的已提交世界副本。它不持有 Working，也不运行玩家上传代码。选择原因见 [ADR-017](../decisions/ADR-017-background-services.md)。后台域是服主安装的可信扩展，多包共用 VM，不提供包间恶意代码隔离。

## 包与注册

`mod.json` 可选 `services: "services.js"`，须为包内现存 JS/MJS 模块，与 entry/player 不同。依赖顺序和内容选择复用既有包装载；服务代码字节参与服务端摘要与玩法计划身份，改包后旧检查点按既有身份门拒绝。无 services 的旧 manifest 序列化不新增字段。默认玩法 realm 不装载 services 入口；后台 realm 只开放 `@kkscreeps/services`，不能装载玩法 SDK。包内静态相对导入可用，跨包/外部/动态导入仍拒绝。

```js
export default function register(api) {
  api.services.define('audit:summary', {
    async start(ctx) { return await ctx.probe('audit:sample'); },
    async request(ctx, args) {
      return { tick: ctx.view.time, label: args.label, sample: await ctx.probe('audit:sample') };
    },
    stop(ctx) { return { tick: ctx.view.time }; },
  });
}
```

注册必须同步，不返回 Promise；request 必填，start/stop 可省略。回调以外的定义字段、重复/外来服务键、注册窗口外调用均拒绝，捕获错误不能把非法声明变成成功注册。Rust 按当前包核对服务键，绑定表与原生登记必须一致。配置经 api.config 冻结。

## 生命周期和只读输入

`BackgroundServices::spawn` 异步等待装载，V8 从创建到销毁均在所属 worker 线程。start 按包依赖和定义序，stop 反序；一次 start 后串行处理 request，重复 start 或未 start 请求是域故障。正常 shutdown 等待 stop 后回收线程；直接丢弃句柄只关闭发送端，worker 完成已有有限请求后退场，不在玩法线程等后台回调。

start/request/stop 各自读取宿主给定的脱离世界的 JSON 副本。`ctx.view`、request args 与配置递归冻结，无法写回权威世界。快照没有自动刷新；后台等待期间世界可继续推进，下一次请求由宿主在提交后重新取得副本。作者 CLI 从实际 EmbedEngine 的 world_snapshot 读取，默认构造最小世界，`--checkpoint` 则先按完整身份恢复世界。该面是管理读面，没有玩家视野过滤；私密配置应放宿主配置，不放可导出的玩家模块源码中。

后台响应当前只返回调用者，不能直接产生玩法指令。后台 JS 全局、模块缓存、队列和正在等待的探针是进程态；冷启动重新注册/start，不恢复调用次数或未完成请求。取消等待不撤销已排队请求。本片没有耐久任务、消费游标或 outbox；G27.b 才负责原子耐久与重放，G26.b storage provider 仍未接入。

## 探针、队列和预算

`ctx.probe(name)` 只接受调用包命名空间下、宿主 ServiceOptions 显式授予的 ProbeReader。没有默认 fetch、文件路径、子进程或世界写能力。FileProbe 预打开常规文件，每次在阻塞池读取最多 max_bytes+1，超限拒绝、UTF-8 文本返回；持有同一文件描述符，原路径被原子替换后需重启宿主才能改读新实体。Unix 用非阻塞打开并复验已打开文件类型，避免元数据检查后换成 FIFO 导致启动挂住。

默认队列容量 16（范围 1..1024）、单个输入/输出 JSON 1 MiB（范围 1..8 MiB）、每个回调最多 8 次探针（范围 1..64）。try_send 即时拒绝队列满，不在游戏执行器等待槽位。探针调用次数按总调用计，不随完成释放；返回回调时有未完成探针则域失败，过期 token、跨包、未授予、非法参数和探针输出超限留存宿主故障，不能被 JS catch 掩盖。普通探针 I/O 错误可由服务处理。

注册与每个生命周期/请求复用 kk-vm 的 CPU、墙钟和 V8 堆/堆外限额。等待计入墙钟，不把阻塞池线程的 CPU 算成 V8 CPU；Rust 读缓冲受探针字节限额约束。错误、预算耗尽或非法能力使后台域退场，剩余队列收到失败，重新启动需创建新实例。失败域不再次调用 stop，也不影响 gameplay VM。已进入 OS 的只读文件任务可能在取消后继续，但不持有 V8/世界；执行器后台关闭避免它拖住 VM 回收。外部 ProbeReader 实现须有界、可取消且只读，耐久副作用不能走此入口。

## 作者验收与后续范围

[公共服务包](../../compat/fixtures/mod-services/README.md)通过 `mod services --root ... --requests ... --probe namespace:name=path --report ...` 执行 start、逐个请求和 stop。报告 schema v1、后台协议 kk-services-v1；输出守卫保护包、请求、配置、检查点和探针文件。该工具验证服务，不启动网络监听。

回归覆盖[宿主生命周期/队列/预算](../../crates/kk-mod-host/tests/services_tests.rs)、[CLI 与真实检查点](../../crates/kk-cli/tests/command_tests/mod_services_tests.rs)、[后台等待期间持续世界与重启](../../crates/kk-runtime/tests/services_tests.rs)。HTTP/WS、自动周期调度、耐久 outbox、storage provider 与业务 provider 仍为后续范围，不据此标记 G27 或 M3 全域完成。
