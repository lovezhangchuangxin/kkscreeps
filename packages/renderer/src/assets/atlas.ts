/**
 * atlas 清单与加载：assets/atlas/*.json（Pixi spritesheet，每页一份）+
 * 独立纹理。两类独立纹理：
 *   - svg/ 直通 PNG（glow/noise/ground 等，TilingSprite/mask 需独立可 wrap 纹理，
 *     无分档，resolution=1 语义——R2 使用侧显式 scale）；
 *   - atlas/standalone/ 的出口条（exit-*，SVG 构建期栅格化，@1x/@2x 分档）。
 * 全部位图经 fetch→createImageBitmap 载入：TilingSprite 用 texture，地形 canvas
 * 烘焙用 bitmap（drawImage 需原始位图）。
 * resolution 选择：按 pixelRatio 只载一档 atlas（两套都载会让两份页纹理同时
 * 驻留 VRAM，§2.4 纪律）。
 */
import { Assets, Texture, type Spritesheet } from 'pixi.js';

/** build-assets.mjs 产物 manifest（只取加载所需字段）。 */
export interface AtlasManifest {
  format: number;
  atlas: Record<
    string,
    {
      pages: Array<{ page: number; png: { file: string }; json: { file: string } }>;
      standalone?: Array<{ name: string; file: string }>;
    }
  >;
}

/** svg/ 直通纹理名（build-assets 直通资产）。 */
export const STANDALONE_TEXTURES = [
  'glow',
  'flare1',
  'flare2',
  'flare3',
  'noise1',
  'noise2',
  'ground',
  'ground-mask',
  'creep-mask',
] as const;

/** 出口条纹理名（SVG 构建期栅格化独立产物，分档）。 */
export const EXIT_TEXTURES = ['exit-top', 'exit-bottom', 'exit-left', 'exit-right'] as const;

/** 独立纹理：texture 供 GPU 渲染，bitmap 供地形 canvas 烘焙（drawImage）。 */
export interface StandaloneTexture {
  texture: Texture;
  bitmap: ImageBitmap;
}

export interface LoadedAtlas {
  /** 已加载 spritesheet 的帧纹理，键=资产名（如 'extension'、'tower-rotatable'）。 */
  frames: Map<string, Texture>;
  /** 独立纹理（直通 PNG + 出口条），键=资产名。 */
  standalone: Map<string, StandaloneTexture>;
  /** 场景拆卸后释放资产；共享页在最后一个持有者释放时卸载。可重复调用。 */
  dispose(): Promise<void>;
}

interface SheetEntry {
  users: number;
  ready: Promise<Spritesheet>;
  unloading?: Promise<void>;
}

// Assets 缓存跨 Application 共享；页租约防一个 renderer 销毁另一个的帧源。
const sheets = new Map<string, SheetEntry>();

async function acquireSheet(
  url: string,
): Promise<{ sheet: Spritesheet; release(): Promise<void> }> {
  let entry = sheets.get(url);
  while (entry?.unloading) {
    await entry.unloading;
    entry = sheets.get(url);
  }
  if (!entry) {
    entry = { users: 0, ready: Assets.load<Spritesheet>(url) };
    sheets.set(url, entry);
  }
  const held = entry;
  held.users++;
  let sheet: Spritesheet;
  try {
    sheet = await held.ready;
  } catch (error) {
    if (--held.users === 0) sheets.delete(url);
    throw error;
  }
  let released = false;
  return {
    sheet,
    async release() {
      if (released) return;
      released = true;
      if (--held.users !== 0) return;
      // 保留卸载中的条目，让并发重挂载等待卸载完成后重新加载。
      held.unloading = Assets.unload(url).finally(() => {
        sheets.delete(url);
      });
      await held.unloading;
    },
  };
}

async function loadBitmapTexture(url: URL): Promise<StandaloneTexture> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`独立纹理加载失败: HTTP ${response.status} ${url.href}`);
  const bitmap = await createImageBitmap(await response.blob());
  try {
    // 私有纹理不进入 Pixi 全局资源缓存，寿命只由 LoadedAtlas 管理。
    return { texture: Texture.from(bitmap, true), bitmap };
  } catch (error) {
    bitmap.close();
    throw error;
  }
}

/**
 * 从 baseUrl（assets/ 目录 URL，尾随 /）加载 atlas 与独立纹理。
 * 路径约定：manifest.json 在 baseUrl 根；spritesheet 页在 atlas/ 子目录；
 * 直通纹理在 svg/ 子目录；出口条在 atlas/standalone/ 子目录（与包内 assets/
 * 布局一致）。
 */
export async function loadAtlas(baseUrl: string, pixelRatio: number): Promise<LoadedAtlas> {
  const manifestResponse = await fetch(new URL('manifest.json', baseUrl));
  if (!manifestResponse.ok) throw new Error(`manifest 加载失败: HTTP ${manifestResponse.status}`);
  const manifest: AtlasManifest = await manifestResponse.json();
  if (manifest.format !== 1) throw new Error(`manifest format 不支持: ${manifest.format}`);

  const scale = pixelRatio > 1 ? '2' : '1';
  const frames = new Map<string, Texture>();
  const standalone = new Map<string, StandaloneTexture>();
  const releases: Array<() => Promise<void>> = [];
  let disposal: Promise<void> | undefined;
  const dispose = (): Promise<void> =>
    (disposal ??= (async () => {
      for (const { texture, bitmap } of standalone.values()) {
        texture.destroy(true);
        bitmap.close();
      }
      standalone.clear();
      frames.clear();
      const results = await Promise.allSettled(releases.splice(0).map((release) => release()));
      const failure = results.find((result) => result.status === 'rejected');
      if (failure?.status === 'rejected') throw failure.reason;
    })());

  const atlas = manifest.atlas[scale];
  if (!atlas) throw new Error(`manifest 缺 ${scale}x atlas`);
  // 页间并行加载（§5 首帧预算）；页内帧纹理由 spritesheet 一次给出。
  const jobs: Array<Promise<void>> = atlas.pages.map(async (page) => {
    const { sheet, release } = await acquireSheet(new URL(`atlas/${page.json.file}`, baseUrl).href);
    releases.push(release);
    for (const [name, tex] of Object.entries(sheet.textures)) {
      frames.set(name, tex);
    }
  });
  for (const name of STANDALONE_TEXTURES) {
    jobs.push(
      loadBitmapTexture(new URL(`svg/${name}.png`, baseUrl)).then((t) => {
        standalone.set(name, t);
      }),
    );
  }
  for (const name of EXIT_TEXTURES) {
    jobs.push(
      loadBitmapTexture(new URL(`atlas/standalone/${name}-${scale}x.png`, baseUrl)).then((t) => {
        standalone.set(name, t);
      }),
    );
  }
  // 失败也等所有在途加载落定，再统一回滚；避免 Promise.all 提前拒绝后泄漏。
  const results = await Promise.allSettled(jobs);
  const failure = results.find((result) => result.status === 'rejected');
  if (failure?.status === 'rejected') {
    try {
      await dispose();
    } catch {
      /* 保留原始加载错误。 */
    }
    throw failure.reason;
  }
  return { frames, standalone, dispose };
}
