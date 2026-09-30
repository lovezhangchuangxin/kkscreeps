/** 所有场景动画共用的时间入口；逻辑 tick 时间仅用于状态求值。
 * elapsedMS 使用 Pixi 未截断的经过时间，rate 供回放器与数据推送速率同步。 */
export class AnimationClock {
  private rate = 1;

  setRate(rate: number): void {
    if (!Number.isFinite(rate) || rate < 0) throw new RangeError('playbackRate 须为有限非负数');
    this.rate = rate;
  }

  advance(elapsedMS: number): number {
    return elapsedMS * this.rate;
  }
}
