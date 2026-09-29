#!/usr/bin/env node
// 资产新鲜度校验：assets/manifest.json（build-assets.mjs 生成）登记的
// 源文件（SVG/PNG/字体）与 atlas 产物逐个重算 sha256 比对，并核对目录
// 文件集与清单严格一致（多文件=忘了重跑 build；少文件=清单过期）。
// 挂在 renderer 包 test 链最前（根 `pnpm run check` 自动拾取）。
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

function main() {
  if (!existsSync(join(pkgRoot, 'assets', 'manifest.json'))) {
    console.error('✗ assets/manifest.json 不存在——先运行 pnpm run build:assets');
    process.exit(1);
  }
  const manifest = JSON.parse(readFileSync(join(pkgRoot, 'assets', 'manifest.json'), 'utf8'));
  const problems = [];

  for (const entry of manifest.sources) {
    const abs = join(pkgRoot, 'assets', entry.path);
    if (!existsSync(abs)) {
      problems.push(`清单有但磁盘无: ${entry.path}`);
      continue;
    }
    const buf = readFileSync(abs);
    if (buf.length !== entry.bytes) problems.push(`字节数不符: ${entry.path}（磁盘 ${buf.length} ≠ 清单 ${entry.bytes}）`);
    if (sha256(buf) !== entry.sha256) problems.push(`sha256 不符: ${entry.path}（源文件已改，需重跑 pnpm run build:assets）`);
  }

  const listedByDir = new Map();
  for (const entry of manifest.sources) {
    const dir = entry.path.split('/')[0];
    if (!listedByDir.has(dir)) listedByDir.set(dir, new Set());
    listedByDir.get(dir).add(entry.path.split('/').slice(1).join('/'));
  }
  for (const [dir, exts] of [
    ['svg', ['.svg', '.png']],
    ['fonts', ['.woff2']],
  ]) {
    const absDir = join(pkgRoot, 'assets', dir);
    const actual = existsSync(absDir)
      ? readdirSync(absDir).filter((f) => exts.some((e) => f.endsWith(e))).sort()
      : [];
    const listed = [...(listedByDir.get(dir) ?? [])].sort();
    for (const f of actual) if (!listed.includes(f)) problems.push(`磁盘多出（清单缺）: ${dir}/${f}`);
    for (const f of listed) if (!actual.includes(f)) problems.push(`清单有（磁盘无）: ${dir}/${f}`);
  }

  for (const scale of Object.keys(manifest.atlas)) {
    for (const page of manifest.atlas[scale].pages) {
      for (const key of ['png', 'json']) {
        const abs = join(pkgRoot, 'assets', 'atlas', page[key].file);
        if (!existsSync(abs)) {
          problems.push(`atlas 产物缺失: ${page[key].file}`);
          continue;
        }
        if (sha256(readFileSync(abs)) !== page[key].sha256) {
          problems.push(`atlas 产物过期: ${page[key].file}（源变更后需重跑 pnpm run build:assets）`);
        }
      }
    }
    for (const standalone of manifest.atlas[scale].standalone ?? []) {
      const abs = join(pkgRoot, 'assets', 'atlas', standalone.file);
      if (!existsSync(abs)) {
        problems.push(`standalone 产物缺失: ${standalone.file}`);
        continue;
      }
      if (sha256(readFileSync(abs)) !== standalone.sha256) {
        problems.push(`standalone 产物过期: ${standalone.file}（源变更后需重跑 pnpm run build:assets）`);
      }
    }
  }

  if (problems.length > 0) {
    for (const p of problems) console.error(`✗ ${p}`);
    console.error(`✗ 资产新鲜度校验失败（${problems.length} 项）——运行 pnpm run build:assets 重新生成`);
    process.exit(1);
  }
  console.log(`✓ 资产新鲜度：${manifest.sources.length} 源 + atlas ${Object.entries(manifest.atlas).map(([s, a]) => `${s}x×${a.pages.length}页`).join(' / ')} 全部一致`);
}

main();
