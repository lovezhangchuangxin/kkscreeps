import { afterEach, describe, expect, it, vi } from 'vitest';
import { Assets, Cache, Texture, TextureSource } from 'pixi.js';
import { loadAtlas } from '../src/assets/atlas.js';
import { mockAssetIO } from './asset-mocks.js';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('atlas 所有权与加载回滚', () => {
  it('私有纹理不进全局 Cache，重复 dispose 只释放一次纹理和位图', async () => {
    const { bitmaps } = mockAssetIO();
    const atlas = await loadAtlas('https://assets.invalid/private/', 1);
    const owned = [...atlas.standalone.values()].map(({ texture }) => ({
      texture,
      source: texture.source,
    }));
    expect(bitmaps).toHaveLength(13);
    expect(bitmaps.every((bitmap) => !Cache.has(bitmap as unknown as string))).toBe(true);
    await Promise.all([atlas.dispose(), atlas.dispose()]);
    expect(owned.every(({ texture, source }) => texture.destroyed && source.destroyed)).toBe(true);
    expect(bitmaps.every((bitmap) => bitmap.close.mock.calls.length === 1)).toBe(true);
    expect(atlas.standalone.size).toBe(0);
  });

  it('两个 renderer 共用 atlas 页，最后一次释放才卸载', async () => {
    mockAssetIO([{ json: { file: 'page.json' } }]);
    const texture = new Texture({ source: new TextureSource({ width: 128, height: 128 }) });
    const load = vi.spyOn(Assets, 'load').mockResolvedValue({ textures: { link: texture } });
    const unload = vi.spyOn(Assets, 'unload').mockImplementation(async () => {
      texture.destroy(true);
    });
    const [a, b] = await Promise.all([
      loadAtlas('https://assets.invalid/shared/', 1),
      loadAtlas('https://assets.invalid/shared/', 1),
    ]);
    expect(load).toHaveBeenCalledTimes(1);
    expect(a.frames.get('link')).toBe(b.frames.get('link'));
    await a.dispose();
    expect(unload).not.toHaveBeenCalled();
    expect(b.frames.get('link')!.destroyed).toBe(false);
    await b.dispose();
    expect(unload).toHaveBeenCalledTimes(1);
    expect(texture.destroyed).toBe(true);
  });

  it('失败也等待在途成功项，再关闭所有已解码位图并归还页租约', async () => {
    const { bitmaps, fetcher } = mockAssetIO([{ json: { file: 'page.json' } }]);
    fetcher.mockImplementation(async (url) => {
      if (url.href.endsWith('flare1.png')) throw new Error('network failure');
      // 包括一个延迟完成的成功项，防止提前 reject 后的漏释放。
      await new Promise((resolve) => setTimeout(resolve, 5));
      return {
        ok: true,
        status: 200,
        json: async () => ({
          format: 1,
          atlas: { '1': { pages: [{ json: { file: 'page.json' } }] } },
        }),
        blob: async () => new Blob(),
      };
    });
    vi.spyOn(Assets, 'load').mockResolvedValue({ textures: {} });
    const unload = vi.spyOn(Assets, 'unload').mockResolvedValue(undefined);
    await expect(loadAtlas('https://assets.invalid/rollback/', 1)).rejects.toThrow(
      'network failure',
    );
    expect(bitmaps).toHaveLength(12);
    expect(bitmaps.every((bitmap) => bitmap.close.mock.calls.length === 1)).toBe(true);
    expect(unload).toHaveBeenCalledTimes(1);
  });

  it('页加载失败不污染租约表，重试可以重新加载', async () => {
    mockAssetIO([{ json: { file: 'page.json' } }]);
    const load = vi
      .spyOn(Assets, 'load')
      .mockRejectedValueOnce(new Error('bad sheet'))
      .mockResolvedValue({ textures: {} });
    const unload = vi.spyOn(Assets, 'unload').mockResolvedValue(undefined);
    await expect(loadAtlas('https://assets.invalid/retry/', 1)).rejects.toThrow('bad sheet');
    const atlas = await loadAtlas('https://assets.invalid/retry/', 1);
    expect(load).toHaveBeenCalledTimes(2);
    await atlas.dispose();
    expect(unload).toHaveBeenCalledTimes(1);
  });

  it('卸载尚未结束时重挂载，等待卸载后再加载，避免取到被毁帧', async () => {
    mockAssetIO([{ json: { file: 'page.json' } }]);
    const load = vi.spyOn(Assets, 'load').mockResolvedValue({ textures: {} });
    let finish!: () => void;
    const unload = vi
      .spyOn(Assets, 'unload')
      .mockImplementationOnce(
        () =>
          new Promise<void>((resolve) => {
            finish = resolve;
          }),
      )
      .mockResolvedValue(undefined);
    const a = await loadAtlas('https://assets.invalid/remount/', 1);
    const disposal = a.dispose();
    const remount = loadAtlas('https://assets.invalid/remount/', 1);
    await new Promise((resolve) => setTimeout(resolve, 5));
    expect(load).toHaveBeenCalledTimes(1);
    finish();
    await disposal;
    const b = await remount;
    expect(load).toHaveBeenCalledTimes(2);
    await b.dispose();
    expect(unload).toHaveBeenCalledTimes(2);
  });

  it('Texture 创建失败也关闭已经解码的位图', async () => {
    const { bitmaps } = mockAssetIO();
    vi.spyOn(Texture, 'from').mockImplementation(() => {
      throw new Error('bad source');
    });
    await expect(loadAtlas('https://assets.invalid/source-failure/', 1)).rejects.toThrow(
      'bad source',
    );
    expect(bitmaps).toHaveLength(13);
    expect(bitmaps.every((bitmap) => bitmap.close.mock.calls.length === 1)).toBe(true);
  });
});
