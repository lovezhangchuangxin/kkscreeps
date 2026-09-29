#!/usr/bin/env node
// 资产管线：官方 SVG → @resvg/resvg-js 栅格化 @1x/@2x → maxrects-packer 打包
// → Pixi Assets 兼容 spritesheet（每页一个 JSON+PNG）入库 assets/atlas/。
// 纪律（同引擎常量管线）：生成确定性——排序遍历、无时间戳，重跑逐字节一致；
// 新鲜度由 check-assets-fresh.mjs 对照 assets/manifest.json 校验。
//
// 栅格化采样策略 = 浏览器语义（官方渲染器经浏览器解码 SVG，内在尺寸取
// width/height 属性，无属性才退 viewBox；严格 XML 下小写 `viewbox` 被忽略，
// resvg 语义天然一致——vendor 源零改动、零归一化）。80/117 文件带属性，
// 其中 60 个属性与 viewBox 分歧（宽或高任一不等，±0.01 容差；如 factory
// 属性 200 vs viewBox 41.2），按
// 属性采样才与官方
// 密度一致（评审 A 实测复核：80/117 带属性文件，其中 66 个属性与 viewBox
// 分歧——factory 属性 200 vs viewBox 41.2 等）。@2x 恰为 @1x 整 2 倍；个别
// 资产需例外时在 RASTER_OVERRIDE 表登记，不静默改全局策略。
// 页面 PNG 用内置 zlib 编码（RGBA、filter 0），无新原生依赖。
import { readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { deflateSync } from 'node:zlib';
import { Resvg } from '@resvg/resvg-js';
import { MaxRectsPacker } from 'maxrects-packer';

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const svgDir = join(pkgRoot, 'assets', 'svg');
const outDir = join(pkgRoot, 'assets', 'atlas');
const manifestPath = join(pkgRoot, 'assets', 'manifest.json');

const BIN_SIZE = 2048;
const PADDING = 2;

/** 个别资产栅格化尺寸覆盖（键=去扩展名，值=@1x 目标宽）。空表=全按内在尺寸。 */
const RASTER_OVERRIDE = {};

const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');
// 尺寸取整：@1x = ceil(内在尺寸)，@2x 恰为 @1x 的整 2 倍（倍率不变量，单测断言）。
const ceilPx = (n) => Math.ceil(n);

function intrinsicSize(svgText, name) {
  // 浏览器语义：width/height 属性 = 内在尺寸（单双引号均为合法 XML；px 单位
  // 等价无单位，其他单位本资产集不存在、不匹配即退 viewBox）；无属性退
  // viewBox；再无则兜底。
  const w = svgText.match(/<svg[^>]*?\swidth\s*=\s*["']([\d.]+)(?:px)?["']/);
  const h = svgText.match(/<svg[^>]*?\sheight\s*=\s*["']([\d.]+)(?:px)?["']/);
  if (w && h && Number(w[1]) > 0 && Number(h[1]) > 0) {
    return { width: Number(w[1]), height: Number(h[1]), from: 'width/height' };
  }
  const viewBox = svgText.match(/\bviewBox\s*=\s*["']([^"']+)["']/);
  if (viewBox) {
    const parts = viewBox[1].trim().split(/[\s,]+/).map(Number);
    if (parts.length === 4 && parts.every(Number.isFinite) && parts[2] > 0 && parts[3] > 0) {
      return { width: parts[2], height: parts[3], from: 'viewBox' };
    }
  }
  console.warn(`⚠ ${name}: 无 width/height/viewBox，兜底 100×100`);
  return { width: 100, height: 100, from: 'fallback' };
}

function renderRgba(svgText, scale, size) {
  const baseW = ceilPx(size.width);
  const baseH = ceilPx(size.height);
  const targetW = scale === 1 ? baseW : baseW * 2;
  const targetH = scale === 1 ? baseH : baseH * 2;
  const resvg = new Resvg(svgText, {
    fitTo: { mode: 'width', value: targetW },
    background: 'rgba(0,0,0,0)',
    font: { loadSystemFonts: false },
  });
  const rendered = resvg.render();
  if (rendered.width !== targetW) throw new Error(`resvg 宽度不符: ${rendered.width} != ${targetW}`);
  return { pixels: rendered.pixels, width: targetW, height: rendered.height };
}

// ---- 最小 PNG 编码（RGBA8、filter 0、zlib deflate；确定性）----
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function pngChunk(type, data) {
  const out = Buffer.alloc(8 + data.length + 4);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, 'ascii');
  data.copy(out, 8);
  out.writeUInt32BE(crc32(out.subarray(4, 8 + data.length)), 8 + data.length);
  return out;
}
function encodePng(pixels, width, height) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0; // filter: None
    pixels.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    pngChunk('IHDR', ihdr),
    pngChunk('IDAT', deflateSync(raw, { level: 9 })),
    pngChunk('IEND', Buffer.alloc(0)),
  ]);
}

function packFrames(frames) {
  const packer = new MaxRectsPacker(BIN_SIZE, BIN_SIZE, PADDING, {
    smart: true,
    pot: true,
    square: false,
    allowRotation: false,
  });
  packer.addArray(frames.map((f) => ({ width: f.width, height: f.height, data: f })));
  return packer.bins.map((bin) =>
    bin.rects.map((r) => ({ frame: r.data, x: r.x, y: r.y, width: r.width, height: r.height })),
  );
}

function main() {
  const svgFiles = readdirSync(svgDir).filter((f) => f.endsWith('.svg')).sort();
  const pngFiles = readdirSync(svgDir).filter((f) => f.endsWith('.png')).sort();
  const fontFiles = existsSync(join(pkgRoot, 'assets', 'fonts'))
    ? readdirSync(join(pkgRoot, 'assets', 'fonts')).filter((f) => f.endsWith('.woff2')).sort()
    : [];
  if (svgFiles.length === 0) throw new Error('assets/svg/ 下没有 SVG');

  const sources = [];
  const byScale = { 1: [], 2: [] };
  for (const file of svgFiles) {
    const name = file.replace(/\.svg$/, '');
    const buf = readFileSync(join(svgDir, file));
    const text = buf.toString('utf8');
    let size = intrinsicSize(text, name);
    if (RASTER_OVERRIDE[name] != null) {
      const w = RASTER_OVERRIDE[name];
      size = { width: w, height: Math.round((w * size.height) / size.width), from: 'override' };
    }
    // @1x 宽 = ceil(内在尺寸)；manifest 记录 @1x 实际渲染尺寸（viewport 属性
    // 决定纵横比；非整数内在尺寸下 @2x 高可与之差 ±1px，帧尺寸以各档实际渲染为准）。
    const raster = { width: ceilPx(size.width), height: ceilPx(size.height) };
    const rendered1 = renderRgba(text, 1, raster);
    const rendered2 = renderRgba(text, 2, raster);
    byScale[1].push({ name, ...rendered1 });
    byScale[2].push({ name, ...rendered2 });
    sources.push({
      path: `svg/${file}`,
      sha256: sha256(buf),
      bytes: buf.length,
      rasterWidth: rendered1.width,
      rasterHeight: rendered1.height,
      sizeFrom: size.from,
    });
  }
  for (const file of pngFiles) {
    const buf = readFileSync(join(svgDir, file));
    sources.push({ path: `svg/${file}`, sha256: sha256(buf), bytes: buf.length });
  }
  for (const file of fontFiles) {
    const buf = readFileSync(join(pkgRoot, 'assets', 'fonts', file));
    sources.push({ path: `fonts/${file}`, sha256: sha256(buf), bytes: buf.length });
  }

  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });

  const manifest = { format: 1, sources, atlas: {} };
  for (const scale of [1, 2]) {
    const pagesRects = packFrames(byScale[scale]);
    const pageEntries = pagesRects.map((rects, pageIdx) => {
      const pagePixels = Buffer.alloc(BIN_SIZE * BIN_SIZE * 4);
      const frames = {};
      for (const r of rects) {
        const { pixels, width, height, name } = r.frame;
        for (let y = 0; y < height; y++) {
          const srcStart = y * width * 4;
          pixels.copy(pagePixels, (r.y + y) * BIN_SIZE * 4 + r.x * 4, srcStart, srcStart + width * 4);
        }
        frames[name] = { page: pageIdx, x: r.x, y: r.y, w: width, h: height };
      }
      const pngName = `atlas-${scale}x-${pageIdx}.png`;
      const pngBuf = encodePng(pagePixels, BIN_SIZE, BIN_SIZE);
      writeFileSync(join(outDir, pngName), pngBuf);
      const jsonName = `atlas-${scale}x-${pageIdx}.json`;
      const sheet = {
        frames: Object.fromEntries(
          Object.entries(frames).map(([name, f]) => [
            name,
            {
              frame: { x: f.x, y: f.y, w: f.w, h: f.h },
              rotated: false,
              trimmed: false,
              spriteSourceSize: { x: 0, y: 0, w: f.w, h: f.h },
              sourceSize: { w: f.w, h: f.h },
            },
          ]),
        ),
        animations: {},
        meta: {
          app: 'kkscreeps-build-assets',
          version: '1.0',
          image: pngName,
          format: 'RGBA8888',
          size: { w: BIN_SIZE, h: BIN_SIZE },
          scale: String(scale),
        },
      };
      const jsonBuf = Buffer.from(JSON.stringify(sheet));
      writeFileSync(join(outDir, jsonName), jsonBuf);
      return { page: pageIdx, png: { file: pngName, sha256: sha256(pngBuf) }, json: { file: jsonName, sha256: sha256(jsonBuf) }, frames };
    });
    manifest.atlas[String(scale)] = {
      pages: pageEntries.map(({ page, png, json }) => ({ page, png, json })),
      frames: Object.fromEntries(pageEntries.flatMap((p) => Object.entries(p.frames).map(([k, v]) => [k, v]))),
    };
  }

  writeFileSync(manifestPath, JSON.stringify(manifest));

  const summary = Object.fromEntries([1, 2].map((s) => [`${s}x`, manifest.atlas[String(s)].pages.length]));
  console.log(`✓ 栅格化 ${svgFiles.length} SVG → atlas ${JSON.stringify(summary)} 页（${pngFiles.length} PNG 与 ${fontFiles.length} 字体为直通资产，记入 manifest）`);
  console.log(`✓ manifest.json：${sources.length} 源条目，确定性生成可重跑比对`);
}

main();
