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
}

async function loadBitmapTexture(url: URL): Promise<StandaloneTexture> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`独立纹理加载失败: HTTP ${response.status} ${url.href}`);
  const bitmap = await createImageBitmap(await response.blob());
  return { texture: Texture.from(bitmap), bitmap };
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

  const atlas = manifest.atlas[scale];
  if (!atlas) throw new Error(`manifest 缺 ${scale}x atlas`);
  // 页间并行加载（§5 首帧预算）；页内帧纹理由 spritesheet 一次给出。
  await Promise.all(
    atlas.pages.map(async (page) => {
      const sheet = await Assets.load<Spritesheet>(new URL(`atlas/${page.json.file}`, baseUrl).href);
      for (const [name, tex] of Object.entries(sheet.textures)) {
        frames.set(name, tex);
      }
    }),
  );

  const jobs: Array<Promise<void>> = [];
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
  await Promise.all(jobs);

  return { frames, standalone };
}
