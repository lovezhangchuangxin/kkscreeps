# G02.d1 静态移动放行（2026-10-07）

基线 `ee78894`。本记录交付请求登记与仲裁的静态放行切片，G02 父项仍部分完成。现行接口见[移动预检](../../architecture/mod-move-precheck.md)，示例见[ghost/seal](../../../compat/fixtures/mod-move-static/README.md)。机器结果见[同名证据](g02d1-static-movement-2026-10-07.json)。

## 实施与复审

复用 `room.movePrecheck`，新增 `allowStatic`，输入分列登记拒绝快照与当前静态初判。Hosted 登记暂存拒绝请求，完成后过滤默认未放行和全部显式否决；正常拒绝丢弃 JS 决策并兑现登记快照，避免障碍后续删除导致原生回退漂移。无贡献路径维持直接拒绝。

静态判定拆为共用模块，动态 creep 占格始终由取消链复核。缩减矩阵每格只含胜者，放行不会从同格落选者泄漏；准入瘦预演复制静态许可并运行正式取消链。显式 block 不能被后续 allowStatic 清除，后续障碍槽也可否决。候选集合不进入检查点，无新持久写入域；revision v14→v15，旧计划无自动迁移。

本轮自审检查了：两登记入口、单请求覆盖、移动槽正常拒绝快照、整房 decline、竞争输入过滤、胜者许可归属、预演复制、动态占格与安全模式分支、对象 tick 执行和出生顺序、跨房过渡、能力门及写故障留存、namespace 暂存/重试与身份恢复。出生在仲裁之后消费矩阵，不会读到默认已拒绝请求。跨房到达仍不查目的格占格/地形，这是既有过渡语义，未借本片扩大。

测试夹具初次运行发现两个测试准备问题：停用后 VM 待恢复时不能管理插入；预演覆盖表传空会表示无人获准。已修正测试准备顺序与实际准入覆盖，非产品故障。另补入落选者许可隔离和双玩家/并发对照；修正域外 ID 反例为真实 kk 前缀格式，避免重复测试解析拒绝；移除模块拆分遗留的两个未使用导入，去掉预演不读的登记快照复制。当前无已知阻塞项，完整门禁待确认。

## 已执行定向验收

- `cargo test -p kk-engine --lib precheck::tests`：最初新增三例后 7 通过；胜者许可隔离为后加例，8 例已随两轮完整门禁确认。
- `cargo test -p kk-runtime --test mod_chain_tests move_static`：最初八例 8 通过；双玩家/并发为后加例，9 例已随两轮完整门禁确认。
- `cargo test -p kk-cli --test sim_tests move_static`：1 通过；四场景 × 双通道、保存退出/新进程加载、namespace 计数/四身份、各保存与续跑的回放 passed，99.19 秒。

完整门禁、兼容 ratchet 与提交结论见下节。未修改或新增 Rust doctest。

## 复审清偿与最终结论（2026-10-07）

三路分域复审（引擎逻辑、mod 接线、测试/夹具/文档）一致结论：设计声称全部核实、无 P1/P2。复审清偿四处：`Working::apply_movement_precheck_blocks` 死包装改为 `apply_movement_precheck_decisions` 统一入口并回接 contest 调用点（恢复 Working 包装层惯例）；`ContributionDispatch::run_precheck` 外层注释同步「正常拒绝也返回决策」的新语义；架构文档/台账/机器证据中「内建槽 18」为陈旧数字（pull-registration 与 moveCost 两批后实为 20，本批未增槽），统一改为 20；host_movement trait 默认方法块多余空行。复审另登记两条不阻塞观察：moveTo 不自动穿墙目前是架构推断（结算侧放行不改变玩家面寻路），无回归测试，随 G02.d2 盘点钉住；ratchet 两条「豁免条目未命中」警告为历史豁免表陈旧条目提示，非本批引入。

- 完整门禁 `pnpm run check`（无参数）：清偿前后各通过一轮；清偿后 Rust 1213 通过/12 既有忽略、compat JS 15、renderer 1，文档链接 333 文件、源码规模 626 文件。
- release ratchet（清偿后 release 二进制）：1918/2914 通过（floor 1918 恰达标）、相对 HEAD~1 基线零转红（豁免 0、待归因 0）。
- 提交状态：用户确认后已随本批提交；提交 ID 由下次接手从 Git 补录。

## 剩余

G02.d2 先盘点疲劳/MOVE 的方向 move 与 moveTo 玩家门，以及 PathFinder/寻路摘要消费者；当前方向 move 在正常身体/疲劳下不挡静态障碍，因此本片直接生效，但 moveTo 不会自动规划穿墙。不能把通用服务器 JS 同步回调进玩家 VM，需冻结声明或纯数据规则。同类玩法无需修改 Rust，但这不代表全部移动玩法族已经开放。
