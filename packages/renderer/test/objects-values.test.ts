import { describe, expect, it } from 'vitest';
import type { PartEnv, PartValues } from '../src/objects/part-decl.js';
import { valuesEqual } from '../src/objects/part-decl.js';
import {
  containerBarHeights,
  decayAlpha,
  mineralColors,
  PARTS_CONTAINER,
  PARTS_DEPOSIT,
  PARTS_RUIN,
  PARTS_SOURCE,
  PARTS_TOMBSTONE,
  resourceCircleValues,
  resourcesTotal,
} from '../src/objects/parts-naturals.js';
import {
  controllerDowngradeOpacity,
  controllerStatusValues,
  extensionTier,
  flagColors,
  isCooldown,
  labMineralLevel,
  nukerTrianglePoints,
  PARTS_CONSTRUCTED_WALL,
  PARTS_CONSTRUCTION_SITE,
  PARTS_CONTROLLER,
  PARTS_EXTENSION,
  PARTS_FACTORY,
  PARTS_INVADER_CORE,
  PARTS_LAB,
  PARTS_NUKER,
  PARTS_RAMPART,
  PARTS_SPAWN,
  PARTS_TERMINAL,
  PARTS_TOWER,
  powerBankRadius,
  powerBankRadius as pbr,
  spawningAngle,
  storageBarHeights,
  terminalRectSizes,
} from '../src/objects/parts-structures.js';
import { roadNodes } from '../src/objects/road-geometry.js';
import { rampartGroupHash, rampartGroups } from '../src/objects/rampart-groups.js';
import {
  CONTROLLER_LEVELS,
  DECAY_SPEC,
  EXTENSION_SPEC,
  MINERAL_SPEC,
  NUKER_SPEC,
} from '../src/objects/specs.js';
import type { RoomObjectState } from '@kkscreeps/shared';

// 规格值函数：官方 metadata 计算式的逐项断言（file:line 见各断言注释）。

function env(time = 0, player: string | null = 'p1'): PartEnv {
  return { cell: 100, time, player, isOwn: (u) => u !== null && u === player, controller: null };
}

function run(parts: { id: string; values: (s: RoomObjectState, e: PartEnv) => PartValues | null }[], id: string, s: RoomObjectState, e: PartEnv): PartValues | null {
  const part = parts.find((p) => p.id === id);
  if (!part) throw new Error(`无此部件 ${id}`);
  return part.values(s, e);
}

function obj(fields: Record<string, unknown>): RoomObjectState {
  return { id: 't', type: 'structure', ...fields } as RoomObjectState;
}

describe('source（source.metadata.js）', () => {
  it('energySize = 60×energy/energyCapacity（:27-33）', () => {
    expect(run(PARTS_SOURCE, 'energyRect', obj({ energy: 2400, energyCapacity: 3000 }), env()))
      .toEqual({ size: 48 });
    expect(run(PARTS_SOURCE, 'energyRect', obj({ energy: 3000, energyCapacity: 3000 }), env()))
      .toEqual({ size: 60 });
    expect(run(PARTS_SOURCE, 'energyRect', obj({ energy: 100, energyCapacity: 0 }), env()))
      .toEqual({ size: 0 });
  });
  it('glowSmall 仅 energy>0（:162-173）', () => {
    expect(run(PARTS_SOURCE, 'glowSmall', obj({ energy: 1 }), env())).toEqual({ visible: true });
    expect(run(PARTS_SOURCE, 'glowSmall', obj({ energy: 0 }), env())).toEqual({ visible: false });
  });
});

describe('mineral（mineral.metadata.js:7-36）', () => {
  it('色表逐项', () => {
    expect(mineralColors('H')).toEqual(MINERAL_SPEC.colors.H);
    expect(mineralColors('K')).toEqual({ foreground: 0x9370ff, background: 0x331a80 });
  });
  it('未知类型退 O 灰对（防御性分歧，官方此处崩）', () => {
    expect(mineralColors('Q')).toEqual(MINERAL_SPEC.colors.O);
  });
});

