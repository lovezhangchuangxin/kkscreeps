import { readFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// 资产管线静态校验：manifest 完整性、atlas 几何（不重叠/不越界/双分辨率一致）、
// 许可随附。运行时加载行为由 harness 验证（单测不 import Pixi）。
const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const manifest = JSON.parse(readFileSync(join(pkgRoot, 'assets', 'manifest.json'), 'utf8')) as {
  format: number;
  sources: Array<{ path: string; sha256: string; bytes: number; rasterWidth?: number; rasterHeight?: number; sizeFrom?: string }>;
  atlas: Record<string, { pages: Array<{ page: number; png: { file: string; sha256: string }; json: { file: string; sha256: string } }>; frames: Record<string, { page: number; x: number; y: number; w: number; h: number }>; standalone?: Array<{ name: string; file: string; sha256: string; width: number; height: number }> }>;
};

const BIN = 2048;

describe('assets/manifest.json', () => {
  it('format=1，源面 = 117 SVG + 9 PNG + 2 字体', () => {
    expect(manifest.format).toBe(1);
    const svg = manifest.sources.filter((s) => s.path.startsWith('svg/') && s.path.endsWith('.svg'));
    const png = manifest.sources.filter((s) => s.path.startsWith('svg/') && s.path.endsWith('.png'));
    const fonts = manifest.sources.filter((s) => s.path.startsWith('fonts/'));
    expect(svg.length).toBe(117);
    expect(png.length).toBe(9);
    expect(fonts.length).toBe(2);
  });

  it('每个 SVG 源的栅格尺寸 = 浏览器语义内在尺寸（width/height 属性优先，退 viewBox）', () => {
    for (const s of manifest.sources) {
      if (!s.path.endsWith('.svg')) continue;
      const text = readFileSync(join(pkgRoot, 'assets', s.path), 'utf8');
      const w = text.match(/<svg[^>]*?\swidth\s*=\s*["']([\d.]+)(?:px)?["']/);
      const h = text.match(/<svg[^>]*?\sheight\s*=\s*["']([\d.]+)(?:px)?["']/);
      let expectW: number;
      let expectH: number;
      let from: string;
      if (w && h && Number(w[1]) > 0 && Number(h[1]) > 0) {
        expectW = Number(w[1]);
        expectH = Number(h[1]);
        from = 'width/height';
      } else {
        const vb = text.match(/\bviewBox\s*=\s*["']([^"']+)["']/);
        if (!vb) throw new Error(`${s.path}: 无 width/height 也无 viewBox（build 侧会走 100×100 兜底告警）`);
        const p = vb[1]!.trim().split(/[\s,]+/).map(Number);
        expectW = p[2]!;
        expectH = p[3]!;
        from = 'viewBox';
      }
      expect(s.sizeFrom, s.path).toBe(from);
      expect(s.rasterWidth, s.path).toBe(Math.ceil(expectW));
      expect(s.rasterHeight, s.path).toBe(Math.ceil(expectH));
    }
  });

  it('字体是合法 woff2（wOF2 魔数）', () => {
    for (const s of manifest.sources) {
      if (!s.path.startsWith('fonts/')) continue;
      const magic = readFileSync(join(pkgRoot, 'assets', s.path)).subarray(0, 4).toString('ascii');
      expect(magic, s.path).toBe('wOF2');
    }
  });

  it('许可随附且内容正确：ISC（官方美术）+ OFL-1.1（字体）', () => {
    const isc = readFileSync(join(pkgRoot, 'assets', 'svg', 'LICENSE.txt'), 'utf8');
    expect(isc).toContain('ISC');
    expect(isc).toContain('Artem Chivchalov');
    const ofl = readFileSync(join(pkgRoot, 'assets', 'fonts', 'LICENSE'), 'utf8');
    expect(ofl).toContain('Open Font License');
  });
});

describe('atlas 几何', () => {
  it('1x/2x 各有帧表，帧数一致且覆盖全部 SVG', () => {
    const svgNames = manifest.sources
      .filter((s) => s.path.endsWith('.svg'))
      .map((s) => s.path.split('/').at(-1)!.replace('.svg', ''))
      .sort();
    for (const scale of ['1', '2']) {
      expect(Object.keys(manifest.atlas[scale]!.frames).sort()).toEqual(svgNames);
    }
  });

  it('帧不越界、同页内不重叠', () => {
    for (const scale of ['1', '2']) {
      const frames = Object.entries(manifest.atlas[scale]!.frames);
      for (const [name, f] of frames) {
        expect(f.x, `${scale}x ${name}`).toBeGreaterThanOrEqual(0);
        expect(f.y).toBeGreaterThanOrEqual(0);
        expect(f.x + f.w).toBeLessThanOrEqual(BIN);
        expect(f.y + f.h).toBeLessThanOrEqual(BIN);
      }
      const byPage = new Map<number, Array<[string, typeof frames[number][1]]>>();
      for (const entry of frames) {
        if (!byPage.has(entry[1].page)) byPage.set(entry[1].page, []);
        byPage.get(entry[1].page)!.push(entry);
      }
      for (const [page, entries] of byPage) {
        for (let i = 0; i < entries.length; i++) {
          for (let j = i + 1; j < entries.length; j++) {
            const a = entries[i]![1];
            const b = entries[j]![1];
            const overlap = a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
            expect(overlap, `${scale}x 页${page} 帧重叠: ${entries[i]![0]} vs ${entries[j]![0]}`).toBe(false);
          }
        }
      }
    }
  });

  it('帧尺寸与 manifest 栅格尺寸一致（1x 帧精确相等，2x 与 2 倍差 ≤1px）', () => {
    const raster = Object.fromEntries(
      manifest.sources
        .filter((s) => s.rasterWidth != null && s.rasterHeight != null)
        .map((s) => [s.path.split('/').at(-1)!.replace('.svg', ''), s as typeof s & { rasterWidth: number; rasterHeight: number }]),
    );
    for (const [name, f1] of Object.entries(manifest.atlas['1']!.frames)) {
      const r = raster[name]!;
      expect(f1.w, name).toBe(r.rasterWidth);
      expect(f1.h, name).toBe(r.rasterHeight);
      const f2 = manifest.atlas['2']!.frames[name]!;
      expect(Math.abs(f2.w - r.rasterWidth * 2), `${name} 2x 宽`).toBeLessThanOrEqual(1);
      expect(Math.abs(f2.h - r.rasterHeight * 2), `${name} 2x 高`).toBeLessThanOrEqual(1);
    }
  });

  it('引用的 PNG/JSON 产物在盘上且是合法 PNG 头（2048²）', () => {
    for (const scale of ['1', '2']) {
      for (const page of manifest.atlas[scale]!.pages) {
        const pngPath = join(pkgRoot, 'assets', 'atlas', page.png.file);
        const jsonPath = join(pkgRoot, 'assets', 'atlas', page.json.file);
        expect(existsSync(pngPath), page.png.file).toBe(true);
        expect(existsSync(jsonPath), page.json.file).toBe(true);
        const header = readFileSync(pngPath).subarray(0, 24);
        expect([...header.subarray(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
        expect(header.readUInt32BE(16)).toBe(BIN);
        expect(header.readUInt32BE(20)).toBe(BIN);
      }
    }
  });

  it('spritesheet JSON meta 与页文件互指一致', () => {
    for (const scale of ['1', '2']) {
      for (const page of manifest.atlas[scale]!.pages) {
        const sheet = JSON.parse(readFileSync(join(pkgRoot, 'assets', 'atlas', page.json.file), 'utf8'));
        expect(sheet.meta.image).toBe(page.png.file);
        expect(sheet.meta.scale).toBe(scale);
        expect(sheet.meta.size).toEqual({ w: BIN, h: BIN });
        expect(Object.keys(sheet.frames).length).toBeGreaterThan(0);
      }
    }
  });

  it('出口条独立纹理（standalone）：四边 × 双档在盘且为合法 PNG（sha256 由 check-assets-fresh 承担）', () => {
    for (const scale of ['1', '2']) {
      const standalone = manifest.atlas[scale]!.standalone;
      expect(standalone?.map((s) => s.name).sort()).toEqual(
        ['exit-bottom', 'exit-left', 'exit-right', 'exit-top'],
      );
      for (const entry of standalone!) {
        const abs = join(pkgRoot, 'assets', 'atlas', entry.file);
        expect(existsSync(abs), entry.file).toBe(true);
        const header = readFileSync(abs).subarray(0, 24);
        expect([...header.subarray(0, 8)]).toEqual([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
      }
    }
  });
});
