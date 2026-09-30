/** Pure phase sampling on the room clock; viewport sleep skips scene writes. */
import type { Container } from 'pixi.js';
import type { RoomObjectState } from '@kkscreeps/shared';
import type { PartValues } from '../objects/part-decl.js';
import { PropertyChannels } from './property-channels.js';
import { EFFECT_SPEC } from './effect-specs.js';

export interface Viewport { minX: number; minY: number; maxX: number; maxY: number }
export interface AmbientStats { active: number; awake: number; sleeping: number }
export interface AmbientPart { readonly node: Container; readonly channels: PropertyChannels; readonly prev: PartValues | null }
interface Sample {
  readonly key: string;
  readonly stamp?: string;
  readonly duration?: number;
  readonly channels: PropertyChannels;
  sample(age: number): void;
  reset?(): void;
}
interface Entry { spec: Sample; started: number; done: boolean }
interface ObjectBinding { readonly position: () => { x: number; y: number }; readonly extent: number; readonly entries: Map<string, Entry>; sleeping: boolean }
interface Aim { from: number; to: number; started: number }
const TAU = Math.PI * 2;
export function blink(age: number, from: number, to: number, up: number, down: number): number {
  const p = age % (up + down);
  return p < up ? from + (to - from) * p / up : to + (from - to) * (p - up) / down;
}
export function colorMix(from: number, to: number, p: number): number {
  let color = 0;
  for (const shift of [16, 8, 0]) color |= Math.round(((from >> shift) & 255) + (((to >> shift) & 255) - ((from >> shift) & 255)) * p) << shift;
  return color;
}
function hash(id: string): number { let h = 2166136261; for (const c of id) h = Math.imul(h ^ c.charCodeAt(0), 16777619); return h >>> 0; }
function number(v: unknown): number { return typeof v === 'number' && Number.isFinite(v) ? v : 0; }
function ease(p: number): number { return p < 0.5 ? 2 * p * p : 1 - (-2 * p + 2) ** 2 / 2; }
function easeOut(p: number): number { return 1 - (1 - p) ** 2; }