describe('掉落资源（resourceCircle.js:8-58）', () => {
  it('amount/缺省容量 1250 → r=30×min(1, amount/cap)', () => {
    expect(resourceCircleValues(obj({ type: 'resource', resourceType: 'energy', amount: 300 })))
      .toEqual({ color: 0xffe56d, radius: 30 * (300 / 1250), radiusLighting: Math.round(30 * (300 / 1250)) });
  });
  it('显式容量', () => {
    expect(resourceCircleValues(obj({ type: 'resource', resourceType: 'energy', energy: 500, energyCapacity: 1000 })))
      .toEqual({ color: 0xffe56d, radius: 15, radiusLighting: 15 });
  });
  it('power 红色 r45；未知类型白 r45', () => {
    expect(resourceCircleValues(obj({ type: 'resource', resourceType: 'power', amount: 1250 })).color)
      .toBe(0xf41f33);
    expect(resourceCircleValues(obj({ type: 'resource', resourceType: 'X', amount: 1 })).color)
      .toBe(0xffffff);
  });
});

describe('deposit（deposit.metadata.js:53-58）', () => {
  it('fillAlpha = max(0, 0.6 − harvested/1e5)', () => {
    const v = run(PARTS_DEPOSIT, 'sprites', obj({ type: 'structure', structureType: 'deposit', depositType: 'silicon', harvested: 0 }), env());
    expect(v).toEqual({ color: 0x4ca7e5, fillAlpha: 0.6, kind: 'silicon' });
    const half = run(PARTS_DEPOSIT, 'sprites', obj({ type: 'structure', structureType: 'deposit', depositType: 'silicon', harvested: 30000 }), env());
    expect(half).toEqual({ color: 0x4ca7e5, fillAlpha: 0.3, kind: 'silicon' });
    const done = run(PARTS_DEPOSIT, 'sprites', obj({ type: 'structure', structureType: 'deposit', depositType: 'silicon', harvested: 100000 }), env());
    expect(done).toEqual({ color: 0x4ca7e5, fillAlpha: 0, kind: 'silicon' });
  });
});

describe('container 条高（container.metadata.js:13-26）', () => {
  it('×50/(storeCapacity||resourcesTotal)', () => {
    const b = containerBarHeights(obj({ store: { energy: 100, power: 50, X: 50 }, storeCapacity: 400 }), 50);
    expect(b).toEqual({
      hasOther: true,
      hasPower: true,
      hasEnergy: true,
      otherH: 25,
      powerH: 18.75,
      energyH: 12.5,
    });
  });
  it('无容量时以总量为基准；纯 energy 无白条', () => {
    const b = containerBarHeights(obj({ store: { energy: 100 } }), 50);
    expect(b.hasOther).toBe(false);
    expect(b.energyH).toBe(50);
    expect(b.powerH).toBe(50);
  });
  it('PARTS_CONTAINER.bars 消费同函数', () => {
    const s = obj({ store: { energy: 10 }, storeCapacity: 100 });
    expect(run(PARTS_CONTAINER, 'bars', s, env())).toEqual(containerBarHeights(s, 50));
  });
});

describe('decayAlpha（tombstone.metadata.js:16-19）', () => {
  const s = obj({ type: 'tombstone', deathTime: 0, decayTime: 90 });
  it('线性衰减', () => {
    expect(decayAlpha(s, 0)).toBe(1);
    expect(decayAlpha(s, 45)).toBeCloseTo(0.5);
    expect(decayAlpha(s, 90)).toBe(0);
  });
  it('越界钳制与零分母', () => {
    expect(decayAlpha(s, 900)).toBe(0);
    expect(decayAlpha(obj({ type: 'tombstone', deathTime: 5, decayTime: 5 }), 5)).toBe(0);
  });
  it('ruin 同构（start 键=destroyTime，官方键名即不同）', () => {
    const v = run(PARTS_RUIN, 'sprites', { ...s, type: 'ruin', user: 'p1', store: { energy: 1 } }, env(45));
    expect(v!.borderAlpha).toBeCloseTo(0.5);
    expect(v!.resourceTint).toBe(0x8fbb93); // own p1（resourcesTotal>0）
  });
  it('空仓 tint 黑（tombstone.metadata.js:11-14）', () => {
    const v = run(PARTS_TOMBSTONE, 'sprites', obj({ type: 'tombstone', deathTime: 0, decayTime: 90, store: {} }), env(0));
    expect(v!.resourceTint).toBe(DECAY_SPEC.emptyColor);
  });
});

