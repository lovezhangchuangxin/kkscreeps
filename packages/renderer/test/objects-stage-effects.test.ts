import { CanvasTextMetrics, Container, Texture } from 'pixi.js';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ObjectsStage } from '../src/objects/objects-stage.js';
import type { LightingPass } from '../src/core/lighting.js';
import type { RoomObjectState } from '@kkscreeps/shared';
import pressure from '../fixtures/pressure.json';
import actions from '../fixtures/actions.json';

interface Internals { views: Map<string, { root: Container; cover?: Container; parts: Map<string, { node: Container }> }>; dying: Map<string, unknown> }
function inspect(stage: ObjectsStage): Internals { return stage as unknown as Internals; }
const stages: ObjectsStage[] = [];
function setup(player: string | null = 'p1') {
  const content = new Container(), dirty = vi.fn();
  const lighting = { content, addMember: (n: Container) => content.addChild(n), removeMember: (n: Container) => content.removeChild(n), markDirty: dirty } as unknown as LightingPass;
  const frames = new Map(['cover', 'link-energy', 'nuke', 'fx-nuke-blur', 'fx-nuke-pulse-0', 'fx-nuke-pulse-1', 'fx-nuke-pulse-2', 'fx-nuke-pulse-3',
    'fx-beam-12', 'fx-beam-18', 'fx-beam-12-blur-5', 'fx-beam-18-blur-5'].map(n => [n, Texture.WHITE]));
  const stage = new ObjectsStage({ cell: 100, lighting, assets: { frames, glow: Texture.WHITE, creepMask: null }, player, objectsLayer: new Container(), tickDurationMS: 1000 });
  stages.push(stage); return { stage, content, dirty };
}
const creep = (id: string, x: number, y: number): RoomObjectState => ({ id, type: 'creep', x, y, user: 'p1', body: [{ type: 'attack', hits: 100 }, { type: 'move', hits: 100 }], spawning: false });
afterEach(() => { for (const s of stages.splice(0)) s.destroy(); });
describe('feed-to-object effects integration', () => {
  it('private speech from another owner is never rendered', () => {
    const { stage } = setup(); const enemy = { ...creep('enemy', 10, 10), user: 'p2' };
    stage.apply([enemy], 0, [], 'snapshot');
    stage.apply([enemy], 1, [], 'delta', { says: [{ id: 'enemy', message: 'private', private: true }] });
    expect(stage.effects.stats.texts).toBe(0); expect(stage.effects.stats.active).toBe(0);
  });
  it('finite effects (say/beams) render above name overlays, matching the official effects-layer insertion order', () => {
    vi.spyOn(CanvasTextMetrics, 'measureText').mockImplementation(() => ({ width: 180 }) as CanvasTextMetrics);
    const { stage } = setup(); const speaker = { ...creep('c1', 10, 10), name: 'Hero' };
    stage.apply([speaker], 0, [], 'snapshot');
    const nameIdx = stage.effectsRoot.getChildIndex(inspect(stage).views.get('c1')!.parts.get('name')!.node);
    expect(nameIdx).toBeGreaterThanOrEqual(0);
    stage.apply([speaker], 1, [], 'delta', { says: [{ id: 'c1', message: 'hi', private: false }] });
    expect(stage.effectsRoot.getChildIndex(stage.effects.root)).toBeGreaterThan(nameIdx);
    vi.mocked(CanvasTextMetrics.measureText).mockRestore();
  });
  it('with no local player identity, private speech stays invisible while public renders', () => {
    // Node has no 2D canvas: stub measurement so the public bubble can actually play.
    vi.spyOn(CanvasTextMetrics, 'measureText').mockImplementation(() => ({ width: 180 }) as CanvasTextMetrics);
    const { stage } = setup(null); const own = creep('own', 10, 10); const enemy = { ...creep('enemy', 12, 10), user: 'p2' };
    stage.apply([own, enemy], 0, [], 'snapshot');
    stage.apply([own, enemy], 1, [], 'delta', { says: [
      { id: 'own', message: 'mine', private: true }, { id: 'enemy', message: 'theirs', private: true }, { id: 'enemy', message: 'loud', private: false },
    ] });
    expect(stage.effects.stats.texts).toBe(1);
    vi.mocked(CanvasTextMetrics.measureText).mockRestore();
  });
  it('real ranged heal and completed build resolve from a removed site; snapshot cancels all finite instances', () => {
    const { stage, content } = setup();
    const snapshot = actions.ticks[0]! as { objects: RoomObjectState[] };
    const delta = actions.ticks[1]! as unknown as { time: number; upsert: RoomObjectState[]; removedIds: string[]; events: import('@kkscreeps/shared').RoomEvent[] };
    stage.apply(snapshot.objects, 0, [], 'snapshot');
    const current = new Map(snapshot.objects.map(s => [s.id, s]));
    for (const o of delta.upsert) current.set(o.id, o); for (const id of delta.removedIds) current.delete(id);
    stage.apply([...current.values()], delta.time, delta.events);
    expect(stage.effects.root.getChildByLabel('beam-rangedHeal')).not.toBeNull();
    expect(stage.effects.root.getChildByLabel('target-cover-build')?.position).toMatchObject({ x: 1400, y: 1700 });
    stage.update(450); expect(stage.effects.root.getChildByLabel('target-cover-build')?.alpha).toBeCloseTo(1);
    stage.apply(snapshot.objects, 0, [], 'snapshot'); expect(stage.effects.stats.active).toBe(0);
    expect(content.children.some(n => n.label === 'target-light')).toBe(false);
    expect(inspect(stage).dying.size).toBe(0);
  });
  it('new movement cancels a bite without shifting the interpolation anchor or its overlays', () => {
    const { stage } = setup(); stage.apply([creep('a', 10, 10), creep('b', 11, 10)], 0, [], 'snapshot');
    stage.apply([creep('a', 10, 10), creep('b', 11, 10)], 1, [{ event: 1, objectId: 'a', data: { targetId: 'b', attackType: 1 } }]);
    stage.update(250); const a = inspect(stage).views.get('a')!; expect(a.root.x).toBe(1010);
    stage.apply([creep('a', 10, 11), creep('b', 11, 10)], 2); expect(a.root.x).toBe(1000);
    stage.update(500); expect([a.root.x, a.root.y]).toEqual([1000, 1050]);
    expect(a.parts.get('glow')!.node.position).toMatchObject({ x: 1000, y: 1050 });
  });
  it('only official cover types flash, including tower/lab/link', () => {
    const { stage } = setup(); const objects = ['tower', 'lab', 'link', 'constructedWall'].map((type, i) => ({ id: type, type: 'structure', structureType: type, x: i + 10, y: 10, hits: 100, hitsMax: 1000 }));
    stage.apply(objects, 0, [], 'snapshot'); stage.apply(objects, 1, objects.map(o => ({ event: 1, objectId: 'ghost', data: { targetId: o.id, attackType: 5 } })));
    for (const id of ['tower', 'lab', 'link']) expect(inspect(stage).views.get(id)!.cover).toBeDefined();
    expect(inspect(stage).views.get('constructedWall')!.cover).toBeUndefined();
  });
  it('nuke removal after landing emits one independent blast; early removal and landed snapshots do not', () => {
    const { stage } = setup(); const nuke = { id: 'n', type: 'nuke', x: 25, y: 25, landTime: 3 };
    stage.apply([nuke], 0, [], 'snapshot'); stage.update(200);
    stage.apply([], 3); const blast = stage.effects.root.getChildByLabel('nuke-blast')!; expect(blast).not.toBeNull();
    stage.apply([], 4); expect(stage.effects.root.children.filter(n => n.label === 'nuke-blast')).toHaveLength(1);
    stage.update(1000); expect(blast.destroyed).toBe(true);
    stage.apply([nuke], 3, [], 'snapshot'); expect(stage.effects.stats.active).toBe(0);
    stage.apply([nuke], 0, [], 'snapshot'); stage.apply([], 1); expect(stage.effects.stats.active).toBe(0);
  });
  it('changing link energy interpolates and a mid-animation snapshot synchronizes immediately', () => {
    const { stage } = setup(); const link = { id: 'l', type: 'structure', structureType: 'link', x: 10, y: 10, store: { energy: 800 }, storeCapacityResource: { energy: 800 } };
    stage.apply([link], 0, [], 'snapshot'); const node = inspect(stage).views.get('l')!.parts.get('energy')!.node;
    expect(node.width).toBe(50); stage.apply([{ ...link, store: { energy: 0 } }], 1); stage.update(500); expect(node.width).toBe(25);
    stage.apply([link], 0, [], 'snapshot'); expect(node.width).toBe(50); stage.update(2000); expect(node.width).toBe(50);
  });
  it('50 concurrent effects deduplicate event/log channels and repeated replay returns to its node baseline', () => {
    const { stage, content } = setup(); const snapshot = pressure.ticks[0]! as { objects: RoomObjectState[] };
    const d = pressure.ticks[1]! as unknown as import('@kkscreeps/shared').RoomDelta;
    for (let i = 0; i < 8; i++) {
      stage.apply(snapshot.objects, 0, [], 'snapshot'); const count = content.children.length;
      stage.apply(d.upsert, d.time, d.events, 'delta', d); expect(stage.effects.stats.active).toBe(50);
      stage.update(300); stage.update(10000); expect(stage.effects.stats.active).toBe(0);
      stage.apply(snapshot.objects, 0, [], 'snapshot'); expect(content.children.length).toBe(count); expect(stage.effects.root.children).toHaveLength(0);
      expect(inspect(stage).views.size).toBe(50);
    }
    stage.apply([], 0, [], 'snapshot'); expect(content.children).toHaveLength(0); expect(inspect(stage).views.size).toBe(0);
    expect(Texture.WHITE.destroyed).toBe(false);
  });
});
