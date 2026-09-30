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
import { DEFAULT_TICK_DURATION_MS } from './objects/specs.js';
import { AnimationClock } from './anim/clock.js';
import type { EffectsStats } from './anim/effects-stage.js';
import type { AmbientStats } from './anim/ambient-scheduler.js';
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
   * （无资产管线也能显示几何——对象层缺帧降级隐藏，地形照常烘焙）。
   */
  atlasBaseUrl?: string;
  /** 渲染后端偏好，默认 webgpu（不可用时 pixi 自动回退 webgl）。 */
  preference?: 'webgpu' | 'webgl';
  /**
   * 本玩家 id（userOwnerColor 色板的 own 判定——G9：契约 v0 无 users 映射，
   * 玩家色走官方 demo 同款 userOwnerColor 模式）。缺省 null=无本玩家视角
   * （全敌对色）。**非最终契约面，R6 API 冻结时评审去留。**
   */
  localPlayerId?: string;
  /**
   * tick 时长 ms（运动插值/受击罩/死亡淡出时序；G6 契约参数化，缺省 1000，
   * 官方 demo 慢放同参）。**非最终契约面，R6 API 冻结时评审去留。**
   */
  tickDurationMs?: number;
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
  effects: EffectsStats;
  ambient: AmbientStats;
  lightingPass: { invalidations: number; renders: number; members: number; privateSources: number };
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
  /** 相机状态直设（世界锚点+缩放；纯数据入参）。回放器/截图工具用——
   * 非最终契约面，R6 API 冻结时评审去留。 */
  setCameraState(state: { x: number; y: number; zoom: number }): void;
  /** 场景动画回放倍率，默认 1；0 暂停。调用方同步调整 RoomFeed 推送速率。
   * 保留当前动画进度。非最终契约面，R6 冻结时评审。 */
  setPlaybackRate(rate: number): void;
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
  const clock = new AnimationClock();
  let atlas: LoadedAtlas | null = null;
  let destroyScene: (() => void) | undefined;
  let disconnect: (() => void) | undefined;
  let disposal: Promise<void> | undefined;
  const dispose = (): Promise<void> => disposal ??= (async () => {
    disconnect?.();
    camera.detach();
    try {
      destroyScene?.();
    } finally {
      try { await kernel.destroy(); } finally { await atlas?.dispose(); }
    }
  })();

  try {
    camera.attach(kernel.app.canvas);
    if (options.atlasBaseUrl) {
      atlas = await loadAtlas(options.atlasBaseUrl, kernel.app.renderer.resolution);
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

    // 对象舞台资产（R3）：atlas 帧 + glow/creep-mask 独立纹理；无 atlas 时对象
    // 层缺帧降级。
    const objectsSetup = atlas
      ? {
          assets: {
            frames: new Map([...atlas.frames, ...[...atlas.standalone].map(([key, value]) => [key, value.texture] as const)]),
            glow: atlas.standalone.get('glow')?.texture ?? null,
            creepMask: atlas.standalone.get('creep-mask')?.texture ?? null,
          },
          player: options.localPlayerId ?? null,
          tickDurationMS: options.tickDurationMs ?? DEFAULT_TICK_DURATION_MS,
        }
      : undefined;

    const room = new RoomScene(textures, cellSize, objectsSetup);
    destroyScene = () => room.destroy();
    kernel.stage.addChild(room.root);

    const feed = new FeedStore();
    // feed→scene 接线：快照携带地形即重建（hash 门控在 TerrainStage 内）；
    // 对象列表逐变更对账（constructedWall 格集变化在 applyObjects 内返回并
    // 触发墙重烘焙）；事件面随变更记录下发（R4 攻击/治疗罩，R5 扩全表）。
    const unsubscribe = feed.onChange((roomKey, change) => {
      const terrain = feed.terrain(roomKey);
      if (terrain) room.setTerrain(terrain.codes);
      room.applyObjects(change.objects, change.time, change.events, change.kind, change);
    });

    const onResize = () => camera.syncViewport();
    const onTick = (ticker: { elapsedMS: number }): void => {
      camera.syncViewport();
      // deltaMS 有 minFPS 截断且受 speed 缩放；动作必须按实际经过时间推进。
      const { width, height } = viewSize();
      const cam = camera.state;
      room.update(clock.advance(ticker.elapsedMS), kernel.app.renderer, {
        minX: cam.x - width / cam.zoom / 2, minY: cam.y - height / cam.zoom / 2,
        maxX: cam.x + width / cam.zoom / 2, maxY: cam.y + height / cam.zoom / 2,
      });
    };
    kernel.app.renderer.on('resize', onResize);
    kernel.app.ticker.add(onTick);
    disconnect = () => {
      unsubscribe();
      kernel.app.ticker.remove(onTick);
      kernel.app.renderer.off('resize', onResize);
    };

    camera.setBounds(floorRect);

    const fitRoom = () => {
      const { width, height } = viewSize();
      camera.setState(fitRect(floorRect.minX, floorRect.minY, view, view, width, height));
    };
    fitRoom();

    const handle: RendererHandle = {
      getStats(): RendererStats {
        return {
          backend: kernel.backend,
          fps: Math.round(kernel.app.ticker.FPS),
          zoom: camera.zoom,
          atlasFrames: atlas?.frames.size ?? 0,
          lighting: room.lighting.isDegraded ? 'degraded' : 'real',
          effects: room.objects?.effects.stats ?? { active: 0, nodes: 0, privateTexts: 0, skipped: 0 },
          ambient: room.objects?.ambient.stats ?? { active: 0, awake: 0, sleeping: 0 },
          lightingPass: room.lighting.stats,
        };
      },
      fitRoom,
      setCameraState(state: { x: number; y: number; zoom: number }): void {
        camera.setState(state);
      },
      setPlaybackRate(rate: number): void {
        clock.setRate(rate);
      },
      feed,
      pickAt(screenX: number, screenY: number): PickResult {
        camera.syncViewport();
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
        (handle as unknown as { __room?: RoomScene }).__room = undefined;
        await dispose();
      },
    };
    // dev/harness 诊断附加字段（B-S2 风险缓释：不可枚举防 spread 拖走场景树、
    // destroy 置空防悬挂引用；活 pixi 树进运行时公共面的取舍 R6 冻结时裁定）
    Object.defineProperty(handle, '__room', {
      value: room,
      enumerable: false,
      writable: true,
      configurable: false,
    });
    return handle;
  } catch (error) {
    // 所有初始化阶段共用销毁路径，回滚已创建的节点、监听和资产。
    try { await dispose(); } catch { /* 保留原始初始化错误。 */ }
    throw error;
  }
}