export class AmbientScheduler {
  private now = 0;
  private readonly objects = new Map<string, ObjectBinding>();
  private readonly aims = new Map<string, Aim>();
  private readonly nested = new WeakMap<Container, PropertyChannels>();
  constructor(private readonly tick: number) {}
  get stats(): AmbientStats {
    const s = { active: 0, awake: 0, sleeping: 0 };
    for (const o of this.objects.values()) for (const e of o.entries.values()) if (!e.done) { s.active++; if (o.sleeping) s.sleeping++; else s.awake++; }
    return s;
  }
  aim(id: string, angle: number, node: Container): void {
    let delta = (angle - node.rotation) % TAU;
    if (delta > Math.PI) delta -= TAU; else if (delta < -Math.PI) delta += TAU;
    this.aims.set(id, { from: node.rotation, to: node.rotation + delta, started: this.now });
  }
  sync(id: string, type: string, state: RoomObjectState, time: number,
    parts: ReadonlyMap<string, AmbientPart>, position: () => { x: number; y: number }): void {
    const specs: Sample[] = [];
    const add = (partId: string, sample: (c: PropertyChannels, age: number) => void, options: { stamp?: string; duration?: number } = {}) => {
      const part = parts.get(partId); if (!part || part.prev === null || !part.node.visible) return;
      specs.push({ key: partId, channels: part.channels, ...options, sample: age => sample(part.channels, age) });
    };
    const child = (parent: string, label: string): PropertyChannels | undefined => {
      const node = parts.get(parent)?.node.getChildByLabel(label); if (!node) return;
      let c = this.nested.get(node); if (!c) { c = new PropertyChannels(node, () => parts.get(parent)?.channels.invalidate()); this.nested.set(node, c); } return c;
    };
    const once = { stamp: `${time}`, duration: this.tick };
    if (type === 's:observer') {
      const eye = parts.get('eye');
      if (eye?.prev) specs.push({ key: 'eye', channels: eye.channels, sample: age => {
        const points = [[20, 0], [0, 15], [-20, 0], [0, -15]] as const;
        const phase = age % 2000 / 500, i = Math.floor(phase), to = points[i]!;
        const from = age < 500 ? [0, 0] : points[(i + 3) % 4]!;
        const p = Math.min(1, (phase - i) * 2);
        const x = from[0]! + (to[0] - from[0]!) * p, y = from[1]! + (to[1] - from[1]!) * p;
        if (eye.node.x !== x || eye.node.y !== y) eye.node.position.set(x, y);
      }, reset: () => eye.node.position.set(0, 0) });
    }
    if (type === 's:deposit') {
      const deposit = child('glow', 'deposit-lighting'), glow = child('glow', 'deposit-glow');
      if (number(state.cooldownTime) >= time && number(state.cooldownTime) > 0) {
        const scale = (age: number) => { const p = age % 1300; return p < 1000 ? 1 - 0.3 * easeOut(p / 1000) : 0.7 + 0.3 * easeOut((p - 1000) / 300); };
        add('sprites', (c, age) => c.sample(1, scale(age)));
        if (deposit) specs.push({ key: 'deposit-scale', channels: deposit, sample: age => deposit.sample(1, scale(age)) });
      }
      if (glow) specs.push({ key: 'deposit-flicker', channels: glow, sample: age => {
        const delay = hash(id) % 2001, p = age % (3300 + delay);
        const a = p < 100 ? (age < 3300 + delay ? 1 : 0.4) + (0.7 - (age < 3300 + delay ? 1 : 0.4)) * p / 100
          : p < 300 ? 0.7 - 0.3 * (p - 100) / 200 : p < 300 + delay ? 0.4
          : p < 500 + delay ? 0.4 + 0.6 * (p - 300 - delay) / 200 : 1 - 0.6 * (p - 500 - delay) / 2800;
        glow.sample(a);
      } });
    }
    if (state._isDisabled === true) add('disabled', (c, age) => c.sample(blink(age, 0, 1, 300, 1500)));
    if (type === 's:source' || type === 's:mineral') {
      const source = type === 's:source', delay = (hash(id) % 2001);
      const alphas = source ? [0.6, 0.4, 0.8, 0.4] : [0.7, 0.4, 1, 0.4];
      add('glowBig', (c, age) => {
        const p = age % (3300 + delay);
        const startAlpha = age < 3300 + delay ? (source ? 0.5 : 0.7) : alphas[3]!;
        const a = p < 100 ? startAlpha + (alphas[0]! - startAlpha) * p / 100
          : p < 300 ? alphas[0]! + (alphas[1]! - alphas[0]!) * (p - 100) / 200
          : p < 300 + delay ? alphas[1]!
          : p < 500 + delay ? alphas[1]! + (alphas[2]! - alphas[1]!) * (p - 300 - delay) / 200
          : alphas[2]! + (alphas[3]! - alphas[2]!) * (p - 500 - delay) / 2800;
        c.sample(a / (source ? 0.5 : 0.7), blink(age, 1, 1.2, 1000, 1000));
      });
      if (source) {
        add('frame', (c, age) => c.sample(1, 1, 0, colorMix(0x595026, 0x0e0c04, blink(age, 0, 1, 2000, 2000))));
        add('energyRect', (c, age) => {
          const colors = [0xffe56d, 0xffffff, 0xffcd6d, 0xffde84, 0xffe56d];
          const p = age % 2000 / 500, i = Math.floor(p);
          c.sample(1, 1, 0, colorMix(colors[i]!, colors[i + 1]!, p - i));
        });
      }
    }
    if (type === 's:portal' || type === 's:keeperLair') {
      const portal = type === 's:portal';
      add('wave1', (c, age) => { const p = age % 2000 / 1000; c.sample(p < 1 ? 1 : 2 - p, Math.min(1, p)); });
      add('wave2', (c, age) => { const p = age % 2000 / 1000; c.sample(portal && p >= 1 ? 2 - p : 1, p < 1 ? p * 0.3 : 0.3 + (p - 1) * 0.7); });
      if (portal) add('glowBig', (c, age) => c.sample(blink(age, 0.7, 0.1, 3500, 3500) / 0.7, blink(age, 1, 1.2, 2000, 2000)));
    }
    if (type === 's:tower') {
      const gun = child('rotatable', 'gun');
      if (gun) specs.push({ key: 'gun-idle', channels: gun, sample: age => {
        const aim = this.aims.get(id);
        const angle = aim ? aim.from + (aim.to - aim.from) * Math.min(1, (this.now - aim.started) / EFFECT_SPEC.towerTurnMS)
          + Math.max(0, this.now - aim.started - this.tick) * Math.PI / 10000
          : (hash(id) / 0xffffffff) * TAU + age * Math.PI / 10000;
        gun.sample(1, 1, angle);
      } });
    }
    if (type === 's:extractor' && number(state.cooldown) > 0) add('main', (c, age) => c.sample(1, 1, age * TAU / 4000));
    if (type === 'spawn' && state.spawning) {
      add('core', (c, age) => { const p = age % 2500; c.sample(1, p < 500 ? 1 + 0.25 * easeOut(p / 500) : 1.25 - 0.25 * ((p - 500) / 2000) ** 2); });
      add('glowBig', (c, age) => { const p = age % 2500; c.sample(p < 500 ? 1 + easeOut(p / 500) : 2 - ((p - 500) / 2000) ** 2); });
    }
    const cooldown = number(state.cooldownTime);
    if ((type === 's:factory' && cooldown >= time && cooldown > 0) || (type === 's:lab' && cooldown > time) || (type === 's:terminal' && cooldown >= time && cooldown > 0)) {
      add('highlight', (c, age) => { const p = Math.min(1, age / this.tick); c.sample(p < 0.2 ? p / 0.2 : (1 - p) / 0.8); }, once);
    }
    if (type === 's:factory' && number(state.level) > 0) {
      const effects = state.effects;
      const powered = typeof effects === 'object' && effects !== null && Object.values(effects).some(v => typeof v === 'object' && v !== null
        && number((v as Record<string, unknown>).power) === 19 && number((v as Record<string, unknown>).endTime) >= time);
      if (!powered) add('level', (c, age) => c.sample(blink(age, 0, 1, 200, 800)));
    }
    if (type === 's:controller') {
      if (Object.values(parts.get('status')?.prev ?? {}).some(v => v === true)) add('status', (c, age) => c.sample(blink(age, 0.05, 0.4, 1000, 1000) / 0.4));
      add('progress', (c, age) => c.sample(blink(age, 0.8, 0.3, 1000, 1000) / 0.8));
      if (number(parts.get('downgrade')?.prev?.alpha) > 0) add('downgrade', (c, age) => {
        const p = Math.min(1, age / this.tick); c.sample(p < 0.2 ? p / 0.2 : (1 - p) / 0.8);
      }, once);
    }
    // Official blinking lives only in constructionSite.metadata; the invaderCore TTL ring is static.
    if (type === 'constructionSite') add('progress', (c, age) => c.sample(blink(age, 0.8, 0.3, 1000, 1000) / 0.8));
    if (type === 's:constructedWall' && number(state.hits) === 1 && number(state.hitsMax) > 1) add('flash', (c, age) => c.sample(blink(age, 0, 1, 300, 1500)));
    if (type === 's:powerSpawn') add('glowPulse', (c, age) => {
      const p = Math.min(1, age / this.tick), v = p < 0.2 ? p / 0.2 : (1 - p) / 0.8;
      c.sample(1 + v, 1 + v, 0, colorMix(0xffffff, 0xff3333, v));
    }, { ...once, stamp: JSON.stringify(state.store ?? {}) });
    if (type === 'nuke' && number(state.landTime) > time) {
      const breathing = (age: number) => { const p = age % 2000 / 1000; return p < 1 ? 1 - 0.4 * ease(p) : 0.6 + 0.4 * ease(p - 1); };
      add('marker', (c, age) => c.sample(1, breathing(age)));
      add('circle', (c, age) => c.sample(1, breathing(age)));
      const duration = Math.max(400, Math.round((number(state.landTime) - time) / 100) * 100 / 50000 * 8000);
      add('pulse', (c, age) => {
        const p = age % duration / duration; c.sample(p, 1 - p);
        const mask = Math.min(1, p * 2) * 3, index = Math.floor(mask), f = mask - index;
        for (const child of c.node.children) {
          const i = Number(child.label.slice(5));
          const a = i === index ? 1 - f : i === index + 1 ? f : 0;
          if (child.alpha !== a) child.alpha = a;
        }
      }, { stamp: `${duration}` });
    }
    let binding = this.objects.get(id);
    if (!binding) { binding = { position, extent: type === 'nuke' ? 720 : 650, entries: new Map(), sleeping: false }; this.objects.set(id, binding); }
    const wanted = new Set(specs.map(s => s.key));
    for (const [key, entry] of binding.entries) if (!wanted.has(key)) {
      if (type === 's:extractor' && key === 'main') entry.spec.channels.commit(); else entry.spec.channels.reset();
      entry.spec.reset?.();
      binding.entries.delete(key);
    }
    for (const spec of specs) {
      const entry = binding.entries.get(spec.key);
      if (!entry || entry.spec.stamp !== spec.stamp) {
        binding.entries.set(spec.key, { spec, started: this.now, done: false });
        // Preserve the existing first-snapshot static phase until the first frame.
      } else entry.spec = spec;
    }
  }
  update(deltaMS: number, viewport?: Viewport): void {
    if (deltaMS <= 0) return;
    this.now += deltaMS;
    for (const object of this.objects.values()) {
      const p = object.position(), e = object.extent;
      const visible = !viewport || p.x + e >= viewport.minX && p.x - e <= viewport.maxX && p.y + e >= viewport.minY && p.y - e <= viewport.maxY;
      object.sleeping = !visible;
      for (const entry of object.entries.values()) {
        if (entry.done) continue;
        if (!visible) continue;
        const age = this.now - entry.started;
        entry.spec.sample(entry.spec.duration === undefined ? age : Math.min(age, entry.spec.duration));
        if (entry.spec.duration !== undefined && age >= entry.spec.duration) entry.done = true;
      }
    }
  }
  remove(id: string): void { this.objects.delete(id); this.aims.delete(id); }
  reset(): void {
    for (const object of this.objects.values()) for (const e of object.entries.values()) { e.spec.channels.reset(); e.spec.reset?.(); }
    this.objects.clear(); this.aims.clear(); this.now = 0;
  }
}