describe('resourcesTotal（resourceTotal.js）', () => {
  it('Σstore', () => {
    expect(resourcesTotal({ energy: 100, power: 50, H: 3 })).toBe(153);
    expect(resourcesTotal({})).toBe(0);
  });
});

// ---- 结构家族 ----

describe('extension（extension.metadata.js）', () => {
  it('容量档位：≥200 large / ≥100 medium / 其余 small（:14-24）', () => {
    expect(extensionTier(200)).toBe('large');
    expect(extensionTier(600)).toBe('large');
    expect(extensionTier(100)).toBe('medium');
    expect(extensionTier(50)).toBe('small');
    expect(extensionTier(0)).toBe('small');
  });
  it('border 部件给 tier/border 档+玩家色（border 精确相等判定，A-S5）', () => {
    expect(run(PARTS_EXTENSION, 'border', obj({ structureType: 'extension', storeCapacityResource: { energy: 50 }, user: 'p1' }), env()))
      .toEqual({ tier: 'small', border: 'small', tint: 0x8fbb93 });
    // 无匹配档（150）→ border 空=官方三 when 全不命中
    expect(run(PARTS_EXTENSION, 'border', obj({ structureType: 'extension', storeCapacityResource: { energy: 150 }, user: 'p1' }), env()))
      .toEqual({ tier: 'medium', border: '', tint: 0x8fbb93 });
  });
  it('circle scale = min(1, energy/cap)（:27-31）；半径随档位 size×0.32', () => {
    expect(run(PARTS_EXTENSION, 'circle', obj({ store: { energy: 25 }, storeCapacityResource: { energy: 50 } }), env()))
      .toEqual({ scale: 0.5, radius: EXTENSION_SPEC.sizes.small * EXTENSION_SPEC.circleFactor });
    expect(run(PARTS_EXTENSION, 'circle', obj({ store: { energy: 99 }, storeCapacityResource: { energy: 50 } }), env()))
      .toEqual({ scale: 1, radius: EXTENSION_SPEC.sizes.small * EXTENSION_SPEC.circleFactor });
  });
  it('tier glow：cap 匹配且 energy>0 才显示（:119-157）', () => {
    expect(run(PARTS_EXTENSION, 'glowTier', obj({ store: { energy: 1 }, storeCapacityResource: { energy: 50 } }), env()))
      .toEqual({ width: 200, height: 200, alpha: 0.7 });
    expect(run(PARTS_EXTENSION, 'glowTier', obj({ store: { energy: 0 }, storeCapacityResource: { energy: 50 } }), env()))
      .toBeNull();
    expect(run(PARTS_EXTENSION, 'glowTier', obj({ store: { energy: 1 }, storeCapacityResource: { energy: 75 } }), env()))
      .toBeNull();
    expect(EXTENSION_SPEC.glowTiers[2]).toEqual({ cap: 200, width: 250, height: 250, alpha: 0.7 });
  });
});

describe('storage 条高（storage.metadata.js:17-31）', () => {
  it('分母 = max(storeCapacity||1e6, resourcesTotal)', () => {
    const b = storageBarHeights(obj({ store: { energy: 12000, G: 30 }, storeCapacity: 1000000 }));
    expect(b.energyH).toBeCloseTo((12000 * 140) / 1000000);
    expect(b.otherH).toBeCloseTo((12030 * 140) / 1000000);
    expect(b.powerH).toBeCloseTo((12000 * 140) / 1000000);
    expect(b.hasOther).toBe(true);
    expect(b.hasPower).toBe(false);
    expect(b.hasEnergy).toBe(true);
  });
  it('容量缺省 1e6', () => {
    const b = storageBarHeights(obj({ store: { energy: 500000 } }));
    expect(b.energyH).toBeCloseTo(70);
  });
});

