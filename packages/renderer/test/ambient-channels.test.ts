import { Container } from 'pixi.js';
import { describe, expect, it, vi } from 'vitest';
import { AmbientScheduler, type AmbientPart } from '../src/anim/ambient-scheduler.js';
import { PositionChannels, PropertyChannels } from '../src/anim/property-channels.js';
import { ValueTween } from '../src/anim/value-tween.js';
import type { PartValues } from '../src/objects/part-decl.js';

function part(alpha = 1, prev: PartValues | null = {}): AmbientPart {
  const node = new Container(); node.alpha = alpha;
  return { node, channels: new PropertyChannels(node), prev };
}
describe('property ownership and ambient phase', () => {
  it('join base alpha and animation factors multiply; reapplying a join does not capture a sampled base', () => {
    const node = new Container(); node.alpha = 0.5; const dirty = vi.fn();
    const c = new PropertyChannels(node, dirty); c.sample(0.4, 1.2);
    expect(node.alpha).toBeCloseTo(0.2); expect(node.scale.x).toBe(1.2);
    c.restore(); node.alpha = 0.25; c.capture(); expect(node.alpha).toBe(0.1); expect(node.scale.x).toBe(1.2);
    dirty.mockClear(); c.sample(0.4, 1.2); expect(dirty).not.toHaveBeenCalled();
    c.reset(); expect(node.alpha).toBe(0.25); expect(node.scale.x).toBe(1);
  });
  it('position channels retain a movement anchor when bite is replaced/cancelled', () => {
    const node = new Container(), write = vi.fn((x: number, y: number) => node.position.set(x, y));
    const c = new PositionChannels(100, 200, write); c.setOffset(10, 0); c.setBase(150, 250);
    expect([node.x, node.y]).toEqual([160, 250]); c.setOffset(0, 0); expect([node.x, node.y]).toEqual([150, 250]);
  });
  it('offscreen portal wave skips writes, wakes at current phase, and includes glow extents', () => {
    const scheduler = new AmbientScheduler(1000), wave = part(), sample = vi.spyOn(wave.channels, 'sample');
    scheduler.sync('p', 's:portal', { id: 'p', type: 'structure', structureType: 'portal' }, 0, new Map([['wave1', wave]]), () => ({ x: 1000, y: 1000 }));
    scheduler.update(500, { minX: 5000, minY: 5000, maxX: 6000, maxY: 6000 });
    expect(sample).not.toHaveBeenCalled(); expect(scheduler.stats).toEqual({ active: 1, awake: 0, sleeping: 1 });
    scheduler.update(250, { minX: 1600, minY: 1000, maxX: 1700, maxY: 1100 });
    expect(wave.node.scale.x).toBe(0.75); expect(scheduler.stats.awake).toBe(1);
    scheduler.update(0); expect(wave.node.scale.x).toBe(0.75);
    scheduler.update(750); expect(wave.node.alpha).toBe(0.5); scheduler.reset(); expect(wave.node.scale.x).toBe(1);
  });
  it('extractor cooldown stops without rewinding its rotation; the next cycle starts there', () => {
    const scheduler = new AmbientScheduler(1000), main = part(); const parts = new Map([['main', main]]);
    const s = { id: 'e', type: 'structure', structureType: 'extractor', cooldown: 1 };
    scheduler.sync('e', 's:extractor', s, 0, parts, () => ({ x: 0, y: 0 })); scheduler.update(1000);
    expect(main.node.rotation).toBeCloseTo(Math.PI / 2);
    scheduler.sync('e', 's:extractor', { ...s, cooldown: 0 }, 1, parts, () => ({ x: 0, y: 0 })); scheduler.update(1000);
    expect(main.node.rotation).toBeCloseTo(Math.PI / 2); expect(scheduler.stats.active).toBe(0);
    scheduler.sync('e', 's:extractor', s, 2, parts, () => ({ x: 0, y: 0 })); scheduler.update(1000);
    expect(main.node.rotation).toBeCloseTo(Math.PI); scheduler.reset();
  });
  it('one-shot cooldown does not restart on a same-time terrain notification', () => {
    const scheduler = new AmbientScheduler(1000), highlight = part(); const parts = new Map([['highlight', highlight]]);
    const state = { id: 'f', type: 'structure', structureType: 'factory', cooldownTime: 10 };
    scheduler.sync('f', 's:factory', state, 1, parts, () => ({ x: 0, y: 0 })); scheduler.update(200);
    expect(highlight.node.alpha).toBe(1);
    scheduler.sync('f', 's:factory', state, 1, parts, () => ({ x: 0, y: 0 })); scheduler.update(800);
    expect(highlight.node.alpha).toBe(0); expect(scheduler.stats.active).toBe(0);
    scheduler.update(500); expect(highlight.node.alpha).toBe(0);
    scheduler.sync('f', 's:factory', state, 2, parts, () => ({ x: 0, y: 0 })); scheduler.update(200); expect(highlight.node.alpha).toBe(1); scheduler.reset();
  });
  it('removing a binding freezes the phase so fade can own alpha', () => {
    const scheduler = new AmbientScheduler(1000), glow = part(); const sample = vi.spyOn(glow.channels, 'sample');
    scheduler.sync('s', 's:source', { id: 's', type: 'structure' }, 0, new Map([['glowBig', glow]]), () => ({ x: 0, y: 0 }));
    scheduler.update(100); const a = glow.node.alpha; sample.mockClear(); scheduler.remove('s'); scheduler.update(1000);
    expect(sample).not.toHaveBeenCalled(); expect(glow.node.alpha).toBe(a); expect(scheduler.stats.active).toBe(0);
  });
  it('spawn breathing uses official EaseOutQuad upwards and EaseInQuad downwards', () => {
    const scheduler = new AmbientScheduler(1000), core = part(), glow = part(0.5);
    scheduler.sync('s', 'spawn', { id: 's', type: 'spawn', spawning: {} }, 0,
      new Map([['core', core], ['glowBig', glow]]), () => ({ x: 0, y: 0 }));
    scheduler.update(250); expect(core.node.scale.x).toBeCloseTo(1.1875); expect(glow.node.alpha).toBeCloseTo(0.875);
    scheduler.update(1250); expect(core.node.scale.x).toBeCloseTo(1.1875); scheduler.reset();
  });
  it('observer visits four eye positions and snapshot reset restores its local origin', () => {
    const scheduler = new AmbientScheduler(1000), eye = part();
    scheduler.sync('o', 's:observer', { id: 'o', type: 'structure' }, 0, new Map([['eye', eye]]), () => ({ x: 0, y: 0 }));
    scheduler.update(125); expect([eye.node.x, eye.node.y]).toEqual([10, 0]);
    scheduler.update(500); expect([eye.node.x, eye.node.y]).toEqual([10, 7.5]);
    scheduler.update(1375); expect([eye.node.x, eye.node.y]).toEqual([0, -15]);
    scheduler.reset(); expect([eye.node.x, eye.node.y]).toEqual([0, 0]);
  });
  it('a changed nuke countdown speed starts a new pulse instead of reinterpreting its old elapsed phase', () => {
    const scheduler = new AmbientScheduler(1000), pulse = part(0.4);
    const parts = new Map([['pulse', pulse]]), state = { id: 'n', type: 'nuke', landTime: 50000 };
    scheduler.sync('n', 'nuke', state, 0, parts, () => ({ x: 0, y: 0 })); scheduler.update(2000);
    expect(pulse.node.scale.x).toBeCloseTo(0.75);
    scheduler.sync('n', 'nuke', state, 25000, parts, () => ({ x: 0, y: 0 })); scheduler.update(500);
    expect(pulse.node.scale.x).toBeCloseTo(0.875); expect(pulse.node.alpha).toBeCloseTo(0.05); scheduler.reset();
  });
  it('deposit cooldown scales its body and lighting mask together; nested flicker invalidates lighting', () => {
    const scheduler = new AmbientScheduler(1000), sprites = part(), light = part(), dirty = vi.fn();
    const mask = new Container({ label: 'deposit-lighting' }), glow = new Container({ label: 'deposit-glow' });
    light.node.addChild(mask, glow);
    const parts = new Map([['sprites', sprites], ['glow', { ...light, channels: new PropertyChannels(light.node, dirty) }]]);
    const state = { id: 'd', type: 'structure', cooldownTime: 10 };
    scheduler.sync('d', 's:deposit', state, 0, parts, () => ({ x: 0, y: 0 })); scheduler.update(500);
    expect(sprites.node.scale.x).toBeCloseTo(0.775); expect(mask.scale.x).toBeCloseTo(0.775); expect(dirty).toHaveBeenCalled();
    scheduler.sync('d', 's:deposit', { ...state, cooldownTime: 0 }, 1, parts, () => ({ x: 0, y: 0 }));
    expect(sprites.node.scale.x).toBe(1); expect(mask.scale.x).toBe(1); scheduler.reset();
  });
  it('inactive parts and neutral controller status produce no frame writes', () => {
    const scheduler = new AmbientScheduler(1000), status = part(1, { own: false, enemy: false, safe: false });
    const sample = vi.spyOn(status.channels, 'sample');
    scheduler.sync('c', 's:controller', { id: 'c', type: 'structure' }, 0, new Map([['status', status]]), () => ({ x: 0, y: 0 }));
    scheduler.update(500); expect(sample).not.toHaveBeenCalled(); expect(scheduler.stats.active).toBe(0); scheduler.reset();
  });
  it('value interpolation clamps after one received tick, holds draining bars, and never interpolates tint', () => {
    let sample: PartValues = {};
    const tween = new ValueTween({ energyH: 40, hasEnergy: true, tint: 0xff0000 }, { energyH: 0, hasEnergy: false, tint: 0x00ff00 }, ['energyH'], 1000, v => { sample = v; });
    tween.update(500); expect(sample).toEqual({ energyH: 20, hasEnergy: true, tint: 0x00ff00 });
    tween.update(10000); expect(sample).toEqual({ energyH: 0, hasEnergy: false, tint: 0x00ff00 });
  });
});
