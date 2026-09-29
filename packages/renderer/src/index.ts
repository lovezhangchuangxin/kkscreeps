/**
 * @kkscreeps/renderer 公共 API。
 *
 * 原则（ADR-009 / 方案 §0）：框架无关、数据驱动、Pixi 对象绝不进公共面——
 * 输入输出全是普通数据；渲染循环归本包所有。R2 为骨架面（相机+地形+层栈），
 * 公共面 R6 冻结。
 */
import { Texture } from 'pixi.js';
import { CameraController } from './core/camera.js';
import { initKernel, type Backend, type Kernel } from './core/renderer.js';
import { fitRect, screenToWorld, type CameraState } from './core/camera-math.js';
import { loadAtlas, type LoadedAtlas } from './assets/atlas.js';
import { FeedStore, type FeedView } from './feed/room-feed.js';
import { DEFAULT_CELL_SIZE, ROOM_SIZE, RoomScene } from './scenes/room.js';
import type { TerrainTextures } from './scenes/terrain.js';

export type { Backend } from './core/renderer.js';
export type { CameraState } from './core/camera-math.js';
export { fitRect, screenToWorld } from './core/camera-math.js';
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
   * atlas 基目录 URL（assets/，尾随 /）。缺省时以白色占位纹理建场景
   * （无资产管线也能显示几何——R2 起地形烘焙需真纹理，harness 恒传）。
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
  /** lighting 真离屏 pass 状态（G2：degraded=无全局压暗降级）。 */
  lighting: 'real' | 'degraded';
}

/** 拾取结果（R2 接口：几何换算；对象命中随 R3 描述符落位）。 */
export interface PickResult {
  /** 世界像素坐标。 */
  world: { x: number; y: number };
  /** 格坐标（0..49；出房为 null）。 */
  cell: { x: number; y: number } | null;
}

export interface RendererHandle {
  getStats(): RendererStats;
  /** 将相机适配到房间可视矩形（floor：自 -cell/2 起的 view² ）。 */
  fitRoom(): void;
  /** RoomFeed 输入口（契约 @kkscreeps/shared/feed + 只读摘要；R1 起回放/网络共用）。 */
  readonly feed: FeedView;
  /** 拾取接口（屏幕坐标 → 世界/格坐标；对象命中 R3）。
   * 格相位：格中心 (x*cell, y*cell)（官方对象锚点），范围 ±cell/2。 */
  pickAt(screenX: number, screenY: number): PickResult;
  destroy(): Promise<void>;
}

/** 无资产管线的占位纹理集（全白——几何可见，无细节；无烘焙位图）。 */
function placeholderTextures(): TerrainTextures {
  const white = (): { texture: Texture } => ({ texture: Texture.WHITE });
  return {
    ground: white(),
    'ground-mask': white(),
    noise1: white(),
    noise2: white(),
    'exit-top': white(),
    'exit-bottom': white(),
    'exit-left': white(),
    'exit-right': white(),
  };
}

export async function createRenderer(options: CreateRendererOptions): Promise<RendererHandle> {
  const cellSize = options.cellSize ?? DEFAULT_CELL_SIZE;
  const kernel: Kernel = await initKernel({
    container: options.container,
    backgroundColor: options.backgroundColor ?? 0x050505,
    preference: options.preference ?? 'webgpu',
  });

  // 视口尺寸取 CSS 逻辑像素（v8 renderer.screen；renderer.width 同为逻辑像素，
  // 但 screen 语义显式）。
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

  const view = ROOM_SIZE * cellSize;
  const half = cellSize / 2;
  // 官方几何：floor 自 -cell/2 起（半格边距带为出口条）
  const floorRect = { minX: -half, minY: -half, maxX: -half + view, maxY: -half + view };

  const textures = atlas
    ? ({
        ground: atlas.standalone.get('ground')!,
        'ground-mask': atlas.standalone.get('ground-mask')!,
        noise1: atlas.standalone.get('noise1')!,
        noise2: atlas.standalone.get('noise2')!,
        'exit-top': atlas.standalone.get('exit-top')!,
        'exit-bottom': atlas.standalone.get('exit-bottom')!,
        'exit-left': atlas.standalone.get('exit-left')!,
        'exit-right': atlas.standalone.get('exit-right')!,
      } satisfies TerrainTextures)
    : placeholderTextures();

  const room = new RoomScene(textures, cellSize);
  kernel.stage.addChild(room.root);

  const feed = new FeedStore();
  // feed→scene 接线：快照携带地形即重建（hash 门控在 TerrainStage 内）。
  feed.onChange((roomKey) => {
    const terrain = feed.terrain(roomKey);
    if (terrain) room.setTerrain(terrain.codes);
  });

  const onTick = (ticker: { deltaMS: number }): void => {
    room.update(ticker.deltaMS / 16.6667, kernel.app.renderer);
  };
  kernel.app.ticker.add(onTick);

  camera.setBounds(floorRect);

  const fitRoom = () => {
    const { width, height } = viewSize();
    camera.setState(fitRect(floorRect.minX, floorRect.minY, view, view, width, height));
  };
  fitRoom();

  return {
    getStats(): RendererStats {
      return {
        backend: kernel.backend,
        fps: Math.round(kernel.app.ticker.FPS),
        zoom: camera.zoom,
        atlasFrames: atlas?.frames.size ?? 0,
        lighting: room.lighting.isDegraded ? 'degraded' : 'real',
      };
    },
    fitRoom,
    feed,
    pickAt(screenX: number, screenY: number): PickResult {
      const { width, height } = viewSize();
      const cam: CameraState = camera.state;
      const world = screenToWorld(cam, width, height, screenX, screenY);
      // 格中心 (x*cell, y*cell)（评审 A3：±cell/2 范围归本格，非 [x*cell,(x+1)*cell)）
      const cx = Math.round(world.x / cellSize);
      const cy = Math.round(world.y / cellSize);
      const inside = cx >= 0 && cy >= 0 && cx < ROOM_SIZE && cy < ROOM_SIZE;
      return { world, cell: inside ? { x: cx, y: cy } : null };
    },
    async destroy() {
      kernel.app.ticker.remove(onTick);
      camera.detach();
      room.destroy();
      await kernel.destroy();
    },
  };
}
