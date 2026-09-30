import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Container, Ticker } from 'pixi.js';
import { createRenderer, type RendererHandle } from '../src/index.js';
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
  const canvas = { style: {}, addEventListener: vi.fn(), removeEventListener: vi.fn() };
  const destroy = vi.fn(async () => { ticker.destroy(); stage.destroy({ children: true }); });
  const kernel: Kernel = { stage, backend: 'webgl', destroy,
    app: { stage, ticker, renderer, canvas } as unknown as Kernel['app'] };
  return { kernel, ticker, renderer, canvas, listeners, destroy, stage };
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
    expect(runtime.canvas.removeEventListener).toHaveBeenCalledTimes(6);
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
