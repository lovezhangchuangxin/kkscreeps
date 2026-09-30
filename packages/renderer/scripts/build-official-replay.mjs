#!/usr/bin/env node
// Reads official public demo files only. Never modifies the official checkout or engine.
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { makeOfficialFixture } from '../src/perf/official-replay.ts';

const checkout = process.argv[2];
if (!checkout) throw new Error('Usage: node scripts/build-official-replay.mjs <official renderer checkout> [output.json]');
const root = resolve(checkout);
const output = process.argv[3] ? resolve(process.argv[3]) : fileURLToPath(new URL('../fixtures/official-room.json', import.meta.url));
if (!output.endsWith('.json')) throw new Error('Output must end in .json (license is saved alongside it)');
const inputs = ['demo/src/config/objects.json', 'demo/src/config/terrain.json'];
const buffers = inputs.map(path => readFileSync(join(root, path)));
const fixture = makeOfficialFixture(JSON.parse(buffers[0]), JSON.parse(buffers[1]));
const revision = execFileSync('rtk', ['proxy', 'git', '-C', root, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
const provenance = { kind: 'official-demo-recording', source: 'https://github.com/screeps/renderer', revision,
  inputs: inputs.map((path, i) => ({ path, sha256: createHash('sha256').update(buffers[i]).digest('hex') })),
  adapter: 'scripts/build-official-replay.mjs; compressed JSON restored with demo merge semantics',
  notFrozenPerformanceFixture: true, decorationsIncluded: false, originalPlayer: '561e4d4645f3f7244a7622e8', tickDurationMs: 2500 };
writeFileSync(output, JSON.stringify({ ...fixture, provenance }) + '\n');
writeFileSync(output.replace(/\.json$/, '.LICENSE.txt'), readFileSync(join(root, 'LICENSE.txt')));
console.log(`Official recording: ${fixture.room}, ${fixture.ticks.length} ticks → ${output}`);
