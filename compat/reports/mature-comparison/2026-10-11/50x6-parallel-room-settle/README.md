# R09-lite 逐房并行结算——settle 相 −20ms（现行实现）

## 机制

`settle_rooms_stage` 满足资格时先尝试并行逐房结算：主线程克隆房候选
（World 非 Sync），连续块分片给 scoped 工作线程执行 `settle_room` +
逐房钩子，按房序归并（块序拼接即房序，免排序）；跨房尾部（portal/
边缘移动应用）两路径共用。**确定性构造**：gcl/power 为交换律累加
（跨线程 Mutex 共享候选，短临界区）；事件按房序归并；错误取最低房序
（与串行首失败同消息）。**串行回退**：并行段触碰依赖房序的共享路径
（ID 分配/回退、claim 读候选）即恢复共享态耦合量（gcl/power 快照、
ID 游标、fatal、诊断计数）后整拍串行重做——语义与纯串行完全一致。
**资格门**：有效工数 ≥2 ∧ 随机源为 Derived（注入序列仅诊断路径，整拍
串行）∧ 无 claimController 意图 ∧ 派发表无宿主槽。工数 = 环境覆盖
（KK_SETTLE_WORKERS）> 引擎覆盖（set_settle_workers）> 自适应
min(4, 房数/16, 核数−3)——**小核机器（2 核轻量服务器）自动串行**，
不付并行 CPU 代价。

类型手术：规则表（quotas/resources/buildings/contrib/extensions）
Rc→Arc；`TickState` 共享候选 `Rc<RefCell>`→`Arc<Mutex>`；贡献链载荷
Rc→Arc 且闭包加 Send+Sync；**宿主执行器移入线程本地注册表**
（派发表只携带 u64 句柄，类型可跨线程；JS realm 单线程语义保持——
工作线程查表必空报错兜底）。宿主句柄由 `HostedRegistryGuard` 字段守卫
随引擎销毁/换装退役——**泄漏登记会让已销毁 V8 realm 保持存活并崩后续
realm 的 HandleScope（冷重启测试复现打回）**。

## 等价证据

- 单元等价锁（parallel_tests 5 项）：并行/串行同输入**逐房二进制快照
  逐字节一致** + 玩家/事件/ID 游标一致；ID 分配触发回退后与纯串行一致；
  claim 意图/注入随机/少房间各自强制串行。
- B0 端到端：串行取证窗与并行冒烟窗 **300 房终态逐字段全等**；耦合面
  并行前后完全一致（id_allocs/claim/interroom 全零，gcl 220.8/拍交换律）。
- 棘轮 2905/9/0 **转红 0**；kk-engine 140、kk-runtime 全套（lib 53、
  mod_chain 495、embed 102）、sim 106、完整无参数 check。

## 冻结 ABBA（old=71a37eb，new=工作树，2 丢弃+O,N,N,O，5+100，并发 4）

| 相 | old | new | 判定 |
|---|---|---|---|
| **settle P50** | 107.5/115.2/160.3 | **85.4/91.4/97.6** | **相邻对双确认 + 丢弃对一致（−20~−25ms）** |
| wall（凉丢弃对） | 249.1 | **229.0** | −20ms（run 相 106.8 完全一致） |
| wall（测量对） | 265.7 | 265.2 | 序列热漂下打平 |
| CPU core-ms | 102-144k | 108-128k | **+12~18%（以核换墙钟的真实代价；对 xxs CPU 比仍 0.69-0.83）** |
| RSS 峰 | ~3.3G | ~3.3G | 持平 |

并行路径 106/106 拍全启用。

## 标尺（当日多窗，机器随连续窗口热化）

| 窗 | kk | xxs | 比值 | kk 分相 |
|---|---|---|---|---|
| 丢弃 | 260.3 | 232.5 | 1.1196 | proj 40.8/run 117.3/settle **96.9** |
| 测量 | 249.6 | 214.2 | 1.1651 | proj 39.7/run 111.7/settle **95.2** |
| W=2 | 264.6 | 238.9 | 1.1074 | settle 106.6（劣于 W4） |
| 末尾双窗 | 381/413 | 330+ | — | 热毁废弃 |

settle 相 111.6（昨日串行）→95-97 稳定兑现；当日串行对照（ABBA old 窗
107-160）同受热态影响，跨日绝对值不可比。**run 相"膨胀"经查为当日机器
热态**（QoS 降 E 核试验未改变 run，ABBA 丢弃对 run 完全一致）——QoS
试验已撤（settle 反而变慢）。标尺比值待凉窗多窗中枢复测（目标协议要求
的验收口径本就如此）。

## 复现

```bash
cargo build --release -p kk-cli
KK_SETTLE_WORKERS=4 ./target/release/kkscreeps compare-probe \
  --fixture /tmp/l1b-fixture.json --warmup 5 --ticks 100 --concurrency 4 \
  --report /tmp/par-report.json
# ABBA 驱动 /tmp/par-abba.py（old=/tmp/kkscreeps-par-old）
```
