import { describe, expect, it } from 'vitest';
import type { PartEnv } from '../src/objects/part-decl.js';
import type { RoomObjectState } from '@kkscreeps/shared';
import {
  PARTS_CREEP,
  PARTS_POWER_CREEP,
  controllerSummary,
  powerCreepTexture,
} from '../src/objects/parts-creeps.js';
import { CREEP_SPEC } from '../src/objects/specs.js';

// creep/powerCreep 值函数：官方 creep.metadata.js / powerCreep.metadata.js
// 计算式逐项断言。

function env(opts: Partial<PartEnv> = {}): PartEnv {
  return {
    cell: 100,
    time: 0,
    player: 'p1',
    isOwn: (u) => u !== null && u === 'p1',
    controller: null,
    ...opts,
  };
}

function run(parts: typeof PARTS_CREEP, id: string, s: RoomObjectState, e: PartEnv): ReturnType<NonNullable<ReturnType<typeof Object>>> | null {
  const part = parts.find((p) => p.id === id);
  if (!part) throw new Error(`无件 ${id}`);
  return part.values(s, e);
}

function creep(fields: Record<string, unknown>): RoomObjectState {
  return { id: 'c1', type: 'creep', x: 10, y: 10, ...fields } as RoomObjectState;
}

describe('PARTS_CREEP.main（creep.metadata.js）', () => {
  it('body 环扁平编码 [color,a1,a2,ccw]×n', () => {
    const v = run(PARTS_CREEP, 'main', creep({ body: [{ type: 'work', hits: 100 }], user: 'p1' }), env())!;
    const ring = v.ring as number[];
    expect(ring).toHaveLength(8); // 一对镜像弧 = 2×4
    expect(ring[0]).toBe(CREEP_SPEC.ring.colors.work);
  });

  it('tough 旗标（任一 tough 存活）', () => {
    const alive = run(PARTS_CREEP, 'main', creep({ body: [{ type: 'tough', hits: 40 }, { type: 'move', hits: 100 }] }), env())!;
    expect(alive.tough).toBe(1);
    const dead = run(PARTS_CREEP, 'main', creep({ body: [{ type: 'tough', hits: 0 }] }), env())!;
    expect(dead.tough).toBe(0);
  });

  it('NPC 旗标（user 2/3，:12）', () => {
    expect(run(PARTS_CREEP, 'main', creep({ user: '2' }), env())!.npc).toBe(1);
    expect(run(PARTS_CREEP, 'main', creep({ user: '3' }), env())!.npc).toBe(1);
    expect(run(PARTS_CREEP, 'main', creep({ user: 'p2' }), env())!.npc).toBe(0);
  });

  it('资源圆半径 = min(1, x/cap)×20（:33-47）', () => {
    const v = run(PARTS_CREEP, 'main', creep({ store: { energy: 50, power: 50 }, storeCapacity: 200 }), env())!;
    expect(v.totalR).toBeCloseTo(20 * (100 / 200));
    expect(v.powerR).toBeCloseTo(20 * (100 / 200)); // power 圆=energy+power（:46）
    expect(v.energyR).toBeCloseTo(20 * (50 / 200));
  });

  it('白圆 when 表达式 || 优先级 quirk（:140——G12）：非能量资源也显示', () => {
    // 官方意图=energy+power<total 才显示白圆，实际解析恒真 → H>0 即显示
    const v = run(PARTS_CREEP, 'main', creep({ store: { H: 500 }, storeCapacity: 1000 }), env())!;
    expect(v.totalR).toBeCloseTo(10);
    expect(v.powerR).toBe(0);
    expect(v.energyR).toBe(0);
  });

  it('storeCapacity=0 → min 钳 1（官方 Infinity 同语义；防御面）', () => {
    const v = run(PARTS_CREEP, 'main', creep({ store: { energy: 30 }, storeCapacity: 0 }), env())!;
    expect(v.totalR).toBeCloseTo(20);
  });

  it('store 缺位 → 三圆全隐', () => {
    const v = run(PARTS_CREEP, 'main', creep({}), env())!;
    expect(v.totalR).toBe(0);
    expect(v.powerR).toBe(0);
    expect(v.energyR).toBe(0);
  });

  it('敌方 safemode α0.5（calculations.safeMode :49-62）', () => {
    const ctl = { user: 'p1', safeModeUntil: 100 };
    const enemy = run(PARTS_CREEP, 'main', creep({ user: 'p2' }), env({ time: 50, controller: ctl }))!;
    expect(enemy.alpha).toBe(CREEP_SPEC.safeModeAlpha);
    // 己方不暗、safemode 过期不暗、无 controller 不暗
    expect(run(PARTS_CREEP, 'main', creep({ user: 'p1' }), env({ time: 50, controller: ctl }))!.alpha).toBe(1);
    expect(run(PARTS_CREEP, 'main', creep({ user: 'p2' }), env({ time: 100, controller: ctl }))!.alpha).toBe(1);
    expect(run(PARTS_CREEP, 'main', creep({ user: 'p2' }), env({ controller: null }))!.alpha).toBe(1);
  });
});

