# 前端架构（Web 客户端与渲染器）

本文覆盖游戏客户端的完整技术方案：包怎么拆、渲染器内部怎么设计、客户端应用长什么样、类型怎么与 Rust 侧保持一致。服务端架构见本目录其他文档。

## 一、总览

前端要交付两类东西：

1. **游戏客户端**（一个 Vue 3 应用）：观战房间、世界地图、控制台、代码编辑上传、市场、回放播放器——对标官方客户端的功能面；
2. **可复用的库**：渲染引擎、API 客户端、常量与类型——单独成包发布，让社区拿去做 bot 可视化、分析工具，而不必依赖我们的整个客户端。

与后端的关系不变：**网络协议保持与官方兼容**（HTTP 接口 + sockjs/WebSocket 订阅推送，见 [storage.md](storage.md) 网络层一节）。协议兼容是验收关卡之一，但它约束的是"接口"，不约束客户端实现——所以客户端我们完全自建，不基于官方客户端开发。

**不复用官方渲染器的代码**（决策见 [ADR-009](../decisions/ADR-009-frontend-stack.md)）：官方渲染器基于旧版 PixiJS 和一套自研表达式引擎驱动渲染，技术栈老旧。我们用最新版 PixiJS 自建，但**复用两样东西**：官方开源的美术资源（全套 SVG，宽松许可）；以及它"数据驱动渲染"的设计思想（对象长什么样由元数据描述，而非硬编码）。

## 二、技术栈与工程形态

| 层面 | 选型 | 说明 |
|---|---|---|
| 框架 | Vue 3（组合式 API）+ TypeScript | |
| 构建 | Vite | 库包用 library 模式，应用用常规模式 |
| 包管理 | pnpm workspace | 单 monorepo：Rust workspace 与 pnpm workspace 共存于仓库根（Cargo 与 pnpm 互不干扰，这是 Tauri 等混合项目的常见形态） |
| 代码编辑器内核 | Monaco（已定） | 智能提示对写 TS 的玩家价值大；包体较大，客户端按需加载（动态 import 独立 chunk），非编辑页面零成本 |
| 渲染 | PixiJS 最新大版本（v8+，WebGPU/WebGL 双后端） | 实现期锁定当时的具体版本 |
| 单元测试 | Vitest | |
| E2E | Playwright（M3 后按需） | |

不引入 turborepo/nx 这类编排工具：包数量少、Vite 本身够快，先用根目录 package.json 的脚本统一 dev/build/test，将来有需要再加。

## 三、包划分

```
kkscreeps/
├── Cargo.toml / rust-toolchain        ← Rust workspace
├── crates/ …                          （9 个引擎 crate，见 overall.md）
├── compat/                            （screeps-ok 适配器，TS 包，见 ADR-010）
├── pnpm-workspace.yaml / package.json ← JS workspace（仓库根）
├── apps/
│   └── game/                          # 客户端应用（唯一的 app）
├── packages/
│   ├── shared/                        # @kkscreeps/shared：常量 + 协议类型
│   ├── renderer/                      # @kkscreeps/renderer：渲染引擎（框架无关）
│   ├── api/                           # @kkscreeps/api：网络客户端（浏览器/Node 通用）
│   └── editor/                        # @kkscreeps/editor：代码编辑器 Vue 组件
├── spikes/  docs/  …
```

| 包 | 职责 | 依赖 | 发布 |
|---|---|---|---|
| `@kkscreeps/shared` | 全部游戏常量（443 个，**从 Rust 的 kk-core 生成**，见第六节）+ 网络协议类型（DTO）+ 渲染器与客户端共用的游戏类型 | 零运行时依赖 | 是 |
| `@kkscreeps/renderer` | PixiJS 渲染引擎：房间场景、世界地图场景、对象渲染、动画、视口交互 | `shared`、`pixi.js` | 是（核心可复用资产） |
| `@kkscreeps/api` | 类型化的网络客户端：REST + 订阅推送、认证、自动重连、订阅管理 | `shared` | 是（浏览器/Node 通用，bot 工具可用） |
| `@kkscreeps/editor` | 代码编辑器组件（Monaco 内核：多模块管理、语法高亮与智能提示、提交上传） | Vue、`shared`、`api`、`monaco-editor` | 是 |
| `apps/game` | 客户端应用：组装以上全部，提供完整 UI | 全部包 | 否（构建产物嵌入服务器二进制分发） |

依赖方向单向：`game → editor/api/renderer → shared`。渲染器**不依赖** Vue 和 api——它只吃数据（见下），这是它能被 React 项目或 Node 工具复用的前提。

## 四、渲染器架构（@kkscreeps/renderer）

### 分层

