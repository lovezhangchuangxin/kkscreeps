/**
 * @kkscreeps/renderer 公共 API。
 *
 * 原则（ADR-009 / 方案 §0）：框架无关、数据驱动、Pixi 对象绝不进公共面——
 * 输入输出全是普通数据；渲染循环归本包所有。R0 为骨架面（相机+底色房），
 * RoomFeed 数据接口 R1 落位，公共面 R6 冻结。
 */
import { CameraController } from './core/camera.js';
import { initKernel, type Backend, type Kernel } from './core/renderer.js';
import { fitRect } from './core/camera-math.js';
import { loadAtlas, type LoadedAtlas } from './assets/atlas.js';
import { FeedStore, type FeedView } from './feed/room-feed.js';
import {
  DEFAULT_CELL_SIZE,
  ROOM_SIZE,
  RoomScene,
} from './scenes/room.js';

export type { Backend } from './core/renderer.js';
export type { CameraState } from './core/camera-math.js';
export { fitRect } from './core/camera-math.js';
export { DEFAULT_CELL_SIZE, ROOM_SIZE } from './scenes/room.js';
export type { FeedView, RoomSummary } from './feed/room-feed.js';

export interface CreateRendererOptions {
  /** 渲染画布挂载容器（尺寸即视口尺寸）。 */
  container: HTMLElement;
  /** 世界像素/格，默认 100（官方 demo 取值）。 */
  cellSize?: number;
  /** 房间外背景色，默认 0x050505（官方 demo 取值）。 */
  backgroundColor?: number;
  /**
   * atlas 基目录 URL（assets/，尾随 /）。缺省跳过资产加载
   * （纯底色房可用）；harness 传入以验证资产管线端到端。
   */
  atlasBaseUrl?: string;
  /** 渲染后端偏好，默认 webgpu（不可用时 pixi 自动回退 webgl）。 */
  preference?: 'webgpu' | 'webgl';
}

export interface RendererStats {
  backend: Backend;
  /** 实测帧率（ticker 平滑值）。 */
  fps: number;
  zoom: number;
  /** 已加载 atlas 帧数（未加载 atlas 时为 0）。 */
  atlasFrames: number;
}

export interface RendererHandle {
  getStats(): RendererStats;
  /** 将相机适配到整个房间（留边）。 */
  fitRoom(): void;
  /** RoomFeed 输入口（契约 @kkscreeps/shared/feed + 只读摘要；R1 起回放/网络共用）。 */
  readonly feed: FeedView;
  destroy(): Promise<void>;
}

export async function createRenderer(options: CreateRendererOptions): Promise<RendererHandle> {
  const cellSize = options.cellSize ?? DEFAULT_CELL_SIZE;
  const kernel: Kernel = await initKernel({
    container: options.container,
    backgroundColor: options.backgroundColor ?? 0x050505,
    preference: options.preference ?? 'webgpu',
  });

  const room = new RoomScene(cellSize);
  kernel.stage.addChild(room.root);

  // 视口尺寸取 CSS 逻辑像素（v8 renderer.screen；renderer.width 同为逻辑像素，
  // 但 screen 语义显式、不依赖对 resolution 的记忆）。
  const viewSize = () => ({
    width: kernel.app.renderer.screen.width,
    height: kernel.app.renderer.screen.height,
  });
  const camera = new CameraController(kernel.stage, viewSize);
  camera.attach(kernel.app.canvas);

  let atlas: LoadedAtlas | null = null;
  try {
    if (options.atlasBaseUrl) {
      atlas = await loadAtlas(options.atlasBaseUrl, kernel.app.renderer.resolution);
    }
  } catch (e) {
    // 资产加载失败：内核已起（canvas 入 DOM、ticker/监听存活），先清理再抛。
    await kernel.destroy();
    throw e;
  }

  const fitRoom = () => {
    const extent = ROOM_SIZE * cellSize;
    const { width, height } = viewSize();
    camera.setState(fitRect(0, 0, extent, extent, width, height));
  };
  fitRoom();

  const feed = new FeedStore();

  return {
    getStats(): RendererStats {
      return {
        backend: kernel.backend,
        fps: Math.round(kernel.app.ticker.FPS),
        zoom: camera.zoom,
        atlasFrames: atlas?.frames.size ?? 0,
      };
    },
    fitRoom,
    feed,
    async destroy() {
      camera.detach();
      await kernel.destroy();
    },
  };
}