describe('PARTS_CREEP mask/glow/glowNpc/name（layer 件）', () => {
  it('mask：spawning/NPC 隐藏（:186-200）', () => {
    expect(run(PARTS_CREEP, 'mask', creep({ spawning: true, user: 'p1' }), env())).toBeNull();
    expect(run(PARTS_CREEP, 'mask', creep({ user: '2' }), env())).toBeNull();
    expect(run(PARTS_CREEP, 'mask', creep({ user: 'p1' }), env())).toEqual({ alpha: 1 }); // alpha 传导 safemode（评审 A-B3）
  });

  it('glow 400：仅 spawning 隐藏（:214-226 无 NPC 门——NPC 双 glow 并存）', () => {
    expect(run(PARTS_CREEP, 'glow', creep({ spawning: true, user: '2' }), env())).toBeNull();
    expect(run(PARTS_CREEP, 'glow', creep({ user: '2' }), env())).toEqual({ alpha: 1 });
  });

  it('glowNpc：仅 NPC（:201-213）', () => {
    expect(run(PARTS_CREEP, 'glowNpc', creep({ user: '3' }), env())).toEqual({ alpha: 1 });
    expect(run(PARTS_CREEP, 'glowNpc', creep({ user: 'p1' }), env())).toBeNull();
  });

  it('name：己方且非孵化（displayName 模板 + R3 §12 决策⑥）', () => {
    const own = run(PARTS_CREEP, 'name', creep({ name: 'worker', user: 'p1' }), env())!;
    expect(own.text).toBe('worker');
    expect(own.fill).toBe(0x8fbb93);
    expect(run(PARTS_CREEP, 'name', creep({ name: 'x', user: 'p2' }), env())).toBeNull();
    expect(run(PARTS_CREEP, 'name', creep({ name: 'x', user: 'p1', spawning: true }), env())).toBeNull();
  });
});

describe('PARTS_POWER_CREEP（powerCreep.metadata.js）', () => {
  it('纹理档 = className-lvl{min(4,ceil(level/6))}（:41-44）', () => {
    expect(powerCreepTexture('operator', 0)).toBe('operator-lvl0');
    expect(powerCreepTexture('operator', 5)).toBe('operator-lvl1');
    expect(powerCreepTexture('operator', 6)).toBe('operator-lvl1');
    expect(powerCreepTexture('operator', 12)).toBe('operator-lvl2');
    expect(powerCreepTexture('operator', 24)).toBe('operator-lvl4');
    expect(powerCreepTexture('operator', 30)).toBe('operator-lvl4');
    const v = run(PARTS_POWER_CREEP, 'main', { id: 'p1', type: 'powerCreep', className: 'executor', level: 10 } as RoomObjectState, env())!;
    expect(v.texture).toBe('executor-lvl2');
  });

  it('light 件可见键与纹理同源（:138-149 texture 计算——texture-light 死代码）', () => {
    const v = run(PARTS_POWER_CREEP, 'light', { id: 'p1', type: 'powerCreep', className: 'operator', level: 3 } as RoomObjectState, env())!;
    expect(v).toEqual({ texture: 'operator-lvl1', visible: true, alpha: 1 }); // visible 必须是 boolean（vBool === true——评审 B-B1）
    expect(run(PARTS_POWER_CREEP, 'light', { id: 'p1', type: 'powerCreep' } as RoomObjectState, env())).toBeNull();
  });

  it('className 缺位 → main 纹理空（join 面隐藏；官方此路径即崩）', () => {
    const v = run(PARTS_POWER_CREEP, 'main', { id: 'p1', type: 'powerCreep', level: 3 } as RoomObjectState, env())!;
    expect(v.texture).toBe('');
  });

  it('name 无 additionalWhen（:124）', () => {
    expect(run(PARTS_POWER_CREEP, 'name', { id: 'p1', type: 'powerCreep', name: 'pc', user: 'p1' } as RoomObjectState, env())!.text).toBe('pc');
  });
});

describe('controllerSummary（官方 stateExtra.controller=objects.find 首个）', () => {
  it('首个 controller 的 user/safeMode', () => {
    const found = controllerSummary([
      { id: 'c', type: 'creep' },
      { id: 'ctl', type: 'structure', structureType: 'controller', owner: 'p1', safeMode: 55 },
    ]);
    expect(found).toEqual({ user: 'p1', safeModeUntil: 55 });
  });
  it('无 controller → null', () => {
    expect(controllerSummary([{ id: 'c', type: 'creep' }])).toBeNull();
  });
});