```
packages/renderer/src/
├── core/   引擎骨架：渲染循环、场景管理、视口（平移/缩放/拾取）、对象池
├── scenes/ 房间场景（50×50 格）、世界地图场景（跨房间缩略）
├── objects/ 对象渲染注册表 + 各游戏类型的渲染实现
├── feed/   状态输入适配：把快照/增量/玩家绘制翻译成场景变更
├── assets/ 资源管线：官方 SVG → 纹理图集、缓存
├── anim/   动效系统：移动补间、受击反馈、进度条
└── index.ts 公共 API（createRenderer 等）
```

### 关键设计

1. **框架无关、数据驱动**。渲染器是一个纯 TS 库，输入输出都是普通数据，不知道 Vue 的存在。每种游戏对象"长什么样"由对象渲染注册表里的元数据描述（官方渲染器的好思想），新增对象类型或换皮肤不需要改引擎骨架。
2. **响应式边界**。渲染器内部拥有自己的渲染循环（Pixi 的 ticker）；Vue 只负责挂载容器和传指令，通过一个薄控制器接口通信。**绝不让 Pixi 对象进入 Vue 的响应式系统**（代理包装会毁掉性能）。
3. **输入接口与后端变更集同构**：

```ts
interface RoomFeed {
  setTerrain(room: string, terrain: TerrainData): void;
  applySnapshot(state: RoomSnapshot): void;   // 全量（首帧/切房间）
  applyDelta(delta: RoomDelta): void;          // 增量（每 tick 推送）
  setVisuals(v: RoomVisual[]): void;           // 玩家绘制（RoomVisual API）
}
```

   同一个接口喂实时订阅、回放历史、本地模拟器三种数据源——回放播放器和 `kkscreeps sim` 的观战界面因此免费获得。
4. **性能要点**：地形每房间静态、烘焙成一张纹理不再逐格绘制；对象用池复用；按视口剔除；缩放分级（远看世界地图时降细节）。增量更新直接对应后端推的变更集，没有全量 diff。
5. **资产管线**：官方 SVG 资源进仓库，构建脚本生成纹理图集与多分辨率版本；元数据（哪个对象用哪张图、怎么排布）是 TS 数据，可被服务器下发的渲染元数据扩展（保留官方"服务端注入渲染元数据"的 mod 扩展点）。

### Vue 集成

提供 `useRenderer(containerRef)` 组合式函数，返回控制器（切房间、订阅 tick、捕获点击对象）。其他框架的用户直接用 `createRenderer` 底层 API。

## 五、客户端应用（apps/game）

**视图清单**（对标官方客户端）：房间观战（渲染器 + 对象详情面板）、世界地图（渲染器地图场景 + 房间状态着色）、控制台（双向：发送表达式/显示输出与错误）、代码管理（模块树 + 编辑器 + 分支 + 提交历史）、Memory 查看器（树形 + 监视）、市场（挂单/成交/历史图表）、排行榜与个人资料、回放播放器（时间轴 = 渲染器的 feed 接口 + 快照历史）、登录与设置。

**状态管理**：Pinia 管应用状态（认证、连接与订阅、控制台流、代码、市场数据、界面偏好）；渲染器内部状态不进 Pinia（见响应式边界）。

**开发与分发**：

- 开发模式：Vite dev server 代理到本地 `kkscreeps serve`，热更新；
- 生产分发：`apps/game` 构建产物经 rust-embed 嵌进服务器二进制——**单二进制自带完整 Web 客户端**，部署仍是"一个文件"；客户端与服务器版本天然匹配（启动握手带版本号）。

## 六、类型的单一事实来源

前后端最大的隐患是"同一个概念两处定义、改了这边忘那边"。对策：

1. **游戏常量**：唯一源头在 Rust 的 `kk-core`（它又是从官方 `constants.js` 生成的）。kk-core 的构建脚本额外产出 `constants.json`，`@kkscreeps/shared` 的构建脚本消费它生成 TS 常量。CI 校验两边同步（Rust 侧改了常量而 TS 侧没重新生成则失败）。
2. **协议 DTO**：后端 axum 的请求/响应类型经**生成管线**同步到 `@kkscreeps/shared` 的 TS 类型（Rust 侧类型导出 JSON Schema → 脚本生成 TS 类型 → CI 校验"生成物是否新鲜"，改了 Rust 忘了生成即失败）；契约测试作为补充（CI 用真实响应跑 TS 类型断言），防生成 schema 本身与实现漂移。
3. **快照/增量结构**：渲染器吃的 `RoomSnapshot/RoomDelta` 与 Rust 侧 `kk-schema` 的布局对应——注意这**不是**内部二进制快照直接暴露（那是宿主内存布局），而是它的"网络投影"，在 backend 序列化时生成，TS 类型放 shared。

## 七、排期影响

自建客户端比"复用官方渲染器"工作量大，但换来可维护性与可复用性。路线图调整：M3 交付"自建客户端可玩 + 官方客户端可连接"（协议兼容仍是关卡），渲染器包在 M3 先达到"房间观战可用"，世界地图/回放播放器/市场图表等视图允许滑到 M3 后期或 M4。
