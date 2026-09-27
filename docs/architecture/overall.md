# 总体形态与模块划分

## 一个程序，三种运行方式

我们只构建一个可执行程序 `kkscreeps`，通过子命令切换运行方式。这带来一个关键保证：**玩家在本地测试用的引擎和线上跑的引擎是同一份代码**，不存在"本地测试通过、上服行为不同"的问题。

| 子命令 | 用途 | 说明 |
|---|---|---|
| `kkscreeps serve` | 运行游戏服务器 | 完整服务：游戏循环 + 网络接口，替代官方的 5 进程 + 2 中间件 |
| `kkscreeps sim` / `kkscreeps test` | 本地模拟与集成测试 | 无网络、内存即存储，快速跑完上千个 tick；见[开发套件设计](../designs/dev-kit.md) |
| `kkscreeps import` / `export` 等 | 运维工具 | 导入官方世界数据、生成地图、管理机器人账号 |

单机形态下所有模块在同一个进程内协作，模块之间就是普通的函数调用。两个例外见 [README 的设计原则 4](README.md)：面向公网的生产部署可把沙箱（kk-runner）按组拆分成多个进程（安全缓解）；多机扩展时把"游戏引擎"和"沙箱"分组部署到多台机器，通过存储接口的消息通道协调——接口预留，按需启用。

## 代码模块（crate）划分

Rust 的 workspace 把代码组织成多个 crate（可以理解为编译单元/模块包），每个有明确职责：

```
kkscreeps/
├── Cargo.toml         Rust workspace（与下方的 pnpm workspace 共存于同一仓库）
├── crates/            Rust 引擎模块（与 JS 侧互不干扰）
│   ├── kk-core        基础数据：游戏数值表、房间名编解码、地形编解码、随机数
│   ├── kk-schema      房间快照的编码格式：字段布局、版本升级、读写器
│   ├── kk-world       世界模型：房间快照的内存表示、空间索引、对象类型定义
│   ├── kk-engine      游戏循环：意图执行、移动仲裁、战斗、能量、市场结算
│   ├── kk-runner      玩家代码沙箱：deno_core 封装、CPU/内存限制、模块加载
│   ├── kk-storage     存储：内存缓存 + 定期写盘；跨机部署时的 Redis 等后端
│   ├── kk-pathfinder  寻路：复用官方 C++ 源码（保证路径逐点一致）
│   ├── kk-backend     网络层：HTTP 接口 + WebSocket 推送（兼容官方协议）
│   └── kk-cli         命令行入口：组装以上模块
├── compat/            兼容性验收：screeps-ok 适配器、官方 API 对照检查
├── pnpm-workspace.yaml / package.json   JS workspace（仓库根）
├── apps/
│   └── game           客户端应用（Vue 3，构建产物嵌入服务器二进制）
├── packages/
│   ├── shared         @kkscreeps/shared：常量（从 kk-core 生成）+ 协议类型
│   ├── renderer       @kkscreeps/renderer：渲染引擎（PixiJS，框架无关）
│   ├── api            @kkscreeps/api：网络客户端（浏览器/Node 通用）
│   └── editor         @kkscreeps/editor：代码编辑器 Vue 组件
├── spikes/            技术验证工程（已完成使命，保留作证据）
└── docs/              本文档
```

前端（apps/ 与 packages/）的详细设计见 [frontend.md](frontend.md)。

依赖方向（上层依赖下层，不允许反向）：

```
kk-cli ──→ kk-backend ──→ kk-engine ──→ kk-world ──→ kk-schema ──→ kk-core
              │               │            │
              │               └──→ kk-runner（沙箱）
              │               └──→ kk-pathfinder
              └──────────→ kk-storage
```

两条纪律：

1. **kk-engine 不依赖任何具体存储实现**。引擎只认识"房间快照"和"意图（intent，玩家下达的动作指令）"这些抽象数据，通过 kk-storage 定义的接口读写。这样"单机内存版"和"多机分布式版"对引擎代码来说没有区别。
2. **玩家可见的游戏规则全部在 kk-engine 与 kk-world**。沙箱（kk-runner）只负责"忠实地把世界给玩家看、把玩家的指令收回来"，不做任何规则判断。这条边界保证兼容性验收时能明确归因。
3. **kk-runner 对引擎暴露一个薄的沙箱接口（trait）**。不是为了换引擎后端（已定 deno_core 唯一），而是为了测试：引擎单元测试用一个返回固定指令的假沙箱实现，不必拖着真 V8 跑。一个真实现 + 一个测试假实现。

## 各模块的来源

不少模块可以直接"抄"而不是"写"，这是三轮调研换来的效率：

| 模块 | 来源 | 方式 |
|---|---|---|
| kk-core 数值表 | 官方 `common/lib/constants.js`（1591 行纯数据） | 写脚本自动转成 Rust 常量，不手抄 |
| kk-core 意图校验规则 | 官方 `common/lib/system.js` 的字段白名单 | 逐条移植 |
| kk-pathfinder | 官方 `driver/native/src/pf.cc`（613 行 C++） | FFI 直接复用（FFI 即外部函数接口，让 Rust 直接调用 C/C++ 编译出的代码；直接用官方源码保证路径逐点一致） |
| 沙箱内的玩家 API 层 | 官方 `engine/src/game/` + xxscreeps `game/` | 移植 + 用实验验证过的"共享内存直读 + 惰性加载"模式 |
| 房间快照设计 | xxscreeps 的二进制格式（`engine/schema/`） | 思路照搬，实现改在 Rust 编译期完成布局计算 |
