import type { PartValues } from '../objects/part-decl.js';
import type { Tween } from './tweens.js';

/** Explicit continuous fields; tint, body-ring topology and status flags
 * are never guessed as interpolatable numbers. */
export function continuousKeys(type: string, part: string): readonly string[] {
  if (part === 'bars') return ['otherH', 'powerH', 'energyH'];
  if (part === 'rects') return ['restSize', 'powerSize', 'energySize'];
  if (type === 'creep' && part === 'main') return ['totalR', 'powerR', 'energyR'];
  if (type === 'resource' && part === 'circle') return ['radius'];
  if (type === 's:source' && part === 'energyRect') return ['size'];
  if (type === 'spawn' && part === 'core') return ['resourceScale', 'arcAngle'];
  if (type === 's:tower' && part === 'rotatable') return ['height', 'radius'];
  if (type === 's:lab' && part === 'energyRect') return ['width'];
  if (
    part === 'energy' ||
    part === 'mineralActive' ||
    part === 'resourceCircle' ||
    (type === 's:extension' && part === 'circle')
  )
    return ['scale'];
  if (part === 'gRect') return ['width'];
  if (part === 'arc' || part === 'progress') return ['angle'];
  return [];
}
export class ValueTween implements Tween {
  private elapsed = 0;
  constructor(
    private readonly from: PartValues,
    private readonly to: PartValues,
    private readonly keys: readonly string[],
    private readonly duration: number,
    private readonly write: (values: PartValues) => void,
  ) {}
  update(delta: number): boolean {
    this.elapsed = Math.min(this.duration, this.elapsed + delta);
    const p = this.duration > 0 ? this.elapsed / this.duration : 1;
    if (p >= 1) {
      this.finish();
      return true;
    }
    const values: PartValues = { ...this.to };
    for (const key of this.keys) {
      const a = this.from[key],
        b = this.to[key];
      if (
        typeof a === 'number' &&
        typeof b === 'number' &&
        Number.isFinite(a) &&
        Number.isFinite(b)
      )
        values[key] = a + (b - a) * p;
    }
    // Keep a draining bar until the final frame instead of hiding it early.
    for (const key of Object.keys(values))
      if (key.startsWith('has') && this.from[key] === true) values[key] = true;
    this.write(values);
    return false;
  }
  finish(): void {
    this.write(this.to);
  }
}
