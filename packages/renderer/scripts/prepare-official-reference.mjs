#!/usr/bin/env node
// Optional local comparison only: no official runtime dependency or vendoring.
import { copyFileSync, existsSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const source = process.argv[2];
if (!source) throw new Error('Usage: node scripts/prepare-official-reference.mjs <official renderer checkout>');
const official = resolve(source), pkg = fileURLToPath(new URL('../', import.meta.url));
const inputs = {
  'renderer.js': join(official, 'engine/dist/renderer.js'),
  'metadata.js': join(official, 'metadata/dist/renderer-metadata.js'),
  assets: join(pkg, 'assets'), fixtures: join(pkg, 'fixtures'),
};
for (const path of Object.values(inputs)) if (!existsSync(path)) throw new Error(`Missing reference input: ${path}`);
const config = readFileSync(join(official, 'demo/src/config/resourceMap.js'), 'utf8');
const resources = Object.fromEntries([...config.matchAll(/\s+([\w-]+|'[^']+'): '([^']+)'/g)]
  .map(([, key, file]) => [key.replaceAll("'", ''), `/assets/svg/${file}`]));
if (Object.keys(resources).length < 100) throw new Error('Unsupported official resourceMap format');
const out = mkdtempSync(join(tmpdir(), 'kkscreeps-official-reference-'));
for (const [name, path] of Object.entries(inputs)) symlinkSync(path, join(out, name));
copyFileSync(new URL('official-reference.html', import.meta.url), join(out, 'index.html'));
writeFileSync(join(out, 'resources.json'), JSON.stringify(resources));
console.log(out);
console.log('Serve this directory with a local HTTP server; ?fixture=effects&tick=1&sample=450 selects a frozen reference frame.');
