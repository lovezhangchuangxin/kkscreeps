# 主项目结构治理（2026-10-05）

基于 `7815529` 盘点，发现 19 个超过 1000 行的代码文件：9 个主项目生产文件、8 个主项目测试文件，另有固定版本 Lodash 和独立 inspector 实验。前两类均完成按职责拆分；后两类保留，范围与理由见[现行源码组织](../../architecture/source-organization.md)。

本轮只治理结构：保留玩家行为、候选发布与失败恢复边界、公开 API，以及集成套件的串行调用方式。Rust 拆出真正的子模块，通过原入口重导出既有公开类型/函数；JS 拆为独立 Memory、市场和 Game 闭包，桥函数在加载期捕获。新增主项目 1000 行规模检查并接入完整门禁。

## 拆分清单

下表的“入口行数”是原文件拆分后的行数，迁移代码在独立职责子模块内，不代表删除了实现。47 个 Rust 子模块和两个 JS 领域模块均不超过门槛；全体主项目 305 个源码/测试文件的最大行数为 1000。

| 原入口 | 原行数 | 拆分后入口行数 |
|---|---:|---:|
| `kk-schema/src/kkpv.rs` | 2581 | 298 |
| `kk-engine/src/settle/mod.rs` | 2255 | 435 |
| `kk-runtime/src/embed.rs` | 2170 | 549 |
| `kk-runner/src/prelude/30_game/game.js` | 1456 | 779 |
| `kk-engine/src/intent.rs` | 1422 | 842 |
| `kk-mod-host/tests/register_tests.rs` | 1390 | 290 |
| `kk-runner/src/sandbox.rs` | 1354 | 727 |
| `kk-world/src/kkpv.rs` | 1336 | 490 |
| `kk-engine/tests/settle_tests/m2b_contrib_link_family.rs` | 1320 | 200 |
| `kk-engine/tests/settle_tests/c3_ext_intent_family.rs` | 1284 | 301 |
| `kk-engine/src/settle/market.rs` | 1208 | 527 |
| `kk-runtime/tests/embed_tests/dual_room_live_fire.rs` | 1156 | 491 |
| `kk-engine/src/settle/contrib.rs` | 1130 | 421 |
| `kk-runtime/tests/embed_tests/embed_channels_and_families.rs` | 1095 | 12 |
| `kk-engine/tests/settle_tests/structures_and_attack.rs` | 1071 | 371 |
| `kk-runner/tests/sandbox_tests/watchdog_and_memory.rs` | 1029 | 193 |
| `kk-runtime/tests/mod_chain_tests.rs` | 1014 | 155 |

## 验收与复审

- 最新源码 `cargo build --release -p kk-cli`：通过。
- `KKSCREEPS_BIN="$PWD/target/release/kkscreeps" pnpm run check`：格式、workspace Clippy、Rust、JavaScript、文档链接及规模门禁全部通过；Rust 648 通过、0 失败、11 忽略，Mod 链 47 通过，compat Node 14 通过，数量与治理前一致。
- `KKSCREEPS_BIN="$PWD/target/release/kkscreeps" node compat/scripts/ratchet.mjs`：门禁通过，2914 个用例中 1918 通过、89 失败、907 跳过，通过下限 1918。逐条对比 `HEAD` 和 `HEAD~1`，状态变化均为 0，没有新增跳过或豁免；完整结果见[兼容性报告](../../../compat/reports/kkscreeps.json)。
- `cargo doc -p kk-schema --no-deps`：成功生成，无警告，保留的公开路径及协议文档链接可解析。三个 JS 领域文件通过 `node --check`。
- 文件规模门禁额外验证 1000/1001 行边界及未跟踪新文件；前者通过、后者拒绝，临时夹具已清理。

固定 screeps-ok 版本、release 二进制 SHA-256、原始路径清单和计数见[机器验收记录](source-structure-2026-10-05.json)。锁文件和历史 JSON 测量证据不属于源码治理对象。

提交前复审核对公开 API 重导出、内部可见性、相对资源路径、JS 加载与捕获顺序、候选提交/重试边界，以及串行场景的原调用顺序。迁移中发现并修复子模块路径、共享深度常量的归属、受限方法可见性及注释绑定问题，随后使用最终源码重建并重跑全部门禁；未发现剩余阻断问题。KKPV 合并改用现有只读方法，不扩大视图字段可见性；JS RawMemory 重建、CJS 重绑、Memory 懒 getter 安装的顺序保持原样。

本轮未做正式性能测量，不宣称性能收益；第三方 Lodash（12351 行）和 inspector 独立实验（1394 行）字节保持不变，例外范围已明确记录。
