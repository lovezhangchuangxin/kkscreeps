import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Container, Ticker } from 'pixi.js';
import type { RoomVisual } from '@kkscreeps/shared';
import { createRenderer, type RendererHandle } from '../src/index.js';
import { RoomScene } from '../src/scenes/room.js';
import { initKernel, type Kernel } from '../src/core/renderer.js';
import { mockAssetIO } from './asset-mocks.js';

vi.mock('../src/core/renderer.js', () => ({ initKernel: vi.fn() }));

function makeKernel() {
  const stage = new Container();
  const ticker = new Ticker();
  const listeners = new Set<() => void>();
  const renderer = {
    screen: { width: 800, height: 800 }, resolution: 1, render: vi.fn(),
    on: vi.fn((_event: string, listener: () => void) => { listeners.add(listener); }),
    off: vi.fn((_event: string, listener: () => void) => { listeners.delete(listener); }),
  };
  const domListeners = new Map<string, Set<(e: Partial<PointerEvent>) => void>>();
  const canvas = {
    style: {},
    getBoundingClientRect: vi.fn(() => ({ left: 0, top: 0 })),
    setPointerCapture: vi.fn(),
    releasePointerCapture: vi.fn(),
    hasPointerCapture: vi.fn(() => false),
    addEventListener: vi.fn((event: string, listener: (e: Partial<PointerEvent>) => void) => {
      let set = domListeners.get(event);
      if (!set) { set = new Set(); domListeners.set(event, set); }
      set.add(listener);
    }),
    removeEventListener: vi.fn((event: string, listener: (e: Partial<PointerEvent>) => void) => {
      domListeners.get(event)?.delete(listener);
    }),
  };
  /** 合成派发 DOM 指针事件（拾取/悬停测试）。 */
  const firePointer = (event: string, e: Partial<PointerEvent>): void => {
    for (const listener of [...(domListeners.get(event) ?? [])]) listener(e);
  };
  const destroy = vi.fn(async () => { ticker.destroy(); stage.destroy({ children: true }); });
  const kernel: Kernel = { stage, backend: 'webgl', destroy,
    app: { stage, ticker, renderer, canvas } as unknown as Kernel['app'] };
  return { kernel, ticker, renderer, canvas, listeners, destroy, stage, domListeners, firePointer };
}

const handles: RendererHandle[] = [];
let runtime: ReturnType<typeof makeKernel>;
const creep = (x: number) => ({ id: 'c', type: 'creep', x, y: 10, user: 'p1', body: [{ type: 'work', hits: 100 }] });
async function mounted(): Promise<RendererHandle> {
  const handle = await createRenderer({ container: {} as HTMLElement, atlasBaseUrl: 'https://renderer.invalid/' });
  handles.push(handle);
  return handle;
}
beforeEach(() => {
  runtime = makeKernel();
  vi.mocked(initKernel).mockResolvedValue(runtime.kernel);
});
afterEach(async () => {
  for (const handle of handles.splice(0)) await handle.destroy();
  vi.restoreAllMocks(); vi.unstubAllGlobals();
});