describe('factory（factory.metadata.js）', () => {
  it('powerHeight 优先级 bug 保真（:44-47 → power || (0+energy) || 0）', () => {
    const bars = run(PARTS_FACTORY, 'bars', obj({ store: { energy: 100, power: 5 }, storeCapacity: 1000 }), env())!;
    // power=5 → powerHeight 基 5（非 105）
    expect(bars.powerH).toBeCloseTo((5 * 50) / 1000);
    // hasOther quirk（B4：energy||power||(0<total) ⇒ total>0 恒 true）
    expect(bars.hasOther).toBe(true);
    const noPower = run(PARTS_FACTORY, 'bars', obj({ store: { energy: 100 }, storeCapacity: 1000 }), env())!;
    // power=0 → || 链落到 energy=100
    expect(noPower.powerH).toBeCloseTo((100 * 50) / 1000);
  });
  it('highlight：cooldown 闪 1，常态 0.4（:75-116）', () => {
    expect(run(PARTS_FACTORY, 'highlight', obj({ cooldownTime: 100 }), env(50))).toEqual({ alpha: 1 });
    expect(run(PARTS_FACTORY, 'highlight', obj({ cooldownTime: 100 }), env(100))).toEqual({ alpha: 1 });
    expect(run(PARTS_FACTORY, 'highlight', obj({}), env(100))).toEqual({ alpha: 0.4 });
  });
  it('level>0 才有 level 件（:126-136）', () => {
    expect(run(PARTS_FACTORY, 'level', obj({ level: 0 }), env())).toBeNull();
    expect(run(PARTS_FACTORY, 'level', obj({ level: 2 }), env())).toEqual({ level: 2, tint: 0xffffff });
  });
});

describe('nuker（nuker.metadata.js:22-44）', () => {
  it('三角顶点按能量比例收缩', () => {
    const pts = nukerTrianglePoints({ energy: 50 }, 100);
    const t = NUKER_SPEC.triangle;
    expect(pts).toEqual([t.x - t.width * 0.25, t.y, t.x, t.y - t.height * 0.5, t.x + t.width * 0.25, t.y]);
  });
  it('gRect 分母=storeCapacityResource.G', () => {
    expect(run(PARTS_NUKER, 'gRect', obj({ store: { energy: 0, G: 30 }, storeCapacityResource: { energy: 100, G: 60 } }), env()))
      .toEqual({ width: NUKER_SPEC.gRect.width * 0.5 });
    expect(run(PARTS_NUKER, 'gRect', obj({ store: { G: 30 } }), env())).toEqual({ width: 0 });
  });
  it('三角部件消费同函数', () => {
    const v = run(PARTS_NUKER, 'triangle', obj({ store: { energy: 100 }, storeCapacityResource: { energy: 200 } }), env())!;
    expect(v.points).toEqual(nukerTrianglePoints({ energy: 100 }, 200));
  });
});

