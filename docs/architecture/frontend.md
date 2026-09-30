# 前端架构

本文定义 Web 客户端及可复用前端库的目标方案。当前 `apps/game`、`packages/shared`、`renderer`、`api` 和 `editor` 只有包声明与占位测试，没有 Vue 应用、渲染器、网络客户端或类型生成管线。技术选择依据见 [ADR-009](../decisions/ADR-009-frontend-stack.md)。

## 功能与技术选择

目标客户端支持房间观战、世界地图、控制台、代码管理、Memory 查看、市场和回放。渲染器、网络 API、编辑器与共享类型单独组织，便于其他工具按需使用。

| 层面 | 目标选择与边界 |
|---|---|
| 应用 | Vue 3、TypeScript、Pinia；应用状态与渲染器对象分开管理 |
| 构建 | Vite；应用和库分别配置构建入口 |
| 工程 | 现有 pnpm workspace；依赖版本在实际引入时锁定 |
| 编辑器 | Monaco，按需加载；仍需测量模块、网络和初始化成本 |
| 渲染 | PixiJS，具体版本与 WebGL/WebGPU 支持范围在实现时验证 |
| 测试 | 目标采用 Vitest 和按需的 Playwright；当前占位测试使用 node:test |

客户端实现自建，目标接口对照官方 HTTP 和 sockjs 协议。协议兼容尚未实装，不能假定社区工具现在已经能连接。官方 SVG 等资源的复用须先核对具体资产的许可证及分发条件；渲染代码不直接复用官方实现。

## 包职责与依赖

| 包 | 目标职责 | 目标依赖 |
|---|---|---|
| `@kkscreeps/shared` | 常量、网络协议与渲染数据类型 | 尽量避免运行时依赖 |
| `@kkscreeps/renderer` | 房间与地图场景、对象显示、动画和视口交互 | shared、PixiJS |
| `@kkscreeps/api` | 认证、REST、订阅、重连和订阅管理 | shared |
| `@kkscreeps/editor` | 模块编辑、智能提示、代码上传 | Vue、shared、api、Monaco |
| `apps/game` | 组装客户端界面与上述库 | editor、api、renderer、shared |

目标依赖方向为 `game → editor/api/renderer → shared`。渲染器不依赖 Vue 或网络客户端，以数据接口接收状态。框架无关不等于自动支持 Node：纹理加载和图形后端的运行环境须单独定义与验证。

库包计划独立分发；应用计划与服务器一起分发。当前并无已发布的库或嵌入二进制的 Web 构建产物。

## 渲染器设计

渲染器计划分为 core、scenes、objects、feed、assets 和 anim：分别负责渲染循环与视口、场景、对象注册、输入转换、资源管线和动画。公共入口为 `createRenderer`，Vue 适配层可提供 `useRenderer`。

对象注册表描述纹理、显示字段和绘制方式。扩展对象类型应通过注册契约加入，不能依赖客户端内硬编码的封闭枚举。服务端提供的渲染元数据仍需定义版本、权限和校验，不能直接当作可执行代码信任。

Pixi 实例由渲染器管理，Vue/Pinia 保存应用数据与控制状态；避免深度代理大型图形对象。地形纹理缓存、对象池、视口剔除和缩放分级是候选优化，须测量收益及内存占用。

目标输入接口如下，类型仅用于说明职责，尚未实现：

```ts
interface RoomFeed {
  setTerrain(room: string, terrain: TerrainData): void;
  applySnapshot(state: RoomSnapshot): void;
  applyDelta(delta: RoomDelta): void;
  setVisuals(visuals: RoomVisual[]): void;
}
```

全量快照用于初始订阅、切房和重同步，增量用于连续推进。协议还需定义版本、顺序、删除项、漏包和权限变化。实时订阅、历史回放和本地模拟可共用此接口，但各自仍需实现数据源与同步逻辑，不能因共用接口就视为已具备回放功能。

资源管线负责纹理图集、分辨率版本和缓存；在引入官方资产前保留授权出处。

### 当前实现与运行时边界（F2 R0–R4）

房间地形、静态对象、creep 运动/旋转/受击罩/死亡淡出已实现，具体规格与偏差见 [F2 方案](../plans/f2-renderer.md)。R5 完整动效、R6 RoomVisual/交互/API 冻结和 R7 性能验收尚未完成；上述世界地图、对象池及 Vue 集成仍为目标设计。

- **资产所有权**：`LoadedAtlas.dispose()` 释放本次加载的独立 Texture/TextureSource 与 ImageBitmap；spritesheet 页按 URL 共用租约，最后一个 renderer 释放时通过 Pixi Assets 卸载。加载失败等待在途任务结束后统一回滚。renderer 先拆场景，再销毁内核，最后释放资产；重复销毁共用同一个 Promise，并取消 feed、ticker、resize 与 DOM 输入订阅。
- **跨层同步**：`OverlayGroup` 集中管理同一对象在 lighting/effects 层的部件位移、父级淡出与 lighting dirty。join 仍负责部件自身的 alpha/visible。`values === null` 表示停用并使上次值失效，重新激活必须应用一次。
- **动画时间**：`AnimationClock` 使用 ticker 未截断的 `elapsedMS`，所有场景动画经过同一倍率入口；逻辑 tick 时间用于状态求值。`setPlaybackRate(rate)` 保留当前动画进度，0 暂停，回放器负责同步数据推送速率；此 API 随 R6 评审冻结。
- **快照与节点寿命**：快照取消旧 tween、清除死亡队列/受击罩/朝向状态、即时同步位置，并复用同 id/type 节点；缺失项立即销毁。增量仍使用运动插值与死亡淡出。
- **视口与坐标**：内核用 ResizeObserver 观察容器；相机在 renderer resize 时重新钳边并同步投影，绘制与拾取共用相同视口。lighting 显式把世界矩形 `[-cell/2, view-cell/2]²` 映射到 RT，再由输出精灵还原，避免半格偏移。

## 应用与分发

目标界面包含房间与对象详情、世界地图、表达式控制台、模块/分支管理、Memory 树、市场、账号和设置；回放需有历史数据与时间轴。认证、连接、订阅和编辑状态由应用管理，渲染内部状态留在渲染器。

开发模式计划通过 Vite 代理到本地服务器。生产模式计划将静态构建产物嵌入服务器二进制，但 `serve`、网络服务和嵌入资源流程目前均未完成。共用分发包减少版本分叉，仍须通过版本握手和协议测试验证兼容性。

## 常量与协议类型

现有常量路径是 `kk-runner/assets/constants.json → scripts/gen-constants.mjs → kk-core/src/constants.rs`，尚无 kk-core 构建脚本额外生成 JSON 或 shared 自动生成 TS 的流程。实现前端常量时应选定统一输入和生成步骤，避免另维护一份数值表。

目标网络类型由 Rust 请求/响应定义生成到 shared，可采用 JSON Schema 等中间表示；CI 应检查生成同步，并用真实响应验证可选字段、缺省值和数值范围。当前没有 axum 路由类型或已运行的生成门禁。

`RoomSnapshot`、`RoomDelta` 是按玩家权限过滤的网络投影，不直接暴露内部二进制快照或 Rust 内存。内部、查询和网络表示之间的字段关系需有独立契约，不能仅凭命名相近假定等价。

## 验收边界

先验证连接、房间观战、对象详情和代码/控制台操作，再扩展地图、市场和回放。验收须包含重连与漏包、视野变化、版本不匹配、大房间渲染和编辑器加载成本。

范围与任务安排见[路线图](../roadmap.md)。本页定义目标职责，不以占位包测试通过证明前端功能完成。
