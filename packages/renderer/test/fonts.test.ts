import { afterEach, describe, expect, it, vi } from 'vitest';
import { ensureFonts, FONT_FAMILY, FONT_SPECS } from '../src/assets/fonts.js';

// 字体契约（G7/D5）：无 DOM 环境回退；缓存同 baseUrl 复用；规格单源。
// FontFace 真实加载路径在 harness 人工验证（node 无 document.fonts）。

afterEach(() => vi.unstubAllGlobals());

describe('ensureFonts', () => {
  it('无 DOM 字体面 → fallback 且不抛错', async () => {
    expect(typeof document).toBe('undefined');
    await expect(ensureFonts('https://renderer.invalid/')).resolves.toBe('fallback');
  });

  it('baseUrl 无尾随斜杠归一后缓存复用（同键同 Promise）', async () => {
    const a = ensureFonts('https://x.invalid');
    const b = ensureFonts('https://x.invalid/');
    expect(a).toBe(b);
    await expect(a).resolves.toBe('fallback');
  });

  it('fallback 不驻留缓存：在途共享、落定后重建实例可重试（评审 B-B1）', async () => {
    const a = ensureFonts('https://retry.invalid/');
    const b = ensureFonts('https://retry.invalid/');
    expect(b).toBe(a); // 在途共享
    await expect(a).resolves.toBe('fallback');
    const c = ensureFonts('https://retry.invalid/');
    expect(c).not.toBe(a); // 落定 fallback → 缓存已清 → 新 Promise（可重试）
    await expect(c).resolves.toBe('fallback');
  });

  it('部分失败重试不累积 face；恢复后复用成功缓存', async () => {
    const resident = new Set<FontFace>(); let fail = true;
    const load = vi.fn(function (this: FontFace) {
      return this.weight === '700' && fail ? Promise.reject(new Error('network')) : Promise.resolve(this);
    });
    vi.stubGlobal('document', { fonts: { add: (f: FontFace) => resident.add(f), delete: (f: FontFace) => resident.delete(f) } });
    vi.stubGlobal('FontFace', class {
      weight: string;
      constructor(_family: string, _source: string, options: FontFaceDescriptors) { this.weight = options.weight!; }
      load = load;
    });
    for (let i = 0; i < 3; i++) {
      await expect(ensureFonts('https://partial.invalid/')).resolves.toBe('fallback');
      expect(resident.size).toBe(0);
    }
    fail = false;
    await expect(ensureFonts('https://partial.invalid/')).resolves.toBe('loaded');
    expect(resident.size).toBe(3);
    const calls = load.mock.calls.length;
    await expect(ensureFonts('https://partial.invalid/')).resolves.toBe('loaded');
    expect(load).toHaveBeenCalledTimes(calls);
  });
  it.each(['constructor', 'add', 'load'])('同步 %s 异常回滚且不 reject', async (failure) => {
    const resident = new Set<object>(); let made = 0;
    vi.stubGlobal('document', { fonts: {
      add: (f: object) => { resident.add(f); if (failure === 'add' && made === 2) throw new Error('add'); },
      delete: (f: object) => resident.delete(f),
    } });
    vi.stubGlobal('FontFace', class {
      constructor() { if (++made === 2 && failure === 'constructor') throw new Error('constructor'); }
      load() { if (made === 2 && failure === 'load') throw new Error('load'); return Promise.resolve(this); }
    });
    await expect(ensureFonts(`https://${failure}.invalid/`)).resolves.toBe('fallback');
    expect(resident.size).toBe(0);
  });
  it('规格表=库内文字件实际使用面（badge 82bold / say 60 / 名字 55）', () => {
    expect(FONT_FAMILY).toBe('Roboto');
    expect(FONT_SPECS).toEqual([
      { weight: '400', size: 55 },
      { weight: '400', size: 60 },
      { weight: '700', size: 82 },
    ]);
  });
});
