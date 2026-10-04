# D4/D6 creep 资源动作槽（2026-10-05）

本批基于 `6d2e9eb`，承接 link 正常拒绝后开放 creep 的 transfer/withdraw。现行机制见[资源动作槽](../../architecture/creep-resource-slots.md)，局部拒绝通用语义见[Mod 执行](../../architecture/mod-execution.md#正常动作拒绝)。

## 本批变化

新增 `builtin:creep.transfer/withdraw` 两个 ObjectAction 槽，提供 before/wrap/replace/after 与正常拒绝。四个内建槽在共享目录中声明，三个资源动作共用拒绝资格表；内置 revision 和内容摘要随新描述改变，规则仍 25 条。creep 类型化对象、玩家 API 和 native 清洗继续使用原实现，没有把两个槽声明视为完整 creep 描述目录完成。

creep 动作改以 RoomCtx 派发，在进入 JS 前释放 Working 借用，原执行序、同名覆盖、压制与归属校验保持生效。整槽 Hosted 保存执行者和目标原始记录及事件长度，覆盖库存和实验室矿物绑定/释放容量。runtime 暂存所有 namespace，正常完成后移交，拒绝时丢弃。原始执行 ID/参数绑定到 native；creep 字段只读，store 合并与玩家投影共用函数，capacity 使用同一身体/boost 计算。

Rust 安装表按冻结槽分配动作处理器，三个动作各用对应 native；扩展动作装配跳过全部内建槽，新增 native 绑定通过显式安装检查。默认 Native 无局部快照、namespace 暂存或 JS 调用，但短借用和分派有成本，没有宣称正式零开销。

## 验证范围

新增 6 个真实 runtime 测试，普通玩家动作均分别运行 JSON/KKPV：

- 同 creep 同轮 transfer 后 withdraw，验证完整链序、旧 store 引用刷新、原 ID/目标/数量/资源种类恢复，以及双向事件归属。
- 两方向各在 before、wrap 前、replace、native 后 wrap、after 拒绝；撤销此前两个 Mod 的 state 写入、执行者库存及实验室首矿绑定/取空释放容量，剩余 after 不执行，玩家 Memory 正常提交。
- 三个 creep 竞争共享仓，中间动作在 native 后拒绝；首末分别实际搬运 20/10，中间资源与事件无残留，两个 namespace 只累计两次成功动作。晚期 RawMemory 写回失败保持完整 PlayerState/世界/Mod state，缓存重试玩家只执行一次。
- 同 creep 的前置 drop 分配资源对象、拒绝 transfer、后续 withdraw 和移动，验证局部恢复保留其他动作及 ID 分配结果，并计入掉落当轮衰减。
- 捕获只读字段写故障再拒绝、或外层捕获拒绝后再抛异常，均整轮失败；停用故障绑定后缓存重试回到 native。
- replace 与 wrap 不调用 next 正常完成，提交 state 并执行 after，保持取消语义。

新增 Rust 引擎测试同时安装三个资源槽的处理器，验证安装表分槽和三个不同的 native 中心。冻结测试锁定四槽目录、native 分派和三个可拒绝资源动作阶段。原 link 与自有动作回滚测试继续参与门禁。装配门矩阵另覆盖自有意图混接三个内建资源槽，即使有已安装 Replace 也必须拒绝。

## 门禁与提交前复审

最终门禁使用本批源码重建的 release CLI；screeps-ok 固定在 `34a630efa42908c117c19c25d2350fa2387bd633`，适配器已重建。机器记录与可执行文件 SHA-256 见[验收数据](d4-creep-resource-slots-2026-10-05.json)。

- `cargo build --release -p kk-cli`：通过。
- `KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check`：格式、workspace Clippy、Rust 测试、JavaScript workspace 测试及文档链接检查通过；Rust 648 通过、0 失败、11 忽略，Mod 链 47 通过，compat Node 测试 14 通过。
- `KKSCREEPS_BIN="$PWD/target/release/kkscreeps" node compat/scripts/ratchet.mjs`：门禁通过，2914 用例中 1918 通过、89 失败、907 跳过，通过下限 1918。另逐条比较 `HEAD` 与 `HEAD~1`，两者状态变化均为 0；没有新增跳过或豁免。完整结果见[兼容性报告](../../../compat/reports/kkscreeps.json)。

提交前复审核对 native 写入集合、整槽恢复边界、namespace 暂存、参数权威性、字段刷新、动作顺序及默认派发。发现自有意图在已安装 replace 时可能混接内建派发槽、创建成功后无法找到 ext 执行器；已在安装门显式拒绝，并增加三个资源槽的负面矩阵，随后重建 release 并重跑全部门禁。复审未发现剩余阻断问题。

本片不提供可写的 creep 库存、多对象资源写 API 或完整 creep 描述目录；特殊存储策略仍留在 native，没有独立处理槽。默认路径的短借用/分派成本未做正式性能测量，不能宣称零开销。
