import { CanvasTextMetrics, Container, Graphics, Sprite, Text, type Texture } from 'pixi.js';
import type { LightingPass } from '../core/lighting.js';
import type { JoinAssets } from '../objects/joins-naturals.js';
import type { EffectCommand, Point, TargetCommand } from './action-resolver.js';
import { angleBetween, beamWindow, coverAlpha, EFFECT_COLORS as C, EFFECT_SPEC as S, POWER_ICONS } from './effect-specs.js';

/** Internal view port; effects never own or destroy object parts. */
export interface ActorBinding {
  readonly position: Point;
  readonly moving: boolean;
  offset(x: number, y: number): void;
  turn(angle: number, durationMS: number): void;
  towerAim(angle: number): void;
  towerGlow(factor: number): void;
}
interface Player {
  readonly owner?: string;
  readonly channel?: string;
  elapsed: number;
  readonly duration: number;
  sample(progress: number): void;
  dispose(): void;
}
export interface EffectsStats { active: number; nodes: number; texts: number; skipped: number }
export type TextMeasure = (message: string, text: Text) => number;

export class EffectsStage {
  readonly root = new Container({ label: 'finite-effects' });
  private readonly players = new Set<Player>();
  private readonly lights = new Set<Container>();
  private texts = 0;
  private skipped = 0;
  constructor(private readonly assets: JoinAssets, private readonly lighting: LightingPass,
    private readonly cell: number, private readonly tick: number,
    private readonly actor: (id: string) => ActorBinding | undefined,
    private readonly measure: TextMeasure = (message, text) => CanvasTextMetrics.measureText(message, text.style).width) {}

