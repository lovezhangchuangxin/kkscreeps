/**
 * atlas 清单与加载：assets/atlas/*.json（Pixi spritesheet，每页一份）+
 * 直通纹理（assets/svg/*.png，TilingSprite/mask 用，必须独立纹理不可进
 * atlas）。多分辨率由调用方按 devicePixelRatio 选 1x/2x 集（显式选择，
 * 不依赖 resolver 隐式匹配）。
 */
import { Assets, type Spritesheet, type Texture } from 'pixi.js';

/** build-assets.mjs 产物 manifest（只取加载所需字段）。 */
export interface AtlasManifest {
  format: number;
  atlas: Record<string, { pages: Array<{ page: number; png: { file: string }; json: { file: string } }> }>;
}

/** 直通纹理别名（build-assets 直通资产；R2 地形/特效使用）。 */
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

export interface LoadedAtlas {
  /** 已加载 spritesheet 的帧纹理，键=资产名（如 'extension'、'tower-rotatable'）。 */
  frames: Map<string, Texture>;
  standalone: Map<string, Texture>;
}

/**
 * 从 baseUrl（assets/ 目录 URL，尾随 /）加载 atlas 与直通纹理。
 * 路径约定：manifest.json 在 baseUrl 根；spritesheet 页在 atlas/ 子目录；
 * 直通纹理在 svg/ 子目录（与包内 assets/ 布局一致）。
 * resolution 选择：按 pixelRatio 只载一档（@1x 或 @2x）——两套都载会让两份
 * 页纹理同时驻留 VRAM（§2.4 纪律：不缓存不用的纹理）。
 */
export async function loadAtlas(baseUrl: string, pixelRatio: number): Promise<LoadedAtlas> {
  const manifestResponse = await fetch(new URL('manifest.json', baseUrl));
  if (!manifestResponse.ok) throw new Error(`manifest 加载失败: HTTP ${manifestResponse.status}`);
  const manifest: AtlasManifest = await manifestResponse.json();
  if (manifest.format !== 1) throw new Error(`manifest format 不支持: ${manifest.format}`);

  const scale = pixelRatio > 1 ? '2' : '1';
  const frames = new Map<string, Texture>();
  const standalone = new Map<string, Texture>();

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

  await Promise.all(
    STANDALONE_TEXTURES.map(async (name) => {
      standalone.set(name, await Assets.load<Texture>(new URL(`svg/${name}.png`, baseUrl).href));
    }),
  );

  return { frames, standalone };
}