describe('公共句柄生命周期与真实 ticker 接线', () => {
  it('长帧使用 elapsedMS，500ms 后 1s 移动到半程', async () => {
    mockAssetIO();
    const handle = await mounted();
    handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [creep(10)] });
    handle.feed.applyDelta({ room: 'W0N0', time: 1, upsert: [creep(11)], removedIds: [] });
    runtime.ticker.lastTime = 0;
    runtime.ticker.update(500);
    expect(runtime.ticker.deltaMS).toBeLessThan(500);
    expect(runtime.stage.getChildByLabel('obj-c', true)!.x).toBeCloseTo(1050);
    runtime.ticker.update(1500);
    expect(runtime.stage.getChildByLabel('obj-c', true)!.x).toBe(1100);
  });

  it('倍率变化保留已完成进度，0 暂停，恢复后继续', async () => {
    mockAssetIO();
    const handle = await mounted();
    handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [creep(10)] });
    handle.feed.applyDelta({ room: 'W0N0', time: 1, upsert: [creep(11)], removedIds: [] });
    runtime.ticker.lastTime = 0;
    handle.setPlaybackRate(2);
    runtime.ticker.update(250);
    const root = runtime.stage.getChildByLabel('obj-c', true)!;
    expect(root.x).toBe(1050);
    handle.setPlaybackRate(0);
    runtime.ticker.update(750);
    expect(root.x).toBe(1050);
    handle.setPlaybackRate(0.5);
    runtime.ticker.update(1750);
    expect(root.x).toBe(1100);
    expect(() => handle.setPlaybackRate(-1)).toThrow(RangeError);
    expect(() => handle.setPlaybackRate(NaN)).toThrow(RangeError);
  });

  it('resize 通知在下一次绘制前更新相机，拾取使用相同投影', async () => {
    mockAssetIO();
    const handle = await mounted();
    handle.setCameraState({ x: 1000, y: 1000, zoom: 1 });
    runtime.renderer.screen.width = 1200;
    for (const listener of runtime.listeners) listener();
    expect(runtime.stage.x + 1000 * runtime.stage.scale.x).toBe(600);
    expect(handle.pickAt(600, 400).world).toEqual({ x: 1000, y: 1000 });
  });

  it('重复销毁释放独立位图、DOM/resize/feed 订阅和内核一次', async () => {
    const { bitmaps } = mockAssetIO();
    const handle = await mounted();
    await Promise.all([handle.destroy(), handle.destroy()]);
    expect(runtime.destroy).toHaveBeenCalledTimes(1);
    // 相机 6（wheel/pointer×4/contextmenu）+ 拾取悬停 5（pointer×5）
    expect(runtime.canvas.removeEventListener).toHaveBeenCalledTimes(11);
    expect(runtime.listeners.size).toBe(0);
    expect(bitmaps.every((bitmap) => bitmap.close.mock.calls.length === 1)).toBe(true);
    // feed 可作为数据仓库继续使用，不能再调用已销毁场景。
    expect(() => handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [creep(10)] })).not.toThrow();
  });

  it('资产加载失败也拆卸相机监听和已创建内核', async () => {
    mockAssetIO();
    vi.stubGlobal('fetch', async () => { throw new Error('offline'); });
    await expect(mounted()).rejects.toThrow('offline');
    expect(runtime.destroy).toHaveBeenCalledTimes(1);
    // 失败发生在拾取监听装配之前：仅相机 6 监听
    expect(runtime.canvas.removeEventListener).toHaveBeenCalledTimes(6);
  });

  it('资产成功后装配失败也释放资产与场景', async () => {
    const { bitmaps } = mockAssetIO();
    runtime.renderer.on.mockImplementation(() => { throw new Error('setup failure'); });
    await expect(mounted()).rejects.toThrow('setup failure');
    expect(runtime.destroy).toHaveBeenCalledTimes(1);
    expect(runtime.stage.destroyed).toBe(true);
    expect(bitmaps.every((bitmap) => bitmap.close.mock.calls.length === 1)).toBe(true);
  });
});

