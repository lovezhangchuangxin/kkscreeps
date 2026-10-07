#!/usr/bin/env node
// Optional local comparison only: no official runtime dependency or vendoring.
import { copyFileSync, existsSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const source = process.argv[2];
if (!source) throw new Error('Usage: node scripts/prepare-official-reference.mjs <official renderer checkout>');
const official = resolve(source), pkg = fileURLToPath(new URL('../', import.meta.url));
const inputs = {
  assets: join(pkg, 'assets'), fixtures: join(pkg, 'fixtures'),
};
for (const path of Object.values(inputs)) if (!existsSync(path)) throw new Error(`Missing reference input: ${path}`);
const config = readFileSync(join(official, 'demo/src/config/resourceMap.js'), 'utf8');
const resources = Object.fromEntries([...config.matchAll(/\s+([\w-]+|'[^']+'): '([^']+)'/g)]
  .map(([, key, file]) => [key.replaceAll("'", ''), `/assets/svg/${file}`]));
if (Object.keys(resources).length < 100) throw new Error('Unsupported official resourceMap format');
const out = mkdtempSync(join(tmpdir(), 'kkscreeps-official-reference-'));
for (const [name, path] of Object.entries(inputs)) symlinkSync(path, join(out, name));
// Bundle both source trees together with the engine global Pixi initializer, as the
// official demo does. Build only in tmp; checkout stays read-only.
const entry = join(out, 'reference-entry.js');
writeFileSync(entry, `import GameRenderer from ${JSON.stringify(join(official, 'engine/src/index.js'))};\nimport metadata from ${JSON.stringify(join(official, 'metadata/src/index.js'))};\nwindow.GameRenderer = GameRenderer; window.RENDERER_METADATA = metadata;\n`);
const requireOfficial = createRequire(join(official, 'demo/package.json'));
const webpack = requireOfficial('webpack');
await new Promise((resolveBuild, reject) => webpack({ mode: 'development', devtool: false, entry,
  output: { path: out, filename: 'renderer.js' },
  resolve: { extensions: ['.js', '.json'], fallback: { path: requireOfficial.resolve('path-browserify') } },
  optimization: { minimize: false },
}, (error, stats) => {
  if (error || stats?.hasErrors()) reject(error || new Error(stats.toString({ all: false, errors: true })));
  else resolveBuild();
}));
copyFileSync(new URL('official-reference.html', import.meta.url), join(out, 'index.html'));
writeFileSync(join(out, 'resources.json'), JSON.stringify(resources));
console.log(out);
console.log('Serve this directory with a local HTTP server; ?fixture=effects&tick=1&sample=450 selects a frozen reference frame.');
