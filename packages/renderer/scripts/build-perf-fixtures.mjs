import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { makeFullRoom } from '../src/perf/fixture.ts';
const fixtureDir = new URL('../fixtures/', import.meta.url);
const prototypes = JSON.parse(readFileSync(new URL('showcase.json', fixtureDir), 'utf8')).ticks[0].objects;
const file = fileURLToPath(new URL('perf-full-room.json', fixtureDir));
writeFileSync(file, JSON.stringify(makeFullRoom(prototypes)) + '\n');
console.log(`R7 synthetic 300 objects / 50 creeps / 100 visuals: ${file}`);
