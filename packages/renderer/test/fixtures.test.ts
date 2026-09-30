import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { isDelta, isSnapshot, type FixtureFile } from '@kkscreeps/shared';

// 入库夹具的契约一致性：四张夹具形状、时序、单调时间轴、地形常量长度。

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixturesDir = join(pkgRoot, 'fixtures');

const names = readdirSync(fixturesDir).filter((f) => f.endsWith('.json')).sort();

/** exporter 产物（R1 纪律：31 tick 全量入库）；showcase 为 R3 手工合成夹具。 */
const EXPORTER_FIXTURES = ['dense-edge.json', 'plains.json', 'standard.json', 'swamp-walls.json'];

describe('入库夹具', () => {
  it('exporter 四张夹具齐全（plains/standard/swamp-walls/dense-edge）+ showcase 合成', () => {
    expect(names).toEqual([...EXPORTER_FIXTURES, 'showcase.json'].sort());
  });

  for (const name of names) {
    it(`${name}：首快照 + 增量链，时间单调，地形 2500 码`, () => {
      const fixture = JSON.parse(readFileSync(join(fixturesDir, name), 'utf8')) as FixtureFile;
      expect(isSnapshot(fixture.ticks[0])).toBe(true);
      expect(fixture.ticks.slice(1).every((t) => isDelta(t))).toBe(true);
      if (EXPORTER_FIXTURES.includes(name)) {
        expect(fixture.ticks).toHaveLength(31); // 1 快照 + 30 增量（exporter 缺省 ticks）
      }

      let prevTime = -1;
      for (const entry of fixture.ticks) {
        expect(entry.time).toBeGreaterThan(prevTime);
        prevTime = entry.time;
        expect(entry.room).toBe(fixture.room);
      }
      expect(fixture.ticks[0]!.terrain.codes).toHaveLength(2500);
      // 每个增量至少动一处（aging/decay/spawning 的自然差量）——空链即坏回放
      // （showcase 的增量是合成值变化，同纪律适用）
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

  it('showcase 夹具覆盖 G5 合成家族与顶罩/双色取材', () => {
    const showcase = JSON.parse(readFileSync(join(fixturesDir, 'showcase.json'), 'utf8')) as FixtureFile;
    const types = new Set(
      showcase.ticks[0]!.objects.map((o) => (o.type === 'structure' ? (o.structureType as string) : o.type)),
    );
    for (const expected of [
      'container', 'link', 'lab', 'terminal', 'factory', 'nuker', 'observer',
      'powerBank', 'powerSpawn', 'portal', 'keeperLair', 'invaderCore',
      'deposit', 'rampart', 'flag', 'constructionSite', 'controller', 'spawn',
      'tombstone', 'ruin', 'resource', 'extractor', 'source', 'mineral', 'tower',
    ]) {
      expect(types.has(expected), `缺 ${expected}`).toBe(true);
    }
    // 双私有顶罩组（p1/p2）+ 公有 rampart + NPC 塔 + 敌我 spawn
    const ramparts = showcase.ticks[0]!.objects.filter((o) => o.structureType === 'rampart');
    expect(ramparts.some((o) => o.isPublic === true)).toBe(true);
    expect(new Set(ramparts.filter((o) => o.isPublic !== true).map((o) => o.user)).size).toBe(2);
    expect(types.has('tower'));
    const towers = showcase.ticks[0]!.objects.filter((o) => o.structureType === 'tower');
    expect(towers.some((o) => o.user === '3')).toBe(true);
  });
});
