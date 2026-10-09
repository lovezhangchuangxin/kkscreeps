# 后台服务公共 SDK 示例（当前 G27.a 首片）

`audit` 包的 services.js 同步注册 audit:summary 服务；start/request 等待显式授予的 audit:sample 文件探针，stop 返回进程内调用次数。server.js 是空玩法注册，不修改世界。所有行为来自公共 SDK，没有按包 ID 特判。

```bash
target/debug/kkscreeps mod services --root compat/fixtures/mod-services --requests compat/fixtures/mod-services/requests.json --probe audit:sample=compat/fixtures/mod-services/sample.txt --report /tmp/services.json
```

命令构造真实最小世界，运行后台独立线程/V8，输出 start、两次 request、stop 与 world_unchanged。加入 `--checkpoint path` 可读取按同一包身份恢复的已提交世界；报告须在包根外，且不能覆盖任何输入或探针文件。probe 只读常规文件，回调读取 ctx.view 的固定副本；服务响应不会写入玩法。

services 清单入口与 entry/player 分开；注册必须同步，request 必填，start/stop 可省略。可 await ctx.probe，不能装载玩法 SDK，也没有默认 fetch/任意文件访问。进程内 calls 在后台重启后从零开始，未完成队列不持久化。使用机制与限额见[后台服务契约](../../../docs/architecture/background-services.md)；持久任务/outbox/storage/网络监听尚未实现。
