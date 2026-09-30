/**
 * @kkscreeps/renderer 公共 API（R6 冻结面）。
 *
 * 原则（ADR-009 / 方案 §0）：框架无关、数据驱动、Pixi 对象绝不进公共面——
 * 输入输出全是普通数据；渲染循环归本包所有。
 *
 * 运行时语义（契约；R5 移交项随冻结文档化，行为详情见各模块头注）：
 *   - 受击罩 owner=来源对象：来源中途删除即取消罩（R5 登记的取消语义）；
 *   - say 隐私过滤：无本玩家身份（localPlayerId 缺省）时私有气泡全滤；
 *   - visuals 全量替换：setVisuals/快照后 visuals 为该房全量（官方每 tick
 *     全量下发语义；applySnapshot 清空旧 visuals）；
 *   - 活动房间：非活动房变更零渲染写入，**离房期间的事件不补播**（切回只
 *     见当前状态）；切房以快照语义重建（跨房跳变不补间，防幻影移动）；
 *   - 快照清除瞬态并即时同步位置（同 id/type 复用节点）；填充量白名单
 *     （link 能量与 spawn/powerSpawn 资源圆等）按 tick 线性补间后钳制。
 */
import { Texture } from 'pixi.js';
import { CameraController } from './core/camera.js';
import { initKernel, type Backend, type Kernel } from './core/renderer.js';
import { fitRect, screenToWorld, type CameraState } from './core/camera-math.js';
import { loadAtlas, type LoadedAtlas } from './assets/atlas.js';
import { ensureFonts, type FontStatus } from './assets/fonts.js';
import { ActiveRoomController } from './feed/active-room.js';
import { FeedStore, type FeedView } from './feed/room-feed.js';
import { resolvePickObject } from './feed/pick.js';
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
export type { FontStatus } from './assets/fonts.js';

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
   * （全敌对色；此时私有 say 全滤）。
   */
  localPlayerId?: string;
  /**
   * tick 时长 ms（运动插值/受击罩/死亡淡出等动画时序的基准；G6，缺省 1000）。
   * **只是动画时序参数，不等于也不驱动数据推送节奏**——逻辑 tick 由
   * RoomFeed 推送方定义。
   */
  tickDurationMs?: number;
  /**
   * Roboto 字体基目录 URL（尾随 /，寻址 `fonts/roboto-latin-<weight>-normal.woff2`；
   * G7：字体加载责任在库——自建 FontFace 预载三规格 55/60/82bold，失败回退
   * 字体渲染并可经 getStats().fonts 验证）。缺省不加载（getStats().fonts='unset'）。
   */
  fontsBaseUrl?: string;
  /**
   * 每帧阶段计时回调（普通数据诊断面；R5 `?smoke=1` 方法学同源——objects
   * 段只含对象舞台更新，lighting 段=离屏 pass；不含 GPU 提交等待）。仅诊断
   * 用，缺省不产生任何每帧开销之外的工作。R7 perf 模式复用此面。
   */
  onFrameTiming?: (timing: { objectsMs: number; lightingMs: number }) => void;
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
  /** 当前房间已渲染 RoomVisual 条数（无效条目不计）。 */
  visuals: number;
  /** Roboto 预载状态（G7：unset=未配置 / loaded=三规格就绪 / fallback=回退）。 */
  fonts: FontStatus;
  effects: EffectsStats;
  ambient: AmbientStats;
  lightingPass: { invalidations: number; renders: number; members: number; privateSources: number };
}

/** 拾取结果（几何 + 数据面命中）。 */
export interface PickResult {
  /** 世界像素坐标。 */
  world: { x: number; y: number };
  /** 格坐标（0..49；出房为 null）。 */
  cell: { x: number; y: number } | null;
  /** 命中对象 id（该格 z 序最高者，平手取 id 码点序；无对象/出房为
   * undefined）。按对象**数据格**命中——移动中 creep 以其目标格计，非插值
   * 渲染位。 */
  objectId?: string;
}

/**
 * 渲染器句柄（R6 冻结面）。全部方法普通数据出入，无 Pixi 类型（ADR-009）。
 */
