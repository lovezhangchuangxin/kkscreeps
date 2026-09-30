import { Container, Sprite, Texture } from 'pixi.js';
import { describe, expect, it, vi } from 'vitest';
import { EffectsStage, type ActorBinding } from '../src/anim/effects-stage.js';
import { beamWindow } from '../src/anim/effect-specs.js';
import type { EffectCommand, TargetCommand } from '../src/anim/action-resolver.js';
import type { LightingPass } from '../src/core/lighting.js';

function setup(tick = 1000) {
  const lightingRoot = new Container(), node = new Container(); node.position.set(1000, 1000);
  const dirty = vi.fn(), turn = vi.fn(), towerAim = vi.fn(), towerGlow = vi.fn();
  const offset = vi.fn((x: number, y: number) => { node.position.set(1000 + x, 1000 + y); });
  const actor: ActorBinding = { get position() { return node.position; }, moving: false, offset, turn, towerAim, towerGlow };
  const actors = new Map([['a', actor]]);
  const names = ['cover', 'flare1', 'flare2', 'lab-mineral', 'operate-tower',
    'fx-beam-12', 'fx-beam-18', 'fx-beam-12-blur-5', 'fx-beam-18-blur-5', 'fx-beam-12-blur-2'];
  const measure = vi.fn(() => 180);
  const lighting = { content: lightingRoot, addMember: (n: Container) => lightingRoot.addChild(n),
    removeMember: (n: Container) => lightingRoot.removeChild(n), markDirty: dirty } as unknown as LightingPass;
  const effects = new EffectsStage({ frames: new Map(names.map(n => [n, Texture.WHITE])), glow: Texture.WHITE, creepMask: null }, lighting, 100, tick, id => actors.get(id), measure);
  return { effects, lightingRoot, actors, node, dirty, turn, offset, towerAim, towerGlow, measure };
}
function command(action: TargetCommand['action'], extra: Partial<TargetCommand> = {}): TargetCommand {
  return { action, sourceId: 'a', sourceType: 'creep', source: { x: 10, y: 10 }, target: { x: 12, y: 10 }, origin: 'event', ...extra };
}
describe('finite effect lifetime and keyframes', () => {
  it('beam head reaches the target at half-time, then its tail follows', () => {
    expect([0, 0.25, 0.5, 0.75, 1].map(beamWindow)).toEqual([[0, 0], [0, 0.5], [0, 1], [0.5, 1], [1, 1]]);
    const { effects } = setup(); effects.play([command('rangedAttack')]);
    const beam = effects.root.children[0]!, core = beam.getChildByLabel('beam-core') as Sprite;
    expect(beam.visible).toBe(false);
    expect(core.blendMode).toBe('add');
    effects.update(150); expect([core.x, core.width]).toEqual([0, 100]);
    effects.update(150); expect(core.width).toBe(200);
    effects.update(150); expect([core.x, core.width]).toEqual([100, 100]);
    effects.update(150); expect(beam.destroyed).toBe(true); expect(effects.stats.active).toBe(0);
    expect(Texture.WHITE.destroyed).toBe(false); effects.destroy();
  });
  it('work cover delay=.3 tick, peak=.45 tick, expiry=.9 tick; lighting is detached', () => {
    const { effects, lightingRoot, dirty } = setup(); effects.play([command('build')]);
    const cover = effects.root.getChildByLabel('target-cover-build')!;
    const light = lightingRoot.children[0]!;
    effects.update(300); expect(cover.alpha).toBe(0);
    effects.update(150); expect(cover.alpha).toBeCloseTo(1); expect(light.alpha).toBeCloseTo(0.5);
    effects.update(450); expect(cover.destroyed).toBe(true); expect(light.destroyed).toBe(true); expect(lightingRoot.children).toHaveLength(0);
    expect(dirty).toHaveBeenCalled(); effects.destroy();
  });
  it('bite composes a lunge, turns at least 400ms, and cancel restores the offset', () => {
    const { effects, node, turn } = setup(); effects.play([command('attack')]);
    expect(turn).toHaveBeenCalledWith(Math.PI / 2, 400);
    effects.update(250); expect(node.x).toBe(1010);
    effects.update(375); expect(node.x).toBe(1005);
    effects.cancelOwner('a'); expect(node.x).toBe(1000); expect(effects.stats.active).toBe(0); effects.destroy();
  });
  it('moving actors skip the lunge while their target work cover still plays', () => {
    const s = setup(); s.actors.set('a', { ...s.actors.get('a')!, moving: true });
    s.effects.play([command('harvest')]); expect(s.offset).not.toHaveBeenCalled();
    expect(s.effects.root.getChildByLabel('target-cover-harvest')).not.toBeNull();
    s.effects.play([command('attack', { sourceId: 'gone' })]);
    expect(s.effects.stats.skipped).toBe(1); s.effects.destroy();
  });
  it('owner removal cancels attached effects; captured room beam endpoints survive', () => {
    const { effects, actors, lightingRoot } = setup(); effects.play([command('build')]);
    const beam = effects.root.getChildByLabel('beam-build')!;
    actors.delete('a'); effects.cancelOwner('a');
    expect(lightingRoot.children).toHaveLength(0); expect(beam.destroyed).toBe(false);
    effects.update(10000); expect(beam.destroyed).toBe(true); expect(effects.stats.active).toBe(0); effects.destroy();
  });
  it('tower branch aims with the reversed artwork vector, flares and pulses once', () => {
    const s = setup(); s.effects.play([command('attack', { sourceType: 's:tower' })]);
    expect(s.towerAim).toHaveBeenCalledWith(-Math.PI / 2);
    expect(s.offset).not.toHaveBeenCalled();
    const flare = s.effects.root.getChildByLabel('tower-shot-flare')!;
    s.effects.update(100); expect(flare.alpha).toBeCloseTo(0.2); expect(s.towerGlow).toHaveBeenLastCalledWith(2);
    s.effects.update(300); expect(flare.destroyed).toBe(true); expect(s.towerGlow).toHaveBeenLastCalledWith(1); s.effects.destroy();
  });
  it('tower ranged heal also aims, flares and pulses beside its wide heal beam', () => {
    const s = setup(); s.effects.play([command('rangedHeal', { sourceType: 's:tower' })]);
    expect(s.towerAim).toHaveBeenCalledWith(-Math.PI / 2);
    expect(s.offset).not.toHaveBeenCalled();
    expect(s.effects.root.getChildByLabel('beam-rangedHeal')).not.toBeNull();
    const flare = s.effects.root.getChildByLabel('tower-shot-flare')!;
    s.effects.update(100); expect(flare.alpha).toBeCloseTo(0.2); expect(s.towerGlow).toHaveBeenLastCalledWith(2); s.effects.destroy();
  });
  it('say is measured once, follows the actor, replacement/reset disposes private text', () => {
    const s = setup(); const say: EffectCommand = { action: 'say', sourceId: 'a', message: 'hello', private: true, origin: 'say' };
    s.effects.play([say]); const bubble = s.effects.root.getChildByLabel('say-bubble')!;
    const text = bubble.children[1]!;
    s.node.x = 1100; s.effects.update(50); expect(bubble.x).toBe(1100); expect(s.measure).toHaveBeenCalledTimes(1);
    s.effects.play([{ ...say, message: 'new' }]); expect(text.destroyed).toBe(true); expect(s.effects.stats.texts).toBe(1);
    s.effects.reset(); expect(s.effects.stats).toMatchObject({ active: 0, texts: 0, nodes: 0 }); s.effects.destroy();
  });
  it('reaction pairs share a single flash, and power has a short beam plus delayed icon', () => {
    const s = setup(); s.effects.play([command('runReaction', { sourceType: 's:lab', source: { x: 8, y: 8 } }), command('runReaction', { sourceType: 's:lab', source: { x: 9, y: 9 } }), command('power', { powerId: 3 })]);
    expect(s.effects.root.children.filter(n => n.label === 'lab-reaction-light')).toHaveLength(1);
    const icon = s.effects.root.getChildByLabel('power-influence')!;
    s.effects.update(300); expect(s.effects.root.getChildByLabel('beam-power')).toBeNull(); expect(icon.alpha).toBe(0);
    s.effects.update(150); expect(icon.alpha).toBeCloseTo(1);
    s.effects.update(10000); expect(s.effects.stats.active).toBe(0); s.effects.destroy();
  });
  it('failed speech measurement releases its private text and records a skipped effect', () => {
    const s = setup(); s.measure.mockImplementationOnce(() => { throw new Error('font measurement'); });
    s.effects.play([{ action: 'say', sourceId: 'a', message: 'hello', private: true, origin: 'say' }]);
    expect(s.effects.stats).toMatchObject({ nodes: 0, active: 0, texts: 0, skipped: 1 }); s.effects.destroy();
  });
  it('short ticks cannot fit the fixed 300ms power-icon delay, so no invalid zero-length icon phase is created', () => {
    const s = setup(200); s.effects.play([command('power', { powerId: 3 })]);
    expect(s.effects.root.getChildByLabel('power-influence')).toBeNull();
    expect(s.effects.stats.active).toBe(1); s.effects.update(200);
    expect(s.effects.stats.active).toBe(0); s.effects.destroy();
  });
  it('nuke flash fades with official default EaseOutQuad', () => {
    const s = setup(); s.effects.nukeBlast({ x: 12, y: 12 });
    const flash = s.effects.root.getChildByLabel('nuke-blast')!.children[0]!;
    s.effects.update(500); expect(flash.alpha).toBeCloseTo(0.175);
    s.effects.update(500); expect(s.effects.stats.active).toBe(0); s.effects.destroy();
  });
  it('mass and nuke waves finish on large elapsed frames and pause does not move them', () => {
    const s = setup(); s.effects.play([{ action: 'rangedMassAttack', sourceId: 'a', source: { x: 10, y: 10 }, origin: 'event' }]);
    s.effects.nukeBlast({ x: 12, y: 12 }); const wave = s.effects.root.getChildByLabel('ranged-mass-wave')!;
    s.effects.update(0); expect(wave.scale.x).toBe(0);
    s.effects.update(300); expect(wave.scale.x).toBe(0.75); expect(wave.alpha).toBeCloseTo(0.1); expect(wave.blendMode).toBe('add');
    s.effects.update(10000); expect(s.effects.root.children).toHaveLength(0); s.effects.destroy();
  });
  it('idle stage has no frame writes and repeated reset is idempotent', () => {
    const s = setup(); s.effects.update(1000); s.effects.reset(); s.effects.reset();
    expect(s.offset).not.toHaveBeenCalled(); expect(s.dirty).not.toHaveBeenCalled(); s.effects.destroy();
  });
});