describe('spawn（spawn.metadata.js）', () => {
  it('spawningAngle（:31-39）：−π/2 + 2π(need−remaining)/need', () => {
    expect(spawningAngle({ needTime: 10, spawnTime: 10 }, 0)).toBe(-Math.PI / 2);
    expect(spawningAngle({ needTime: 10, spawnTime: 10 }, 5)).toBeCloseTo(-Math.PI / 2 + Math.PI);
    expect(spawningAngle({ needTime: 10, spawnTime: 10 }, 10)).toBeCloseTo(-Math.PI / 2 + 2 * Math.PI);
    expect(spawningAngle(undefined, 5)).toBe(-Math.PI / 2);
    expect(spawningAngle({ needTime: 0, spawnTime: 10 }, 5)).toBe(-Math.PI / 2);
  });
  it('resourceScale 无 min 钳制（:41-46 保真）', () => {
    expect(run(PARTS_SPAWN, 'core', obj({ type: 'spawn', store: { energy: 180 }, storeCapacityResource: { energy: 300 } }), env()))
      .toEqual({ badgeColor: 0x555555, resourceScale: 0.6, arcAngle: -Math.PI / 2 });
    expect(run(PARTS_SPAWN, 'core', obj({ type: 'spawn', store: { energy: 900 }, storeCapacityResource: { energy: 300 } }), env()))
      .toEqual({ badgeColor: 0x555555, resourceScale: 3, arcAngle: -Math.PI / 2 });
  });
  it('name 仅己方显示', () => {
    expect(run(PARTS_SPAWN, 'name', obj({ type: 'spawn', name: 'Spawn1', user: 'p1' }), env()))
      .toEqual({ text: 'Spawn1', fill: 0x8fbb93 });
    expect(run(PARTS_SPAWN, 'name', obj({ type: 'spawn', name: 'Spawn1', user: 'p2' }), env()))
      .toBeNull();
  });
});

describe('tower（tower.metadata.js:37-50）', () => {
  it('rotatable 合并件：bar height=min(66.7, 66.7×energy/cap)，radius=min(12, h/2)', () => {
    expect(run(PARTS_TOWER, 'rotatable', obj({ structureType: 'tower', store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }), env()))
      .toEqual({ npc: false, height: 33.35, radius: 12 });
    expect(run(PARTS_TOWER, 'rotatable', obj({ structureType: 'tower', store: { energy: 10 }, storeCapacityResource: { energy: 1000 } }), env()))
      .toEqual({ npc: false, height: 0.667, radius: 0.3335 });
  });
  it('NPC（user 2/3）用 npc 贴图标记', () => {
    // NPC 塔无能量条（A-B6：官方 when: isNotNpc）
    expect(run(PARTS_TOWER, 'rotatable', obj({ structureType: 'tower', user: '2', store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }), env()))
      .toEqual({ npc: true, height: 0, radius: 0 });
    expect(run(PARTS_TOWER, 'rotatable', obj({ structureType: 'tower', user: 'p1' }), env()))
      .toMatchObject({ npc: false });
  });
});

describe('lab（lab.metadata.js:36-57）', () => {
  it('mineral scale = 0.6875×amount/capacity（容量=storeCapacityResource[type]||storeCapacity−Σcap）', () => {
    expect(labMineralLevel(obj({ structureType: 'lab', store: { energy: 100, H: 2500 }, storeCapacity: 5000, storeCapacityResource: { energy: 2000, H: 5000 } })))
      .toEqual({ type: 'H', scale: 0.6875 * 0.5 });
    expect(labMineralLevel(obj({ structureType: 'lab', store: { energy: 100 }, storeCapacity: 5000, storeCapacityResource: { energy: 2000, H: 5000 } })))
      .toEqual({ type: '', scale: 0 });
  });
  it('energyRect width = 67×energy/cap（:52-57）', () => {
    expect(run(PARTS_LAB, 'energyRect', obj({ structureType: 'lab', store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }), env()))
      .toEqual({ width: 33.5 });
  });
  it('glow 条件 = 非 energy 资源在场（:141-165）', () => {
    expect(run(PARTS_LAB, 'glow', obj({ structureType: 'lab', store: { energy: 500, H: 10 }, storeCapacityResource: { energy: 1000 } }), env()))
      .toEqual({ visible: true });
    expect(run(PARTS_LAB, 'glow', obj({ structureType: 'lab', store: { energy: 500 }, storeCapacityResource: { energy: 1000 } }), env()))
      .toEqual({ visible: false });
  });
});