describe('R6：活动房间 / visuals / 拾取悬停 / 字体 / advanceAnimation', () => {
  it('首个快照自动激活；非活动房变更零渲染触达；切房快照语义重建', async () => {
    mockAssetIO();
    const handle = await mounted();
    const terrain = { codes: new Array(2500).fill(0) };
    handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain, objects: [creep(10)] });
    expect(handle.activeRoom).toBe('W0N0');
    expect(runtime.stage.getChildByLabel('obj-c', true)).toBeDefined();
    // 非活动房：对象与 visuals 只进仓库，场景零触达
    handle.feed.applySnapshot({ room: 'W1N1', time: 0, terrain, objects: [{ id: 'bo', type: 'creep', x: 3, y: 3, user: 'p1', body: [{ type: 'work', hits: 100 }] }] });
    expect(runtime.stage.getChildByLabel('obj-bo', true)).toBeNull();
    handle.feed.setVisuals('W1N1', [{ type: 'circle', x: 3, y: 3 }]);
    expect(handle.getStats().visuals).toBe(0);
    expect(handle.feed.visuals('W1N1')).toHaveLength(1); // 数据面照收
    // 切房=该房当前状态重建
    handle.setActiveRoom('W1N1');
    expect(handle.activeRoom).toBe('W1N1');
    expect(runtime.stage.getChildByLabel('obj-c', true)).toBeNull();
    expect(runtime.stage.getChildByLabel('obj-bo', true)).toBeDefined();
    expect(handle.getStats().visuals).toBe(1);
    // 未知房 fail-fast；切回原房 visuals 随仓库恢复
    expect(() => handle.setActiveRoom('Z9Z9')).toThrow('未知房间');
    handle.setActiveRoom('W0N0');
    expect(runtime.stage.getChildByLabel('obj-c', true)).toBeDefined();
    expect(handle.getStats().visuals).toBe(0);
  });

  it('visuals 跨增量存续（getStats 口径；短路细锚在 room-feed/visual-stage 单测）', async () => {
    mockAssetIO();
    const handle = await mounted();
    handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [] });
    const visuals: RoomVisual[] = [{ type: 'rect', x: 1, y: 1, w: 1, h: 1, style: { fill: 'transparent', stroke: '#ffffff' } }];
    handle.feed.setVisuals('W0N0', visuals);
    expect(handle.getStats().visuals).toBe(1);
    handle.feed.applyDelta({ room: 'W0N0', time: 1, upsert: [], removedIds: [] });
    expect(handle.getStats().visuals).toBe(1);
  });

  it('onPick/onHover：命中回调、变化触发、拖拽抑制、退订与销毁清理', async () => {
    mockAssetIO();
    const handle = await mounted();
    handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [creep(10)] });
    handle.setCameraState({ x: 1000, y: 1000, zoom: 1 }); // 屏幕中心 (400,400)=格 (10,10)
    const picks: string[] = [];
    const unsub = handle.onPick((r) => picks.push(`${r.cell?.x},${r.cell?.y}:${r.objectId ?? '-'}`));
    const hovers: Array<string | null> = [];
    handle.onHover((r) => hovers.push(r === null ? null : `${r.cell?.x},${r.cell?.y}:${r.objectId ?? '-'}`));
    runtime.firePointer('pointermove', { button: 0, clientX: 400, clientY: 400 });
    runtime.firePointer('pointermove', { button: 0, clientX: 400, clientY: 400 });
    expect(hovers).toEqual(['10,10:c']); // 同格同对象不重复触发
    // 高亮环随悬停出现/清除（评审 C建议6：G13 高亮环覆盖）
    const hoverLayer = () => runtime.stage.getChildByLabel('layer-hover', true)!.children.length;
    expect(hoverLayer()).toBe(1);
    runtime.firePointer('pointermove', { button: 0, clientX: 700, clientY: 400 });
    expect(hovers).toEqual(['10,10:c', '13,10:-']);
    runtime.firePointer('pointerleave', {});
    expect(hovers[hovers.length - 1]).toBeNull();
    expect(hoverLayer()).toBe(0);
    // 拖拽抑制：位移 >4px 不触发 pick（合成事件流同时真实驱动相机平移）；
    // 拖拽中的悬停同样冻结（评审 C建议4：hover 抑制断言）
    runtime.firePointer('pointerdown', { button: 0, clientX: 400, clientY: 400 });
    runtime.firePointer('pointermove', { button: 0, clientX: 500, clientY: 400 });
    expect(hovers.length).toBe(3);
    runtime.firePointer('pointerup', { button: 0, clientX: 500, clientY: 400 });
    expect(picks).toEqual([]);
    handle.setCameraState({ x: 1000, y: 1000, zoom: 1 }); // 复位拖拽平移
    // 点击判定阈值钉定（评审 C建议5）：5px > PICK_SLOP_PX=4 不触发
    runtime.firePointer('pointerdown', { button: 0, clientX: 400, clientY: 400 });
    runtime.firePointer('pointerup', { button: 0, clientX: 405, clientY: 400 });
    expect(picks).toEqual([]);
    // 点击（位移 ≤4px）触发 pick
    runtime.firePointer('pointerdown', { button: 0, clientX: 400, clientY: 400 });
    runtime.firePointer('pointerup', { button: 0, clientX: 402, clientY: 403 });
    expect(picks).toEqual(['10,10:c']);
    unsub();
    runtime.firePointer('pointerdown', { button: 0, clientX: 400, clientY: 400 });
    runtime.firePointer('pointerup', { button: 0, clientX: 400, clientY: 400 });
    expect(picks).toEqual(['10,10:c']); // 退订后不再触发
    // 销毁后 DOM 监听摘除：合成事件不再触达任何回调
    await handle.destroy();
    expect(() => runtime.firePointer('pointermove', { button: 0, clientX: 700, clientY: 400 })).not.toThrow();
    expect(runtime.canvas.removeEventListener).toHaveBeenCalledTimes(11);
  });

  it('无地形房切房以素地码重建（评审 B-B2：spy 锚接线；墙残留观感留 harness）', async () => {
    mockAssetIO();
    // node 无 canvas——墙烘焙不可达；全零地形走 hash 门控零烘焙路径
    const spy = vi.spyOn(RoomScene.prototype, 'setTerrain');
    try {
      const handle = await mounted();
      handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [] });
      expect(spy).toHaveBeenCalledTimes(1); // 快照注入
      handle.feed.setVisuals('W9N9', [{ type: 'circle', x: 5, y: 5 }]); // 无地形房（惰性建）
      handle.setActiveRoom('W9N9');
      expect(spy).toHaveBeenCalledTimes(2); // 修复前此处跳过 → 上一房地形残留
      const codes = spy.mock.calls[1]![0] as number[];
      expect(codes.length).toBe(2500);
      expect(codes.every((c) => c === 0)).toBe(true);
      expect(handle.getStats().visuals).toBe(1);
    } finally {
      spy.mockRestore();
    }
  });

  it('销毁后场景面方法 no-op、feed 数据面可用（评审 B-B5）', async () => {
    mockAssetIO();
    const handle = await mounted();
    handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [creep(10)] });
    await handle.destroy();
    expect(() => {
      handle.setActiveRoom('W0N0');
      handle.fitRoom();
      handle.setCameraState({ x: 0, y: 0, zoom: 1 });
      handle.setPlaybackRate(1);
      handle.advanceAnimation(100);
      const unsubPick = handle.onPick(() => {});
      unsubPick();
      const unsubHover = handle.onHover(() => {});
      unsubHover();
    }).not.toThrow();
    const stats = handle.getStats();
    expect(stats).toMatchObject({ fps: 0, visuals: 0, atlasFrames: 0 });
    expect(handle.pickAt(400, 400)).toEqual({ world: { x: 0, y: 0 }, cell: null });
    expect(() => handle.feed.applyDelta({ room: 'W0N0', time: 1, upsert: [], removedIds: [] })).not.toThrow();
  });

  it('advanceAnimation 与 ticker 同路径推进（不受 playbackRate 暂停影响），onFrameTiming 收到计时', async () => {
    const timings: Array<{ objectsMs: number; lightingMs: number }> = [];
    mockAssetIO();
    const handle = await createRenderer({
      container: {} as HTMLElement,
      atlasBaseUrl: 'https://renderer.invalid/',
      onFrameTiming: (t) => timings.push(t),
    });
    handles.push(handle);
    handle.feed.applySnapshot({ room: 'W0N0', time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [creep(10)] });
    handle.feed.applyDelta({ room: 'W0N0', time: 1, upsert: [creep(11)], removedIds: [] });
    runtime.ticker.lastTime = 0;
    handle.setPlaybackRate(0);
    runtime.ticker.update(500);
    const root = runtime.stage.getChildByLabel('obj-c', true)!;
    expect(root.x).toBe(1000); // 暂停中 ticker 不推进
    handle.advanceAnimation(500);
    expect(root.x).toBe(1050); // 手动推进同路径
    handle.advanceAnimation(-1);
    handle.advanceAnimation(Number.NaN);
    expect(root.x).toBe(1050); // 宽松入参按 0
    expect(timings.length).toBeGreaterThan(0);
    expect(timings.every((t) => Number.isFinite(t.objectsMs) && Number.isFinite(t.lightingMs))).toBe(true);
  });

  it('fonts 契约：缺省 unset', async () => {
    mockAssetIO();
    const handle = await mounted();
    expect(handle.getStats().fonts).toBe('unset');
  });

  it('fonts 契约：node 环境配置后 fallback（无 DOM 字体面）', async () => {
    mockAssetIO();
    const second = makeKernel();
    vi.mocked(initKernel).mockResolvedValue(second.kernel);
    const withFonts = await createRenderer({
      container: {} as HTMLElement,
      atlasBaseUrl: 'https://renderer.invalid/',
      fontsBaseUrl: 'https://renderer.invalid/',
    });
    handles.push(withFonts);
    expect(withFonts.getStats().fonts).toBe('fallback');
  });
});