export interface RendererHandle {
  getStats(): RendererStats;
  /** 将相机适配到房间可视矩形（floor：自 -cell/2 起的 view² ）。 */
  fitRoom(): void;
  /** 相机状态直设（世界锚点+缩放；纯数据入参）。回放器/截图工具用。 */
  setCameraState(state: { x: number; y: number; zoom: number }): void;
  /** 场景动画回放倍率，默认 1；0 暂停。只改变动画经过时间（保留当前进度），
   * **不驱动数据推送**——调用方自行同步 RoomFeed 推送速率。 */
  setPlaybackRate(rate: number): void;
  /** 手动推进场景动画 ms（与 ticker 同一场景更新路径，可叠加）：回放 seek、
   * 定帧截图与黄金帧回归用。注入动画时间本身，**不受 setPlaybackRate 影响**
   * （暂停态下仍可推进）；负数/非有限值按 0 处理。0 无操作。 */
  advanceAnimation(ms: number): void;
  /** 当前活动房间；首个 applySnapshot 自动激活，此后经 setActiveRoom 切换。 */
  readonly activeRoom: string | null;
  /**
   * 切换活动房间（该房须已有 feed 数据，未知房抛错——fail-fast 同 feed 纪律）。
   * 非活动房的快照/增量/setVisuals 只进数据仓库，零渲染写入；切房=该房当前
   * 仓库状态快照语义重建；**离房期间的事件不补播**（见文件头运行时语义）。
   */
  setActiveRoom(room: string): void;
  /** RoomFeed 输入口（契约 @kkscreeps/shared/feed + 只读摘要；回放/网络共用）。 */
  readonly feed: FeedView;
  /** 拾取接口（屏幕坐标 → 世界/格/对象；几何与命中语义见 PickResult）。 */
  pickAt(screenX: number, screenY: number): PickResult;
  /**
   * 点击拾取回调（多订阅；返回退订函数）。主键点击且位移 ≤4px（非拖拽）时
   * 触发；点在房间外时 cell=null/objectId 缺省仍回调（消费方自行过滤）。
   */
  onPick(listener: (result: PickResult) => void): () => void;
  /**
   * 悬停回调（多订阅；返回退订函数）。悬停格或命中对象变化时触发；离开
   * 画布/房间外回调 null；拖拽中抑制。
   */
  onHover(listener: (result: PickResult | null) => void): () => void;
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

/** 点击判定位移阈值 px（spec 常量——超过视为拖拽不触发 onPick）。 */
const PICK_SLOP_PX = 4;

export async function createRenderer(options: CreateRendererOptions): Promise<RendererHandle> {
  const cellSize = options.cellSize ?? DEFAULT_CELL_SIZE;
  // 字体与内核初始化并行；两者都就绪才建场景（字体失败不阻塞——G7）
  const fontsPending = options.fontsBaseUrl !== undefined ? ensureFonts(options.fontsBaseUrl) : null;
  const kernel: Kernel = await initKernel({
    container: options.container,
    backgroundColor: options.backgroundColor ?? 0x050505,
    preference: options.preference ?? 'webgpu',
  });
  let fontStatus: FontStatus = 'unset';
  if (fontsPending) fontStatus = await fontsPending;

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
    // 活动房间编排（R6 D3）：非活动房变更只进 FeedStore（零渲染写入）；切房
    // 快照语义重建。visuals 同引用短路——增量通知零重建成本。
    const activeRoomCtrl = new ActiveRoomController(
      {
        apply: (name, change) => {
          const terrain = feed.terrain(name);
          if (terrain) room.setTerrain(terrain.codes);
          room.applyObjects(change.objects, change.time, change.events, change.kind, change);
          room.setVisuals(change.visuals);
        },
        rebuild: (name) => {
          const terrain = feed.terrain(name);
          if (terrain) room.setTerrain(terrain.codes);
          room.applyObjects(feed.objects(name), feed.summary(name)?.time ?? 0, [], 'snapshot');
          room.setVisuals(feed.visuals(name));
        },
      },
      (name) => feed.summary(name) !== undefined,
    );
    const unsubscribe = feed.onChange((name, change) => activeRoomCtrl.onChange(name, change));

    // ---- 拾取/悬停（R6 D4）：几何换算 + 数据面命中；DOM 输入与相机拖拽互不干扰 ----
    const pickListeners = new Set<(result: PickResult) => void>();
    const hoverListeners = new Set<(result: PickResult | null) => void>();

    const pickFromScreen = (screenX: number, screenY: number): PickResult => {
      camera.syncViewport();
      const { width, height } = viewSize();
      const cam: CameraState = camera.state;
      const world = screenToWorld(cam, width, height, screenX, screenY);
      // 格中心 (x*cell, y*cell)（评审 A3：±cell/2 范围归本格，非 [x*cell,(x+1)*cell)）
      const cx = Math.round(world.x / cellSize);
      const cy = Math.round(world.y / cellSize);
      const inside = cx >= 0 && cy >= 0 && cx < ROOM_SIZE && cy < ROOM_SIZE;
      const cell = inside ? { x: cx, y: cy } : null;
      const current = activeRoomCtrl.activeRoom;
      const objectId = cell !== null && current !== null
        ? resolvePickObject(cell, feed.objects(current))
        : undefined;
      return { world, cell, objectId };
    };

    const canvasEl = kernel.app.canvas;
    let pressedClient: { x: number; y: number } | null = null;
    let lastHover: PickResult | null = null;
    const fireHover = (result: PickResult | null): void => {
      const changed = (result === null) !== (lastHover === null) ||
        (result !== null && lastHover !== null && (
          result.cell?.x !== lastHover.cell?.x ||
          result.cell?.y !== lastHover.cell?.y ||
          result.objectId !== lastHover.objectId
        ));
      lastHover = result;
      if (!changed) return;
      room.setHighlight(result?.cell ?? null);
      for (const listener of hoverListeners) listener(result);
    };
    const onPointerDown = (e: PointerEvent): void => {
      if (e.button !== 0) return;
      pressedClient = { x: e.clientX, y: e.clientY };
    };
    const onPointerMove = (e: PointerEvent): void => {
      if (pressedClient !== null) return; // 拖拽中抑制悬停
      const rect = canvasEl.getBoundingClientRect();
      const result = pickFromScreen(e.clientX - rect.left, e.clientY - rect.top);
      fireHover(result.cell === null ? null : result);
    };
    const onPointerUp = (e: PointerEvent): void => {
      if (e.button !== 0) return; // 非主键释放不改按压态、不触发拾取
      const start = pressedClient;
      pressedClient = null;
      if (start === null) return;
      if (Math.hypot(e.clientX - start.x, e.clientY - start.y) > PICK_SLOP_PX) return;
      const rect = canvasEl.getBoundingClientRect();
      const result = pickFromScreen(e.clientX - rect.left, e.clientY - rect.top);
      for (const listener of pickListeners) listener(result);
    };
    const onPointerCancel = (): void => {
      pressedClient = null;
    };
    const onPointerLeave = (): void => {
      pressedClient = null;
      fireHover(null);
    };

    // ---- 帧循环：唯一时间入口（AnimationClock）；计时诊断普通数据面 ----
    // scaled=false 为手动定帧推进（advanceAnimation）——注入的是动画时间
    // 本身，不经倍率（暂停态下 seek/截图/黄金帧仍可用；语义见 TSDoc）。
    const advanceFrame = (elapsedMS: number, scaled = true): void => {
      camera.syncViewport();
      // deltaMS 有 minFPS 截断且受 speed 缩放；动作必须按实际经过时间推进。
      const { width, height } = viewSize();
      const cam = camera.state;
      room.update(scaled ? clock.advance(elapsedMS) : elapsedMS, kernel.app.renderer, {
        minX: cam.x - width / cam.zoom / 2, minY: cam.y - height / cam.zoom / 2,
        maxX: cam.x + width / cam.zoom / 2, maxY: cam.y + height / cam.zoom / 2,
      });
      options.onFrameTiming?.({
        objectsMs: room.lastStageMs.objectsMs,
        lightingMs: room.lastStageMs.lightingMs,
      });
    };
    const onTick = (ticker: { elapsedMS: number }): void => advanceFrame(ticker.elapsedMS);

    const onResize = () => camera.syncViewport();
    kernel.app.renderer.on('resize', onResize);
    kernel.app.ticker.add(onTick);
    canvasEl.addEventListener('pointerdown', onPointerDown);
    canvasEl.addEventListener('pointermove', onPointerMove);
    canvasEl.addEventListener('pointerup', onPointerUp);
    canvasEl.addEventListener('pointercancel', onPointerCancel);
    canvasEl.addEventListener('pointerleave', onPointerLeave);
    disconnect = () => {
      unsubscribe();
      kernel.app.ticker.remove(onTick);
      kernel.app.renderer.off('resize', onResize);
      canvasEl.removeEventListener('pointerdown', onPointerDown);
      canvasEl.removeEventListener('pointermove', onPointerMove);
      canvasEl.removeEventListener('pointerup', onPointerUp);
      canvasEl.removeEventListener('pointercancel', onPointerCancel);
      canvasEl.removeEventListener('pointerleave', onPointerLeave);
      pickListeners.clear();
      hoverListeners.clear();
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
          visuals: room.visuals.count,
          fonts: fontStatus,
          effects: room.objects?.effects.stats ?? { active: 0, nodes: 0, texts: 0, skipped: 0 },
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
      advanceAnimation(ms: number): void {
        // 负数/非有限值按 0 处理（诊断面宽松入参，不抛错打断截图脚本）
        advanceFrame(typeof ms === 'number' && Number.isFinite(ms) && ms > 0 ? ms : 0, false);
      },
      get activeRoom(): string | null {
        return activeRoomCtrl.activeRoom;
      },
      setActiveRoom(name: string): void {
        activeRoomCtrl.setActive(name);
      },
      feed,
      pickAt(screenX: number, screenY: number): PickResult {
        return pickFromScreen(screenX, screenY);
      },
      onPick(listener: (result: PickResult) => void): () => void {
        pickListeners.add(listener);
        return () => pickListeners.delete(listener);
      },
      onHover(listener: (result: PickResult | null) => void): () => void {
        hoverListeners.add(listener);
        return () => hoverListeners.delete(listener);
      },
      async destroy() {
        await dispose();
      },
    };
    return handle;
  } catch (error) {
    // 所有初始化阶段共用销毁路径，回滚已创建的节点、监听和资产。
    try { await dispose(); } catch { /* 保留原始初始化错误。 */ }
    throw error;
  }
}
