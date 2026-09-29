import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { isDelta, isSnapshot, type FixtureFile } from '@kkscreeps/shared';

// 入库夹具的契约一致性：四张夹具形状、时序、单调时间轴、地形常量长度。

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixturesDir = join(pkgRoot, 'fixtures');

const names = readdirSync(fixturesDir).filter((f) => f.endsWith('.json')).sort();

describe('入库夹具', () => {
  it('四张夹具齐全（plains/standard/swamp-walls/dense-edge）', () => {
    expect(names).toEqual(['dense-edge.json', 'plains.json', 'standard.json', 'swamp-walls.json']);
  });

  for (const name of names) {
    it(`${name}：首快照 + 增量链，时间单调，地形 2500 码`, () => {
      const fixture = JSON.parse(readFileSync(join(fixturesDir, name), 'utf8')) as FixtureFile;
      expect(isSnapshot(fixture.ticks[0])).toBe(true);
      expect(fixture.ticks.slice(1).every((t) => isDelta(t))).toBe(true);
      expect(fixture.ticks).toHaveLength(31); // 1 快照 + 30 增量（exporter 缺省 ticks）

      let prevTime = -1;
      for (const entry of fixture.ticks) {
        expect(entry.time).toBeGreaterThan(prevTime);
        prevTime = entry.time;
        expect(entry.room).toBe(fixture.room);
      }
      expect(fixture.ticks[0]!.terrain.codes).toHaveLength(2500);
      // 每个增量至少动一处（aging/decay/spawning 的自然差量）——空链即坏回放
      for (const d of fixture.ticks.slice(1)) {
        if (isDelta(d)) expect(d.upsert.length + d.removedIds.length).toBeGreaterThan(0);
      }
    });
  }

  it('standard 夹具覆盖静态对象全家（R3 主夹具的类型面）', () => {
    const standard = JSON.parse(readFileSync(join(fixturesDir, 'standard.json'), 'utf8')) as FixtureFile;
    // 类型判定：结构类 type:"structure" 以 structureType 为二级类型；
    // 工地 type:"constructionSite"（其 structureType 是目标工程非对象类型）
    const types = new Set(
      standard.ticks[0]!.objects.map((o) => (o.type === 'structure' ? (o.structureType as string) : o.type)),
    );
    for (const expected of [
      'creep',
      'controller',
      'spawn',
      'extension',
      'storage',
      'source',
      'mineral',
      'extractor',
      'road',
      'constructedWall',
      'constructionSite',
      'tower',
      'rampart',
    ]) {
      expect(types.has(expected), `缺 ${expected}`).toBe(true);
    }
  });

  it('swamp-walls 夹具地形含墙与沼泽；dense-edge 夹具有边界墙与出口缺口', () => {
    const swamp = JSON.parse(readFileSync(join(fixturesDir, 'swamp-walls.json'), 'utf8')) as FixtureFile;
    const codes = swamp.ticks[0]!.terrain.codes;
    expect(codes.some((c) => (c & 1) !== 0)).toBe(true);
    expect(codes.some((c) => (c & 2) !== 0)).toBe(true);

    const dense = JSON.parse(readFileSync(join(fixturesDir, 'dense-edge.json'), 'utf8')) as FixtureFile;
    const dc = dense.ticks[0]!.terrain.codes;
    expect(dc[0]).toBe(1); // (0,0) 墙
    expect(dc[24]).toBe(0); // (24,0) top 出口缺口
    expect(dc[25]).toBe(0); // (25,0)
    expect(dc[49 * 50 + 24]).toBe(0); // bottom 出口缺口
  });
});
