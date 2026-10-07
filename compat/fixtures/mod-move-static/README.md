# 静态放行公共 JS 包

`ghost` 用 before 层的 `ctx.precheck.allowStatic(id)` 放行全部请求的静态障碍；`seal` 用 after 层的 `block(id)` 禁止进入 `targetX >= 30` 区域。它们分别注册公共 SDK，不依赖 Rust 对包 ID 的分支，组合后区域否决仍生效。

静态放行不能绕过动态 creep 占格、竞争、疲劳/MOVE 或后续障碍策略。`moveTo`/PathFinder 仍按原生地形/结构寻路；场景使用方向 `move` 展示能力。namespace 计数经 `ctx.state` 保存，不依赖模块全局累计。

运行方式与位置对照见[真实场景](../sim-mod-move-static/README.md)，契约见[移动预检](../../../docs/architecture/mod-move-precheck.md)。