describe('terminal（terminal.metadata.js:15-46）', () => {
  it('三方块尺寸 ×76/(cap||3000) cap76；rest 仅在有他资源时', () => {
    expect(terminalRectSizes(obj({ structureType: 'terminal', store: { energy: 1500 }, storeCapacity: 3000 })))
      .toEqual({ restSize: 0, powerSize: 38, energySize: 38 });
    // power 尺寸=energy+power 覆盖（官方 :32-38；store 无 power 键即 0）
    expect(terminalRectSizes(obj({ structureType: 'terminal', store: { energy: 1000, H: 500 }, storeCapacity: 3000 })))
      .toEqual({ restSize: 38, powerSize: 76 * 1000 / 3000, energySize: 76 * 1000 / 3000 });
  });
  it('容量缺省=TERMINAL_CAPACITY 300000（@screeps/common constants.js:332，清偿复审锚）', () => {
    // 无 storeCapacity：300000 满仓 1500 能量 → 76×1500/300000=0.38
    expect(terminalRectSizes(obj({ structureType: 'terminal', store: { energy: 1500 } })))
      .toEqual({ restSize: 0, powerSize: 76 * 1500 / 300000, energySize: 76 * 1500 / 300000 });
  });
  it('arrows/highlight 随 cooldown（>= 判定，:48-51,74-117）', () => {
    expect(run(PARTS_TERMINAL, 'arrows', obj({ structureType: 'terminal', cooldownTime: 100 }), env(100)))
      .toEqual({ alpha: 0.1 });
    expect(run(PARTS_TERMINAL, 'arrows', obj({ structureType: 'terminal' }), env(100)))
      .toEqual({ alpha: 1 });
    expect(run(PARTS_TERMINAL, 'highlight', obj({ structureType: 'terminal', cooldownTime: 100 }), env(100)))
      .toEqual({ alpha: 1 });
    expect(run(PARTS_TERMINAL, 'highlight', obj({ structureType: 'terminal' }), env(100)))
      .toEqual({ alpha: 0 });
  });
});

describe('controller（controller.metadata.js）', () => {
  it('progressTotal = CONTROLLER_LEVELS[level]（:64-70；夹具 level5→1215000 佐证）', () => {
    expect(CONTROLLER_LEVELS[5]).toBe(1215000);
    const v = run(PARTS_CONTROLLER, 'progress', obj({ structureType: 'controller', level: 5, progress: 1200 }), env())!;
    expect(v.angle).toBeCloseTo(2 * Math.PI * (1200 / 1215000));
    expect(run(PARTS_CONTROLLER, 'progress', obj({ structureType: 'controller', level: 5, progress: 0 }), env())).toBeNull();
  });
  it('downgradeOpacity（:53-63；!gameTime 守卫）', () => {
    // CONTROLLER_DOWNGRADE[5]=80000
    expect(controllerDowngradeOpacity(obj({ structureType: 'controller', level: 5, downgradeTime: 80000 }), 0)).toBe(0);
    expect(controllerDowngradeOpacity(obj({ structureType: 'controller', level: 5, downgradeTime: 80000 }), 40000))
      .toBeCloseTo(0.5);
    expect(controllerDowngradeOpacity(obj({ structureType: 'controller', level: 0, downgradeTime: 80000 }), 100)).toBe(0);
  });
  it('状态圈：own reservation 绿；enemy/upgradeBlocked 红；safemode 橙（:74-128）', () => {
    const own = controllerStatusValues(obj({ structureType: 'controller', reservation: { user: 'p1', ticksToEnd: 100 } }), env());
    expect(own).toEqual({ own: true, enemy: false, safe: false });
    const enemyRes = controllerStatusValues(obj({ structureType: 'controller', reservation: { user: 'p2' } }), env());
    expect(enemyRes.own).toBe(false);
    expect(enemyRes.enemy).toBe(true);
    const blocked = controllerStatusValues(obj({ structureType: 'controller', upgradeBlocked: 100 }), env(50));
    expect(blocked.enemy).toBe(true);
    const safe = controllerStatusValues(obj({ structureType: 'controller', safeMode: 100 }), env(50));
    expect(safe.safe).toBe(true);
  });
});

