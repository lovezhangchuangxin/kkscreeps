/**
 * Roboto 预载（G7 契约：字体加载责任在库——R6 D5 拍板）。库自建 FontFace
 * 挂 document.fonts（宿主页面无需 @font-face）；同一 baseUrl 只加载一次
 * （模块级缓存，多实例不重复下载）。allSettled 非阻塞失败语义：任一规格
 * 失败 → 'fallback'（回退字体渲染，失败 face 自 document.fonts 摘除），
 * 不抛错不阻塞首帧。
 *
 * 三规格 = 库内文字件实际使用面单源：badge 82 bold（objects/specs.ts:69）、
 * 名字 55（specs.ts:206）、say 60（anim/effect-specs.ts:12）。
 * 状态面 getStats().fonts（普通数据）：'unset'=未配 fontsBaseUrl；
 * 'loaded'=三规格全部就绪；'fallback'=已配置但加载失败/无 DOM 字体面。
 */
export const FONT_FAMILY = 'Roboto';

export type FontStatus = 'unset' | 'loaded' | 'fallback';

export const FONT_SPECS = [
  { weight: '400', size: 55 },
  { weight: '400', size: 60 },
  { weight: '700', size: 82 },
] as const;

const cache = new Map<string, Promise<FontStatus>>();

export function ensureFonts(baseUrl: string): Promise<FontStatus> {
  const root = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`;
  let pending = cache.get(root);
  if (pending === undefined) {
    // fallback 不驻留缓存（评审 B-B1：resolve 路径也要清，否则「失败重试」
    // 是死代码）——网络恢复后重建 renderer 可重新加载
    pending = load(root).then((status) => {
      if (status === 'fallback') cache.delete(root);
      return status;
    });
    cache.set(root, pending);
  }
  return pending;
}

async function load(root: string): Promise<FontStatus> {
  try {
    if (typeof document === 'undefined' || !('fonts' in document)) return 'fallback';
    const faces = FONT_SPECS.map((spec) => {
      const url = new URL(`fonts/roboto-latin-${spec.weight}-normal.woff2`, root).href;
      const face = new FontFace(FONT_FAMILY, `url(${url})`, { weight: spec.weight });
      document.fonts.add(face);
      return face.load().then(
        () => true,
        () => {
          document.fonts.delete(face);
          return false;
        },
      );
    });
    const results = await Promise.all(faces);
    return results.every(Boolean) ? 'loaded' : 'fallback';
  } catch {
    // 永不 reject（评审 GPT-P2#9）：构造异常等同加载失败——走 fallback
    // 路径（缓存随之清除，可重试），调用方 await 不会跳出统一回滚
    return 'fallback';
  }
}
