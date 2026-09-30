import { readFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { isDelta, isRoomVisual, isSnapshot, type FixtureFile, type RoomDelta } from '@kkscreeps/shared';

// 入库夹具的契约一致性：真实与合成夹具形状、时序、单调时间轴、地形常量长度。
// visual-cases.json 是独立夹具族（R6 G1 合成用例集，非 FixtureFile——专属校验）。

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixturesDir = join(pkgRoot, 'fixtures');

const names = readdirSync(fixturesDir).filter((f) => f.endsWith('.json')).sort();
const replayNames = names.filter((n) => n !== 'visual-cases.json');

/** exporter 产物（R1 纪律：31 tick 全量入库）；showcase 为 R3 手工合成夹具。 */
const EXPORTER_FIXTURES = ['actions.json', 'creeps.json', 'dense-edge.json', 'plains.json', 'standard.json', 'swamp-walls.json'];

describe('入库夹具', () => {
  it('六张真实夹具、五张合成回放夹具与 visual 用例集齐全', () => {
    expect(names).toEqual([...EXPORTER_FIXTURES, 'showcase.json', 'effects.json', 'lifecycle.json', 'pressure.json', 'perf-full-room.json', 'visual-cases.json'].sort());
  });

  for (const name of replayNames) {
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
        if (isDelta(d)) expect(d.upsert.length + d.removedIds.length + (d.events?.length ?? 0) + (d.actionLogs?.length ?? 0) + (d.says?.length ?? 0)).toBeGreaterThan(0);
      }
    });
  }

  it('visual-cases.json：G1 用例集——五原语×默认值/样式矩阵，origin 声明合成', () => {
    const f = JSON.parse(readFileSync(join(fixturesDir, 'visual-cases.json'), 'utf8')) as {
      room: string;
      origin: string;
      visuals: unknown[];
    };
    expect(f.origin).toContain('合成');
    expect(f.room).toBe('W8N8');
    expect(f.visuals.length).toBe(53);
    expect(f.visuals.every((v) => isRoomVisual(v))).toBe(true);
    const typed = f.visuals as Array<{ type: string; style?: unknown; x: number; y: number }>;
    // 五原语齐全，且每原语的「官方全缺省」样例在集
    for (const t of ['line', 'rect', 'circle', 'poly', 'text']) {
      expect(typed.some((v) => v.type === t), `缺原语 ${t}`).toBe(true);
      expect(typed.some((v) => v.type === t && v.style === undefined), `缺 ${t} 默认样例`).toBe(true);
    }
    const styles = typed.map((v) => v.style ?? {}) as Array<Record<string, unknown>>;
    expect(styles.some((s) => s.lineStyle === 'dashed')).toBe(true);
    expect(styles.some((s) => s.lineStyle === 'dotted')).toBe(true);
    expect(styles.some((s) => s.fill === 'transparent')).toBe(true);
    // 官方 any web format：命名色在集（GPT-P2#2 清偿）
    expect(styles.some((s) => s.color === 'green')).toBe(true);
    expect(styles.some((s) => s.color === 'yellow')).toBe(true);
    expect(styles.some((s) => s.fill === 'magenta')).toBe(true);
    // text font 官方分形态（number 与 CSS 串）
    expect(styles.some((s) => typeof s.font === 'number')).toBe(true);
    expect(styles.some((s) => typeof s.font === 'string' && String(s.font).includes('serif'))).toBe(true);
    // 线型退化回归锚（评审 A-B1）：形状原语必须有用例覆盖默认 strokeWidth 的
    // dashed/dotted（否则 dash 宽度联动被假阴性）
    expect(styles.some((s) => s.lineStyle === 'dotted' && s.strokeWidth === undefined)).toBe(true);
    expect(styles.some((s) => s.lineStyle === 'dashed' && s.strokeWidth === undefined)).toBe(true);
    // α 通道与小字型变体（评审 A-J3 矩阵增量；font 串形态）
    expect(styles.some((s) => typeof s.color === 'string' && (s.color as string).length === 9)).toBe(true);
    expect(styles.some((s) => typeof s.font === 'string' && String(s.font).includes('small-caps'))).toBe(true);
    // 浮点相位（单格框 idiom：x/y 带半格偏移）
    expect(typed.some((v) => v.type === 'rect' && !Number.isInteger(v.x))).toBe(true);
  });

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

  it('showcase 夹具覆盖 creep 形态谱系与 powerCreep（R4）', () => {
    const showcase = JSON.parse(readFileSync(join(fixturesDir, 'showcase.json'), 'utf8')) as FixtureFile;
    const objects = showcase.ticks[0]!.objects;
    const creeps = objects.filter((o) => o.type === 'creep');
    expect(creeps.length).toBeGreaterThanOrEqual(12);
    const names = new Set(creeps.map((o) => o.name));
    // body 环形态：50 部件满环×2/混色/掉血/tough 存活+tough 全灭/NPC/孵化
    for (const expected of ['ring-work50', 'ring-move50', 'ring-mixed', 'ring-damaged', 'ring-tough', 'ring-tough-dead', 'npc-keeper', 'spawner-creep']) {
      expect(names.has(expected), `缺 creep ${expected}`).toBe(true);
    }
    // 资源圆三层 + NPC + powerCreep 双方
    expect(creeps.some((o) => Array.isArray(o.store) === false && typeof o.storeCapacity === 'number')).toBe(true);
    const powerCreeps = objects.filter((o) => o.type === 'powerCreep');
    expect(powerCreeps.map((o) => o.className).sort()).toEqual(['executor', 'operator']);
    // safemode 联动：p1 controller safeMode>0（p2 creep 变暗 + 状态圈转橙）
    const controllers = objects.filter((o) => o.structureType === 'controller');
    expect(controllers.some((c) => typeof c.safeMode === 'number' && c.safeMode > 0)).toBe(true);
  });

  it('creeps 夹具覆盖运动/战斗事件面（R4 实弹夹具）', () => {
    const creeps = JSON.parse(readFileSync(join(fixturesDir, 'creeps.json'), 'utf8')) as FixtureFile;
    const deltas = creeps.ticks.slice(1).filter((t): t is RoomDelta => !isSnapshot(t));
    // 走位：部分增量 upsert creep 且坐标 ≠ 快照同 id 坐标
    const snapshot = creeps.ticks[0];
    if (isSnapshot(snapshot)) {
      const moved = deltas.some((d) => d.upsert.some((o) => {
        const before = snapshot.objects.find((s) => s.id === o.id);
        return before && o.type === 'creep' && (o.x !== before.x || o.y !== before.y);
      }));
      expect(moved).toBe(true);
    }
    // 战斗/治疗事件面（罩 + 死亡淡出的数据依据）
    const events = deltas.flatMap((d) => d.events ?? []);
    expect(events.some((e) => e.event === 1)).toBe(true); // EVENT_ATTACK
    expect(events.some((e) => e.event === 6)).toBe(true); // EVENT_HEAL
    expect(deltas.some((d) => d.removedIds.length > 0)).toBe(true); // 死亡
  });
  it('actions is real engine output: ranged heal, work, melee and completed-site coordinates', () => {
    const f = JSON.parse(readFileSync(join(fixturesDir, 'actions.json'), 'utf8')) as FixtureFile;
    const first = f.ticks[1]!;
    const events = (first as RoomDelta).events!;
    expect(new Set(events.map(e => e.event))).toEqual(new Set([1, 4, 5, 6, 7, 9]));
    expect(events.find(e => e.event === 6)?.data).toMatchObject({ healType: 2 });
    const build = events.find(e => e.event === 4)!.data as { targetId: string; x: number; y: number; incomplete: boolean };
    expect(build).toMatchObject({ x: 14, y: 17, incomplete: false });
    expect((first as RoomDelta).removedIds).toContain(build.targetId);
  });
  it('synthetic galleries declare provenance and pressure has exactly 50 creeps', () => {
    for (const name of ['effects', 'lifecycle', 'pressure']) {
      const f = JSON.parse(readFileSync(join(fixturesDir, `${name}.json`), 'utf8'));
      expect(f.provenance).toContain('synthetic');
      if (name === 'pressure') expect(f.ticks[0].objects).toHaveLength(50);
    }
  });
});
