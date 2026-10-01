# 房间放大黑屏：根因、修复与短 GPU 回归（2026-10-01）

本轮从 `f2-renderer@9a0662e` 开始，仅修改 renderer 工作树。用户要求深查放大黑屏并修复，继续暂缓长测。本记录不替代 R7 冻结性能预算、完整矩阵或 10k 内存验收。

## 复现与根因

真实房间 `official-room` 初始快照（E54S13，time=24242100，312 对象/11 creep/0 visual），
`/?fixture=official-room&view=4000,3500,2`，1280×720 CSS、resolution=2，WebGPU：
房间画布全黑，但 HUD、相机 zoom=200%、lighting=real 仍正常。
控制台反复报告 `TexturePool: 32768x32768` 超过池键的 32767px 上限，随后触发 Pixi 警告限流。
见 [修复前截图](renderer-zoom-data-2026-10-01/before-webgpu-zoom2.jpg)。

不是相机溢出，也不是 lighting 的 multiply：无沼泽 `plains` 同配置 zoom=4 正常。
根因在 [terrain.ts](../../../packages/renderer/src/scenes/terrain.ts)：两个 5000×5000 世界尺寸的沼泽噪声
使用 `noise.mask = overlay`，进入 Pixi v8 自动 AlphaMaskPipe。它的 MaskFilter 强制
`clipToViewport=false`，以被遮对象的全房 bounds 分配滤波输入。zoom=2、resolution=2 时
请求约 20000×20000 物理像素，再走池的尺寸选择得到 32768²，超出实测设备限额。
仅一张 RGBA8 的 32768² 基础颜色平面理论上就需 4 GiB；这不是实际 VRAM 测量，也不包含 MSAA 等附加资源。
隐藏 lighting、降低 zoom 上限或屏蔽日志均不能解决此路径。

官方本地原版实际使用 Pixi 7.4.3：Filter 默认 `autoFit=true`，FilterSystem 将 sourceFrame
与当前投影 viewport 相交。官方沼泽同样使用精灵 mask，所以它不会按放大后的全房尺寸分配滤波输入。
官方 lighting 还有显式 screen filterArea，但那不是本次故障路径。

取证均经 `gh api repos/.../contents?...ref=...`，未抓站点锚点；下列为固定源码及 blob SHA：

