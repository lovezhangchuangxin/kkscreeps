/** Official renderer a2db4a7: creepActions.js, say.js and tower/nuke metadata.
 * Seconds are converted by the room clock; tick fractions follow tickDurationMS. */
export const EFFECT_COLORS = {
  work: 0xffe533,
  attack: 0xff3333,
  heal: 0x2ce328,
  both: 0xffff33,
  ranged: 0x3c75c7,
  reserve: 0xb99cfb,
  reaction: 0xffffff,
  power: 0xcc3d3e,
} as const;
export const EFFECT_SPEC = {
  beamDuration: 0.6,
  powerDuration: 0.3,
  beamAlpha: 0.7,
  coverDuration: 0.9,
  workDelay: 0.3,
  bitePenetration: 10,
  waveRadius: 300,
  waveAlpha: 0.4,
  waveColor: 0x5d80b2,
  towerTurnMS: 300,
  towerFlareSize: 400,
  say: {
    fontSize: 60,
    height: 100,
    padding: 60,
    radius: 30,
    strokeWidth: 8,
    y: -170,
    textY: -120,
    tailY: -44,
    private: 0xcccccc,
    public: 0xdd8888,
  },
} as const;
export const HIT_COVER_TYPES = new Set([
  'creep',
  'powerCreep',
  's:tower',
  's:lab',
  's:link',
  's:invaderCore',
]);
/** PWR 1..18; effect IDs checked against Rust alongside event constants. */
export const POWER_ICONS: Readonly<Record<number, string>> = {
  1: 'generate-ops',
  2: 'operate-spawn',
  3: 'operate-tower',
  4: 'operate-storage',
  5: 'operate-lab',
  6: 'operate-extension',
  7: 'operate-observer',
  8: 'operate-terminal',
  9: 'disrupt-spawn',
  10: 'disrupt-tower',
  11: 'disrupt-source',
  12: 'shield',
  13: 'regen-source',
  14: 'regen-mineral',
  15: 'disrupt-terminal',
  16: 'operate-power',
  17: 'fortify',
  18: 'operate-controller',
};

/** getDrawShotFunc: head travels to target in the first half, tail follows in
 * the second. Its local fx is the target and tx is zero (the source). */
export function beamWindow(progress: number): readonly [number, number] {
  const p = Math.min(1, Math.max(0, progress));
  return p <= 0.5 ? [0, p * 2] : [p * 2 - 1, 1];
}
export function coverAlpha(progress: number, delay = 0): number {
  if (progress < delay || progress >= 1) return 0;
  const p = (progress - delay) / (1 - delay);
  return p < 0.25 ? p * 4 : (1 - p) / 0.75;
}
export function angleBetween(from: { x: number; y: number }, to: { x: number; y: number }): number {
  let angle = Math.atan2(to.y - from.y, to.x - from.x) + Math.PI / 2;
  if (angle > Math.PI) angle -= Math.PI * 2;
  return angle;
}