describe('constructedWall 红闪条件（constructedWall.metadata.js:22）', () => {
  it('hits===1 && hitsMax>1', () => {
    expect(run(PARTS_CONSTRUCTED_WALL, 'flash', obj({ structureType: 'constructedWall', hits: 1, hitsMax: 1000 }), env()))
      .toEqual({ visible: true, alpha: 0.2 });
    expect(run(PARTS_CONSTRUCTED_WALL, 'flash', obj({ structureType: 'constructedWall', hits: 2, hitsMax: 1000 }), env())!.visible)
      .toBe(false);
  });
});

describe('constructionSite（constructionSite.metadata.js:11-23）', () => {
  it('angle = 2π×progress/progressTotal + 玩家色 + 冻结闪烁相位', () => {
    const v = run(PARTS_CONSTRUCTION_SITE, 'progress', obj({ type: 'constructionSite', structureType: 'extension', progress: 200, progressTotal: 3000, owner: 'p1' }), env())!;
    expect(v.angle).toBeCloseTo(2 * Math.PI * (200 / 3000));
    expect(v.color).toBe(0x8fbb93);
    expect(v.alpha).toBe(0.8);
  });
});

describe('rampart 公有件（rampart.metadata.js:10-23）', () => {
  it('isPublic 才显示；色=playerColor（A-B5：userOwnerColor 分支）', () => {
    expect(run(PARTS_RAMPART, 'public', obj({ structureType: 'rampart', isPublic: false, user: 'p1' }), env())).toBeNull();
    expect(run(PARTS_RAMPART, 'public', obj({ structureType: 'rampart', isPublic: true, user: 'p1' }), env()))
      .toEqual({ tint: 0x8fbb93 });
    expect(run(PARTS_RAMPART, 'public', obj({ structureType: 'rampart', isPublic: true, user: 'p2' }), env()))
      .toEqual({ tint: 0xff7777 });
  });
});

describe('powerBank（powerBank.metadata.js:16-22）', () => {
  it('r = sqrt((power/5000×3000)/π)', () => {
    expect(powerBankRadius(5000)).toBeCloseTo(Math.sqrt(3000 / Math.PI));
    expect(powerBankRadius(0)).toBe(0);
    expect(pbr(2500)).toBeCloseTo(Math.sqrt((0.5 * 3000) / Math.PI));
  });
});

describe('invaderCore（invaderCore.metadata.js:21-25）', () => {
  it('ttl 百位取整 + 进度角（decayTicks=75000，B2 修正）', () => {
    const v = run(PARTS_INVADER_CORE, 'progress', obj({ structureType: 'invaderCore', decayTime: 75450 }), env(300))!;
    expect(v.angle).toBeCloseTo(2 * Math.PI);
    const half = run(PARTS_INVADER_CORE, 'progress', obj({ structureType: 'invaderCore', decayTime: 37800 }), env(300))!;
    expect(half.angle).toBeCloseTo(2 * Math.PI * (37500 / 75000));
  });
  it('ttl≤0 无扇形（siteProgress.js:38 progress>0 才画）', () => {
    expect(run(PARTS_INVADER_CORE, 'progress', obj({ structureType: 'invaderCore', decayTime: 250 }), env(300)))
      .toBeNull();
  });
});

describe('flag（flag.metadata.js:3-14）', () => {
  it('10 色表', () => {
    expect(flagColors(obj({ type: 'flag', color: 1, secondaryColor: 5 })))
      .toEqual({ primary: 0xf44336, secondary: 0x4caf50 });
    expect(flagColors(obj({ type: 'flag', color: 99 })).primary).toBeNull();
  });
});

describe('cooldown 判定（>= 严格；lab 用 >）', () => {
  it('isCooldown 边界', () => {
    expect(isCooldown(obj({ cooldownTime: 100 }), 100)).toBe(true);
    expect(isCooldown(obj({ cooldownTime: 100 }), 101)).toBe(false);
    expect(isCooldown(obj({ cooldownTime: 100 }), 100, true)).toBe(false);
    expect(isCooldown(obj({}), 100)).toBe(false);
  });
});

// ---- road / rampart 顶罩 ----