| 源码 | SHA / 关键事实 |
| --- | --- |
| [官方 terrain](https://github.com/screeps/renderer/blob/a2db4a76bb8f4c70e0c2a9e3b7d22a35a8c6b504/engine/src/lib/processors/terrain.js) | `d4a6ca6f53b96321826f6eb771d97956c0050d9c`；双沼泽噪声挂精灵 mask |
| [v8 MaskFilter](https://github.com/pixijs/pixijs/blob/v8.21.0/src/filters/mask/MaskFilter.ts) | `a6d7dbc0de8c6f891f26107580644c4da172c29b`；构造强制不裁 viewport，apply 按当前 filter frame/mask transform 计算 UV |
| [v8 AlphaMaskPipe](https://github.com/pixijs/pixijs/blob/v8.21.0/src/rendering/mask/alpha/AlphaMaskPipe.ts) | `aa80131fbb010005e87b24de12bf0418cec79791`；自动 mask 使用同一滤镜，继承 resolution/antialias |
| [v8 FilterSystem](https://github.com/pixijs/pixijs/blob/v8.21.0/src/filters/FilterSystem.ts) | `484672e79f3086df979957f5fc9144f90e6b2c63`；clip=true 每次与当前 rootViewPort/rootResolution 相交 |
| [v7 Filter](https://github.com/pixijs/pixijs/blob/v7.4.3/packages/core/src/filters/Filter.ts) | `cd983570441ee55da4221ed4a1fc2459f5e427ba`；autoFit 默认 true |
| [v7 FilterSystem](https://github.com/pixijs/pixijs/blob/v7.4.3/packages/core/src/filters/FilterSystem.ts) | `9f6a530c80fbdd6302c4c9efc4f02c41646c3283`；sourceFrame.fit(sourceFrameProjected) |

源码没有解释 v8 禁裁剪的初始设计理由，本轮不推断其通用行为有错。这里只为两个无 padding、
仅主屏消费的自有沼泽噪声限定范围。

## 修复与所有权

两个噪声改为自有 MaskFilter，通过 noise.filters 挂载；构造后设置公开属性
`clipToViewport=true`（构造 options 会被 Pixi 覆盖）。继续使用原 red 通道、inverse=false、
继承 resolution/antialias 的 shader；mask Sprite 的 renderable/measurable=false 保持原自动路径行为。
不手写 screen filterArea：每次 render 的当前 viewport 裁剪与 mask UV 矩阵会自然跟随平移、缩放、resize。
MaskFilter 为 Pixi 根导出，但其声明标为 internal；用法限定于当前8.21版本及上述源码验证，升级Pixi时须重跑此回归。

重建/销毁时先摘掉所有 noise.filters，再显式销毁自有滤镜及 TextureMatrix/BindGroup 订阅，
最后销毁白罩私有纹理。共享 noise2 与共享 shader programs 不销毁。
新增单测覆盖同地形 hash 短路、清场顺序、重建与最终销毁；共享噪声使用独立 Texture 验证未被误毁，
不依赖 Texture.WHITE 的 destroy NOOP。公共 createRenderer API/d.ts、normal lighting 与 zoom 上限不变。

## 环境、方法与证据

硬件：Apple M4 / Mac16,12 / 24 GiB；macOS 15.7.3 (24G419)。Codex 内嵌 Chromium 的 UA
归档在原始 JSON；物理 devicePixelRatio=2。测试显式将 renderer resolution 切为 1/2，
并非改动系统 DPR。实际 WebGPU device maxTextureDimension2D=8192；WebGL MAX_TEXTURE_SIZE=16384。
官方演示 `http://localhost:3000/index.html` 只读观察，官方 checkout/引擎没有改动。

可复现命令（renderer 工作树）：

```sh
rtk proxy pnpm --dir packages/renderer exec vite --mode perf --host 127.0.0.1 --port 5175
```

浏览器打开 `/perf.html?fixture=official-room&zoom=1`，WebGL 加 `&backend=webgl`。
[zoom-verification.ts](../../../packages/renderer/dev/zoom-verification.ts) 停 ticker、固定同一动画相位，
仅在安全 zoom=.13/.2 临时切回原自动 sprite mask，比较修复后的原始 RGBA。每组确认恰有两 mask、
图像尺寸/数组长度一致，再隐藏噪声作正对照，防止两路径共同漏画。读回来自同尺寸显式 RenderTexture，
不是主 framebuffer；主画布人工复现/修复截图另行留档。

每后端 70 个高倍率案例：zoom=1.3/2/4，320×240 与 451×287；另含 1920×1080 的 zoom=4；
resolution=1/2；沼泽中心非整数相机及四角请求。实际相机经 clamp 后反推记录，不把请求值当实际锚点。
拦截 TexturePool.getOptimalTexture 记录实际返回纹理尺寸，验证视口界与设备界；每次 RT 在 finally 释放。
三次真实房间→纯空地→真实房间，记录旧滤镜释放、两 mask 重建及 lighting=real。

| 后端 / resolution / zoom | 差异通道数 / 比较通道总数 | 最大 8-bit 差值 |
| --- | --- | --- |
| WebGPU / 1 / .13 | 2 / 307200 | 1 |
| WebGPU / 1 / .2 | 12 / 307200 | 1 |
| WebGPU / 2 / .13 | 3 / 1228800 | 1 |
| WebGPU / 2 / .2 | 28 / 1228800 | 1 |
| WebGL / 1 / .13 | 2 / 307200 | 1 |
| WebGL / 1 / .2 | 15 / 307200 | 1 |
| WebGL / 2 / .13 | 3 / 1228800 | 1 |
| WebGL / 2 / .2 | 22 / 1228800 | 1 |

低倍率对照保留全部差值，不宣称逐像素零差。隐藏噪声正对照有数千至数万通道变化，证明噪声参与画面。
两后端全部 70 个案例都满足申请尺寸界、非黑像素>0，deviceLost=false；最大返回纹理边长 3840。
非黑像素只证明没有整帧黑屏；沼泽视觉由低倍率原始 RGBA 与主画布高倍率沼泽截图共同支持。
这些 pool 尺寸不等于全部 GPU 分配或实际 VRAM。每次回归约数秒，不测 CPU/GPU 帧预算，不跑 soak。
原始 [WebGPU](renderer-zoom-data-2026-10-01/webgpu.json) / [WebGL](renderer-zoom-data-2026-10-01/webgl.json)
含全部 case、正对照、实际相机、纹理尺寸、设备限额与浏览器环境。
最终新页面逐后端独立运行耗时 WebGPU 2208.5ms / WebGL 1264.9ms，这是整段回归墙钟时长，
不是帧耗时。源码/夹具 SHA256 与硬件/方法摘要见[环境清单](renderer-zoom-data-2026-10-01/environment.json)。

1280×720 主画布人工核验：原复现 zoom=2 恢复；双后端 zoom=4 正常；非整数相机 zoom=1.3 与沼泽 zoom=4 正常。
原复现页面实际滚轮继续放大到400%，仍正常绘制且该新页面warn/error日志为空，见[滚轮400%](renderer-zoom-data-2026-10-01/after-wheel-zoom4.jpg)。
整房 zoom=.13 修复前后 JPEG 截图避开 HUD 的 y≥180 区域差值 bbox=null；这仅是该区域的截图检查。
见 [修复后200%](renderer-zoom-data-2026-10-01/after-webgpu-zoom2.jpg)、
[WebGL400%](renderer-zoom-data-2026-10-01/after-webgl-zoom4-dpr2.jpg)、
[WebGPU400%沼泽](renderer-zoom-data-2026-10-01/after-webgpu-swamp-zoom4.jpg)、
[整房修复前](renderer-zoom-data-2026-10-01/before-webgpu-fit.jpg) / [整房修复后](renderer-zoom-data-2026-10-01/after-webgpu-fit.jpg)。

## 既有差异与验证边界

官方 v7 SpriteMaskFilter 使用 mask.worldAlpha=.25；我方 v8 旧自动路径与本次自有路径都使用初始化 uAlpha=1，
并未读取 worldAlpha。此修复保持我方 v8 既有 shader 语义，不能据此宣称官方 alpha/像素完全一致。
这是新增取证备注，不变更用户已确认的 G1–G13 终裁，也没有在本轮擅自调整绿色强度。

临时 RT/地形释放仍可能触发已登记的 Pixi 批次 BindGroup 缓存警告，未屏蔽；
完整日志见 [WebGPU日志](renderer-zoom-data-2026-10-01/webgpu-logs.json) / [WebGL日志](renderer-zoom-data-2026-10-01/webgl-logs.json)。
最终新页面日志错误均为0、TexturePool超限警告均为0；既有BindGroup警告分别44/10条。
Pixi 的警告限流机制没有改动，不能凭警告消失单独证明安全；核心证据是有界实际申请、GPU 完成、设备状态与画布。
R7 长测/内存缓存增长挂账仍见 [短测报告](renderer-r7-short-verification-2026-10-01.md)。

自验：renderer 370（原369零回归）+ shared 4 + kk-cli 7；tsc、fmt/clippy、renderer ESM+d.ts
公共闭包10文件零Pixi；shared src-only JS+d.ts 构建。资产/所有夹具生成链重生 175 文件 SHA 全一致。
三路实际评审及复审通过：lifecycle_review（实现与GPU方法学）、memory_review（资源所有权与单测）、
replay_review（原版语义、文档与数据）；已清偿共享纹理假测试、mask数量/尺寸断言、噪声正对照、实际相机记录。
