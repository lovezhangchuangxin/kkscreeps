import { beforeEach, describe, expect, it } from 'vitest';
import { Container, Texture } from 'pixi.js';
import { ObjectsStage } from '../src/objects/objects-stage.js';
import type { LightingPass } from '../src/core/lighting.js';
import type { RoomObjectState } from '@kkscreeps/shared';

// R4 运动系统对账行为：快照重建/位移插值/事件罩/死亡淡出（pixi Container
// 数学在 node 可跑——无 GPU 依赖；lighting 用桩）。

function mockLighting(): LightingPass {
  const content = new Container({ label: 'lighting-content' });
  return {
    content,
    addMember: (m: Container) => {
      content.addChild(m);
    },
    removeMember: (m: Container) => {
      content.removeChild(m);
    },
    clearMembers: () => {},
    markDirty: () => {},
    update: () => {},
    destroy: () => {},
    isDegraded: false,
  } as unknown as LightingPass;
}

/** 测试诊断面（views/dying 是实现私有——测试经此窄化访问）。 */
interface StageInternals {
  views: Map<string, { root: Container; parts: Map<string, { node: Container }>; lastRotation: number; cover?: Container }>;
  dying: Map<string, { root: Container }>;
}

function internals(stage: ObjectsStage): StageInternals {
  return stage as unknown as StageInternals;
}

function creep(id: string, x: number, y: number, extra: Record<string, unknown> = {}): RoomObjectState {
  return {
    id,
    type: 'creep',
    x,
    y,
    user: 'p1',
    name: 'c' + id,
    body: [{ type: 'work', hits: 100 }],
    spawning: false,
    ...extra,
  } as RoomObjectState;
}