describe('roadNodes（road.js:65-68）', () => {
  function roads(...coords: Array<[number, number]>): RoomObjectState[] {
    return coords.map(([x, y]) => obj({ type: 'structure', structureType: 'road', x, y }));
  }
  it('孤立路四向皆断', () => {
    expect(roadNodes(roads([5, 5]))).toEqual([{ x: 5, y: 5, mm: false, zm: false, pm: false, mz: false }]);
  });
  it('L 形：右与下（官方判定集只看上/左三向+左）', () => {
    // (5,5) (6,5)：官方 mm/zm/pm/mz 判定——(6,5) 的 mz=(5,5) 命中
    const nodes = roadNodes(roads([5, 5], [6, 5]));
    const right = nodes.find((n) => n.x === 6)!;
    expect(right.mz).toBe(true);
    expect(right.mm).toBe(false);
    expect(right.zm).toBe(false);
    expect(right.pm).toBe(false);
  });
  it('上邻接 zm；左上对角 mm；右上对角 pm', () => {
    const nodes = roadNodes(roads([5, 5], [5, 4], [4, 4], [6, 4]));
    const center = nodes.find((n) => n.x === 5 && n.y === 5)!;
    expect(center).toEqual({ x: 5, y: 5, mm: true, zm: true, pm: true, mz: false });
  });
});

describe('rampartGroups（terrain.js:183-247）', () => {
  it('按 user 分组，!isPublic 才入组，格索引行主序升序', () => {
    const groups = rampartGroups([
      obj({ structureType: 'rampart', user: 'p1', x: 3, y: 2 }),
      obj({ structureType: 'rampart', user: 'p1', x: 1, y: 1 }),
      obj({ structureType: 'rampart', user: 'p2', x: 5, y: 5 }),
      obj({ structureType: 'rampart', user: 'p2', x: 5, y: 5, isPublic: true }),
      obj({ structureType: 'tower', user: 'p1', x: 9, y: 9 }),
    ], { isOwn: (u) => u === 'p1' });
    expect(groups).toHaveLength(2);
    // (3,2)→2×50+3=103；(1,1)→1×50+1=51
    expect(groups[0]).toMatchObject({ user: 'p1', cells: [51, 103] });
    expect(groups[0]!.fill).toBe(0x105010);
    expect(groups[1]).toMatchObject({ user: 'p2', cells: [255] });
    expect(groups[1]!.stroke).toBe(0xff4444);
  });
  it('hash 随格集/颜色变化', () => {
    const a = rampartGroupHash({ user: 'p1', cells: [1, 2], fill: 1, stroke: 2 });
    const b = rampartGroupHash({ user: 'p1', cells: [1, 3], fill: 1, stroke: 2 });
    const c = rampartGroupHash({ user: 'p1', cells: [1, 2], fill: 3, stroke: 2 });
    expect(a).not.toBe(b);
    expect(a).not.toBe(c);
    expect(a).toBe(rampartGroupHash({ user: 'p1', cells: [1, 2], fill: 1, stroke: 2 }));
  });
});

// ---- 差量门控 ----

describe('valuesEqual（值相等门控）', () => {
  it('null 对等与不对等', () => {
    expect(valuesEqual(null, null)).toBe(true);
    expect(valuesEqual(null, {})).toBe(false);
    expect(valuesEqual({}, null)).toBe(false);
  });
  it('原始值逐键比较，键集不同即不等', () => {
    expect(valuesEqual({ a: 1, b: 'x' }, { b: 'x', a: 1 })).toBe(true);
    expect(valuesEqual({ a: 1 }, { a: 2 })).toBe(false);
    expect(valuesEqual({ a: 1 }, { a: 1, b: 2 })).toBe(false);
  });
  it('数组逐元素', () => {
    expect(valuesEqual({ p: [1, 2, 3] }, { p: [1, 2, 3] })).toBe(true);
    expect(valuesEqual({ p: [1, 2, 3] }, { p: [1, 2, 4] })).toBe(false);
    expect(valuesEqual({ p: [1, 2] }, { p: [1, 2, 3] })).toBe(false);
  });
});