  get stats(): EffectsStats {
    let nodes = this.lights.size;
    const count = (n: Container) => { nodes++; for (const child of n.children) count(child); };
    for (const n of this.root.children) count(n);
    return { active: this.players.size, nodes, texts: this.texts, skipped: this.skipped };
  }
  recordSkipped(count: number): void { this.skipped += count; }
  play(commands: readonly EffectCommand[]): void {
    const reactions = new Set<string>();
    for (const command of commands) {
      if (command.action === 'say') { this.say(command); continue; }
      if (command.action === 'rangedMassAttack') { this.wave(command); continue; }
      this.target(command);
      if ((command.action === 'runReaction' || command.action === 'reverseReaction') && !reactions.has(command.sourceId)) {
        reactions.add(command.sourceId); this.reaction(command);
      }
    }
  }
  update(deltaMS: number): void {
    if (deltaMS <= 0) return;
    for (const p of this.players) {
      p.elapsed += deltaMS;
      if (p.elapsed >= p.duration) { p.sample(1); this.remove(p); }
      else p.sample(p.elapsed / p.duration);
    }
  }
  cancelOwner(owner: string, channel?: string): void {
    for (const p of this.players) if (p.owner === owner && (channel === undefined || p.channel === channel)) this.remove(p);
  }
  reset(): void { for (const p of this.players) this.remove(p); this.skipped = 0; }
  destroy(): void { this.reset(); this.root.destroy({ children: true }); }
  private remove(p: Player): void { if (this.players.delete(p)) p.dispose(); }
  private run(duration: number, sample: Player['sample'], dispose: Player['dispose'], owner?: string, channel?: string): void {
    if (owner && channel) this.cancelOwner(owner, channel);
    const p: Player = { elapsed: 0, duration, sample, dispose, owner, channel };
    this.players.add(p); p.sample(0);
  }
  private sprite(texture: Texture | null | undefined, label: string, size?: number): Sprite | null {
    if (!texture) return null;
    const s = new Sprite(texture); s.label = label; s.anchor.set(0.5);
    if (size !== undefined) s.setSize(size);
    return s;
  }
  private world(p: Point): Point { return { x: p.x * this.cell, y: p.y * this.cell }; }
  private target(c: TargetCommand): void {
    const tower = c.sourceType === 's:tower', core = c.sourceType === 's:invaderCore';
    const width = tower || core || c.action === 'transferEnergy' ? 18 : 12;
    let color: number = C.work;
    if (c.action === 'attack' || c.action === 'rangedAttack') color = C.ranged;
    else if (c.action === 'heal' || c.action === 'rangedHeal') color = C.heal;
    else if (c.action.endsWith('Controller') && c.action !== 'upgradeController') color = C.reserve;
    else if (c.action === 'power') color = C.power;
    else if (c.action === 'runReaction' || c.action === 'reverseReaction') color = C.reaction;
    const bite = !tower && !core && (c.action === 'attack' || c.action === 'heal' || c.action === 'harvest'
      || c.action === 'reserveController' || c.action === 'attackController' || c.action === 'claimController');
    if (bite) this.bite(c);
    else this.beam(c, color, width, c.action.includes('Reaction') ? 2 : 5, c.action === 'power' ? S.powerDuration : S.beamDuration);
    if (c.action === 'harvest' || c.action === 'build' || c.action === 'repair' || c.action === 'upgradeController'
      || c.action === 'reserveController' || c.action === 'attackController' || c.action === 'claimController') {
      const delay = c.action === 'build' || c.action === 'repair' || c.action === 'upgradeController' || core ? S.workDelay : 0;
      this.cover(c, color, delay);
    }
    if (tower && (c.action === 'attack' || c.action === 'rangedAttack' || c.action === 'heal' || c.action === 'rangedHeal' || c.action === 'repair')) this.tower(c);
    if (c.action === 'power' && c.powerId !== undefined) this.powerIcon(c);
  }
  private beam(c: TargetCommand, color: number, width: number, blur: number, fraction: number): void {
    const from = c.action === 'runReaction' ? this.world(c.source) : this.actor(c.sourceId)?.position ?? this.world(c.source);
    const to = this.world(c.target), dx = to.x - from.x, dy = to.y - from.y, length = Math.hypot(dx, dy);
    if (length === 0) return;
    const root = new Container({ label: `beam-${c.action}` });
    root.rotation = Math.atan2(dy, dx); root.position.set(from.x, from.y);
    const crisp = this.sprite(this.assets.frames.get(`fx-beam-${width}`), 'beam-core');
    const soft = this.sprite(this.assets.frames.get(`fx-beam-${width}-blur-${blur}`), 'beam-glow');
    if (!crisp || !soft) { root.destroy(); crisp?.destroy(); soft?.destroy(); this.skipped++; return; }
    crisp.anchor.set(0, 0.5); soft.anchor.set(0, 0.5);
    crisp.tint = soft.tint = color; crisp.blendMode = 'add'; soft.alpha = S.beamAlpha; soft.blendMode = 'add';
    root.addChild(soft, crisp); this.root.addChild(root);
    this.run(this.tick * fraction, p => {
      const [a, b] = beamWindow(p);
      crisp.x = soft.x = length * a;
      crisp.width = soft.width = Math.max(0.001, length * (b - a));
      root.visible = b > a;
    }, () => root.destroy({ children: true }));
  }
  private bite(c: TargetCommand): void {
    const binding = this.actor(c.sourceId);
    if (!binding) { this.skipped++; return; }
    // Moving and same-cell actors are official suppressions (posChanged gate), not missing data.
    if (binding.moving || c.source.x === c.target.x && c.source.y === c.target.y) return;
    const x = Math.sign(c.target.x - c.source.x) * S.bitePenetration;
    const y = Math.sign(c.target.y - c.source.y) * S.bitePenetration;
    binding.turn(angleBetween(c.source, c.target), Math.max(this.tick / 5, 400));
    this.run(this.tick, p => {
      const v = coverAlpha(p); binding.offset(x * v, y * v);
    }, () => binding.offset(0, 0), c.sourceId, 'bite');
  }
  private cover(c: TargetCommand, color: number, delay: number): void {
    const root = new Container({ label: `target-cover-${c.action}` }), target = this.world(c.target);
    root.position.set(target.x, target.y);
    const cover = this.sprite(this.assets.frames.get('cover'), 'target-disc');
    const flare = this.sprite(this.assets.frames.get('flare2'), 'target-flare', 300);
    const glow = this.sprite(this.assets.glow, 'target-light', 500);
    if (cover) { cover.tint = color; cover.alpha = 0.3; root.addChild(cover); }
    if (flare) { flare.tint = color; flare.alpha = 0.05; flare.blendMode = 'add'; root.addChild(flare); }
    if (glow) { glow.position.set(target.x, target.y); glow.alpha = 0; glow.blendMode = 'screen'; this.lighting.addMember(glow, false); this.lights.add(glow); }
    this.root.addChild(root);
    this.run(this.tick * S.coverDuration, p => {
      const a = coverAlpha(p, delay / S.coverDuration); root.alpha = a;
      if (glow && glow.alpha !== a * 0.5) { glow.alpha = a * 0.5; this.lighting.markDirty(); }
    }, () => { if (glow) { this.lighting.removeMember(glow); this.lights.delete(glow); glow.destroy(); } root.destroy({ children: true }); }, c.sourceId, `cover-${c.action}`);
  }
  private wave(c: Extract<EffectCommand, { action: 'rangedMassAttack' }>): void {
    const root = new Graphics({ label: 'ranged-mass-wave' }).circle(0, 0, S.waveRadius).fill(S.waveColor);
    root.blendMode = 'add';
    const from = this.actor(c.sourceId)?.position ?? this.world(c.source);
    root.position.set(from.x, from.y); this.root.addChild(root);
    this.run(this.tick * S.beamDuration, p => { const eased = 1 - (1 - p) ** 2; root.scale.set(eased); root.alpha = S.waveAlpha * (1 - eased); }, () => root.destroy());
  }
  private tower(c: TargetCommand): void {
    const binding = this.actor(c.sourceId); if (!binding) return;
    // Gun artwork points down; official calculates the reverse target vector.
    binding.towerAim(angleBetween(c.target, c.source));
    const flare = this.sprite(this.assets.frames.get('flare1'), 'tower-shot-flare', S.towerFlareSize);
    if (flare) { flare.blendMode = 'add'; flare.position.set(binding.position.x, binding.position.y); this.root.addChild(flare); }
    this.run(this.tick * 0.4, p => {
      const a = coverAlpha(p); if (flare) flare.alpha = a * 0.2;
      binding.towerGlow(1 + a);
    }, () => { flare?.destroy(); binding.towerGlow(1); }, c.sourceId, 'tower-shot');
  }
  private reaction(c: TargetCommand): void {
    const flare = this.sprite(this.assets.glow, 'lab-reaction-light', 150);
    if (!flare) return;
    const p = this.world(c.sourceType === 's:lab' && c.action === 'runReaction' ? c.target : c.source);
    flare.position.set(p.x, p.y); flare.blendMode = 'add'; this.root.addChild(flare);
    const delay = c.action === 'runReaction' ? 0.3 : 0;
    this.run(this.tick, t => {
      flare.alpha = t < delay ? 0 : t < delay + 0.15 ? (t - delay) / 0.15 : Math.max(0, 1 - (t - delay - 0.15) / 0.55);
    }, () => flare.destroy(), c.sourceId, 'reaction-light');
  }
  private say(c: Extract<EffectCommand, { action: 'say' }>): void {
    if (!this.actor(c.sourceId)) return;
    this.cancelOwner(c.sourceId, 'say');
    const root = new Container({ label: 'say-bubble' });
    const text = new Text({ text: c.message, style: { fontFamily: 'Roboto', fontSize: S.say.fontSize, fill: 0x111111, align: 'center' } });
    text.anchor.set(0.5); text.y = S.say.textY;
    let width: number;
    try { width = this.measure(c.message, text) + S.say.padding; }
    catch { text.destroy({ style: true, texture: true, textureSource: true }); root.destroy(); this.skipped++; return; }
    if (!Number.isFinite(width)) { text.destroy({ style: true, texture: true, textureSource: true }); root.destroy(); this.skipped++; return; }
    const bg = c.private ? S.say.private : S.say.public;
    const left = -width / 2, right = width / 2, top = S.say.y, bottom = top + S.say.height, r = S.say.radius;
    const shape = new Graphics().moveTo(left + r, top).lineTo(right - r, top)
      .arc(right - r, top + r, r, -Math.PI / 2, 0).lineTo(right, bottom - r)
      .arc(right - r, bottom - r, r, 0, Math.PI / 2).lineTo(30, bottom)
      .lineTo(0, S.say.tailY).lineTo(-30, bottom).lineTo(left + r, bottom)
      .arc(left + r, bottom - r, r, Math.PI / 2, Math.PI).lineTo(left, top + r)
      .arc(left + r, top + r, r, Math.PI, Math.PI * 1.5).closePath()
      .fill(bg).stroke({ width: S.say.strokeWidth, color: 0x000000 });
    root.addChild(shape, text); this.root.addChild(root); this.texts++;
    this.run(this.tick, () => {
      const binding = this.actor(c.sourceId); if (binding) root.position.set(binding.position.x, binding.position.y);
    }, () => { text.destroy({ style: true, texture: true, textureSource: true }); root.destroy({ children: true }); this.texts--; }, c.sourceId, 'say');
  }
  private powerIcon(c: TargetCommand): void {
    const name = POWER_ICONS[c.powerId!];
    const icon = this.sprite(name ? this.assets.frames.get(name) : undefined, 'power-influence', 300);
    if (!icon) { this.skipped++; return; }
    const base = icon.scale.x, delay = 300, phase = Math.max(0, (this.tick * 0.9 - delay) / 4);
    if (phase === 0) { icon.destroy(); return; }
    const p = this.world(c.target); icon.position.set(p.x, p.y); this.root.addChild(icon);
    const duration = delay + phase * 2;
    this.run(duration, progress => {
      const age = progress * duration;
      const a = age < delay ? 0 : age < delay + phase ? (age - delay) / phase : Math.max(0, 1 - (age - delay - phase) / phase);
      icon.alpha = a; icon.scale.set(base * (0.5 + Math.min(1, Math.max(0, (age - delay) / (phase || 1))) * 0.5));
    }, () => icon.destroy(), c.sourceId, 'power-influence');
  }
  /** Independent of the disappearing nuke's view. Snapshots never replay a blast. */
  nukeBlast(position: Point): void {
    const p = this.world(position);
    const root = new Container({ label: 'nuke-blast' }); this.root.addChild(root);
    const flash = new Graphics().rect(-this.cell / 2, -this.cell / 2, 50 * this.cell, 50 * this.cell).fill(0xff0000);
    flash.blendMode = 'add'; root.addChild(flash);
    const glow = this.sprite(this.assets.glow, 'nuke-explosion', 1);
    if (glow) { glow.position.set(p.x, p.y); glow.tint = 0xff2222; glow.blendMode = 'add'; root.addChild(glow); }
    this.run(this.tick, t => {
      const ease = 1 - (1 - t) ** 2; flash.alpha = 0.7 * (1 - ease);
      if (glow) { glow.setSize(1 + (glow.texture.width * 20 - 1) * ease); glow.alpha = 1 - t; }
    }, () => root.destroy({ children: true }));
  }
}