describe('ObjectsStage 运动/快照/事件/淡出（R4）', () => {
  let stage: ObjectsStage;
  let objectsLayer: Container;

  beforeEach(() => {
    objectsLayer = new Container();
    stage = new ObjectsStage({
      cell: 100,
      lighting: mockLighting(),
      assets: { frames: new Map([['cover', Texture.WHITE]]), glow: null, creepMask: null },
      player: 'p1',
      objectsLayer,
      tickDurationMS: 1000,
    });
  });

  it('增量位移：root 不瞬移，update 推进插值至目标', () => {
    stage.apply([creep('a', 10, 10)], 0, [], 'snapshot');
    stage.apply([creep('a', 11, 10)], 1, [], 'delta');
    const view = internals(stage).views.get('a')!;
    // 起步仍在原位（插值非瞬移）
    expect(view.root.x).toBe(1000);
    // 500ms（半程）→ easeInOutQuad(0.5)=0.5
    stage.update(500);
    expect(view.root.x).toBeCloseTo(1050);
    // 完成 → 吸附目标
    stage.update(500);
    expect(view.root.x).toBe(1100);
  });

  it('快照重建：同 id 不同坐标瞬移到位、不播插值（跨快照跳变不做补间）', () => {
    stage.apply([creep('a', 10, 10)], 0, [], 'snapshot');
    stage.update(100);
    stage.apply([creep('a', 40, 40)], 5, [], 'snapshot');
    const view = internals(stage).views.get('a')!;
    expect(view.root.x).toBe(4000);
    expect(view.root.y).toBe(4000);
    stage.update(1000);
    expect(view.root.x).toBe(4000); // 无残余 tween
  });

  it('朝向：位移变化驱动 main 节点旋转（0.2 tick 线性）', () => {
    stage.apply([creep('a', 10, 10)], 0, [], 'snapshot');
    stage.apply([creep('a', 10, 11)], 1, [], 'delta'); // 向下 → calculateAngle=atan2(1,0)+π/2=π
    const view = internals(stage).views.get('a')!;
    const main = view.parts.get('main')!.node;
    expect(view.lastRotation).toBeCloseTo(Math.PI);
    stage.update(100); // 半程（200ms 的 50%）
    expect(main.rotation).toBeCloseTo(Math.PI / 2);
    stage.update(100);
    expect(main.rotation).toBeCloseTo(Math.PI);
  });

  it('死亡淡出：移除进 dying，α 线性降 0 后销毁', () => {
    stage.apply([creep('a', 10, 10)], 0, [], 'snapshot');
    stage.apply([], 1, [], 'delta');
    expect(internals(stage).views.has('a')).toBe(false);
    const view = internals(stage).dying.get('a')!;
    stage.update(250);
    expect(view.root.alpha).toBeCloseTo(0.5);
    stage.update(250);
    // 自然结束 → 销毁 + 出表
    expect(internals(stage).dying.has('a')).toBe(false);
    expect(view.root.destroyed).toBe(true);
  });

  it('攻击/治疗事件罩：目标视图挂 ADD 罩，同 tick 双事件=黄色', () => {
    stage.apply([creep('a', 10, 10), creep('b', 20, 20)], 0, [], 'snapshot');
    stage.apply([creep('a', 10, 10), creep('b', 20, 20)], 1, [], 'delta');
    stage.update(1);
    stage.apply(
      [creep('a', 10, 10), creep('b', 20, 20)],
      2,
      [
        { event: 1, objectId: 'x', data: { targetId: 'a' } },
        { event: 6, objectId: 'y', data: { targetId: 'a' } },
        { event: 1, objectId: 'z', data: { targetId: 'b' } },
      ],
      'delta',
    );
    const a = internals(stage).views.get('a')!;
    const b = internals(stage).views.get('b')!;
    expect(a.cover).toBeDefined();
    expect(a.cover!.tint).toBe(0xffff33); // 攻+疗=黄
    expect(b.cover).toBeDefined();
    expect(b.cover!.tint).toBe(0xff3333); // 纯攻击=红
    // 罩 α 按关键帧推进：峰值在 0.225td（官方 call-site ×0.9——评审 A-B2）
    stage.update(225);
    expect(a.cover!.alpha).toBeCloseTo(0.5);
    stage.update(675);
    expect(a.cover!.alpha).toBe(0);
  });

  it('powerCreep light 件 join 层冒烟：visible 键为 boolean 且真实可见（评审 B-B1 回归锚）', () => {
    const stage2 = new ObjectsStage({
      cell: 100,
      lighting: mockLighting(),
      assets: { frames: new Map([['cover', Texture.WHITE], ['operator-lvl1', Texture.WHITE]]), glow: Texture.WHITE, creepMask: null },
      player: 'p1',
      objectsLayer: new Container(),
      tickDurationMS: 1000,
    });
    const pc = {
      id: 'pc', type: 'powerCreep', x: 5, y: 5, className: 'operator', level: 8,
      user: 'p2', owner: 'p2', name: 'pc',
    } as unknown as RoomObjectState;
    stage2.apply([pc], 0, [], 'snapshot');
    const view = internals(stage2).views.get('pc')!;
    const light = view.parts.get('light')!.node;
    expect(light.visible).toBe(true); // number 1 会让 vBool 判 false——永不显示
    const glow = view.parts.get('glow')!.node;
    expect(glow.visible).toBe(true);
  });

  it('罩事件对非视图目标（已死对象 id）静默忽略', () => {
    stage.apply([creep('a', 10, 10)], 0, [], 'snapshot');
    expect(() =>
      stage.apply([creep('a', 10, 10)], 1, [{ event: 1, objectId: 'x', data: { targetId: 'ghost' } }], 'delta'),
    ).not.toThrow();
    expect(internals(stage).views.get('a')!.cover).toBeUndefined();
  });

  it('罩事件对非 motion 类型（结构承伤）不挂罩（官方 creepActions 声明域——评审 A-B1 回归锚）', () => {
    // constructedWall：非 motion 结构且不触发 rampart 顶罩烘焙（node 测试无 DOM）
    const wall: RoomObjectState = {
      id: 'w', type: 'structure', structureType: 'constructedWall', x: 30, y: 30, hits: 1000,
    } as unknown as RoomObjectState;
    stage.apply([creep('a', 10, 10), wall], 0, [], 'snapshot');
    stage.apply(
      [creep('a', 10, 10), wall],
      1,
      [{ event: 1, objectId: 'x', data: { targetId: 'w' } }],
      'delta',
    );
    expect(internals(stage).views.get('w')).toBeDefined();
    expect(internals(stage).views.get('w')!.cover).toBeUndefined();
    expect(internals(stage).views.get('a')!.cover).toBeUndefined();
  });

  it('safemode 视图死亡：overlay 自捕捉底 α 乘法淡出，不跳亮（评审 A 二轮回归锚）', () => {
    const enemy = creep('e', 10, 10, { user: 'p2' });
    // controller 对象供 stage 的 env 扫描（官方 stateExtra.controller 同路径）
    const controller: RoomObjectState = {
      id: 'ctl', type: 'structure', structureType: 'controller', x: 40, y: 40,
      owner: 'p1', safeMode: 100,
    } as unknown as RoomObjectState;
    stage.apply([enemy, controller], 0, [], 'snapshot');
    const view = internals(stage).views.get('e')!;
    const glow = view.parts.get('glow')!.node;
    expect(glow.alpha).toBe(0.5); // 敌方 safemode 暗化已传导
    stage.apply([], 1, [], 'delta'); // 死亡
    const dying = internals(stage).dying.get('e')!;
    stage.update(1); // 淡出首帧
    expect(dying.root.alpha).toBeLessThan(1);
    expect(glow.alpha).toBeLessThanOrEqual(0.5 + 1e-9); // 不得跳亮回 1
    stage.update(500);
    expect(glow.alpha).toBe(0);
  });
});
