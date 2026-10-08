import { vi } from 'vitest';

export class TestBitmap {
  width = 32;
  height = 32;
  close = vi.fn();
}

/** 仅替换网络与解码，保留真实 Pixi Texture/TextureSource 生命周期。 */
export function mockAssetIO(pages: Array<{ json: { file: string } }> = []) {
  const bitmaps: TestBitmap[] = [];
  vi.stubGlobal('ImageBitmap', TestBitmap);
  vi.stubGlobal(
    'createImageBitmap',
    vi.fn(async () => {
      const bitmap = new TestBitmap();
      bitmaps.push(bitmap);
      return bitmap;
    }),
  );
  const fetcher = vi.fn(async (_url: URL) => ({
    ok: true,
    status: 200,
    json: async () => ({ format: 1, atlas: { '1': { pages } } }),
    blob: async () => new Blob(),
  }));
  vi.stubGlobal('fetch', fetcher);
  return { bitmaps, fetcher };
}
