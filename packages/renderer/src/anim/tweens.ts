/**
 * tween 池（R4 最小核——官方 engine/src/lib/actions 的语义等价物）。
 *
 * 官方语义锚：
 *   - MoveTo 是「追驰式」积分（speed=(target−pos)/restTime·delta，
 *     MoveTo.js:18-26），外包 Ease(EASE_IN_OUT_QUAD, Ease.js) 后解析等价于
 *     **pos = lerp(start, target, easeInOutQuad(t/T))**（Ease 把原始帧时间
 *     映射为缓动时间，位置对缓动时间线性）；
 *   - RotateTo 走最短角路径（RotateTo.js:17-24 逐帧归一到 ±π 内）线性插值；
 *   - FadeOut 线性（FadeOut.js）；
 *   - 动作按**真实帧时间**推进（ActionManager._getDeltaMs 用 Date.now 差值，
 *     ActionManager.js:72-79）——tick 数据断流动画照跑（〔官拆〕§7）；
 *   - 重触发语义（登记偏差，评审 A-S1）：官方 metadata.actions 重触发走
 *     finishActions 吸附旧终点再起新段（GameObject.js:139-146、
 *     World.js:322-327、ActionManager.js:44-49）；我方 cancel 保留当前位
 *     平滑续走——影响仅限动画中途收到新位移。原引证 moveTo.js:38-41 是
 *     无任何 metadata 引用的 processor（死路径）。
 *
 * 时长单位 ms（官方 TimeableAction 构造 ×1000 同口径）。零 pixi 导入——
 * 目标经闭包/普通对象注入，Vitest 直采。
 */

/** easeInOutQuad（官方 easingHelper.js easeInOut(2)：t<0.5 ? 2t² : 1−2(1−t)²）。 */
export function easeInOutQuad(t: number): number {
  return t < 0.5 ? 2 * t * t : 1 - 2 * (1 - t) * (1 - t);
}

/** 最短角差（(−π, π]）：RotateTo.js:17-24 的归一等价。 */
export function shortestAngleDelta(from: number, to: number): number {
  let d = (to - from) % (2 * Math.PI);
  if (d > Math.PI) d -= 2 * Math.PI;
  if (d < -Math.PI) d += 2 * Math.PI;
  return d;
}

export interface Tween {
  /** 推进一帧；返回 true=结束（结束前已自行 finish 到终态）。 */
  update(deltaMS: number): boolean;
  /** 立即到终态（不触发 onEnd——cancel 与提前收尾共用）。 */
  finish(): void;
}

/** 位移动画：easeInOutQuad 位置插值（官方 Ease(MoveTo) 解析等价）。 */
export class MoveTween implements Tween {
  private elapsed = 0;

  constructor(
    private readonly setPos: (x: number, y: number) => void,
    private readonly fromX: number,
    private readonly fromY: number,
    private readonly toX: number,
    private readonly toY: number,
    private readonly durationMS: number,
  ) {}

  update(deltaMS: number): boolean {
    this.elapsed = Math.min(this.elapsed + deltaMS, this.durationMS);
    const t = this.durationMS > 0 ? easeInOutQuad(this.elapsed / this.durationMS) : 1;
    this.setPos(this.fromX + (this.toX - this.fromX) * t, this.fromY + (this.toY - this.fromY) * t);
    if (this.elapsed >= this.durationMS) {
      this.finish();
      return true;
    }
    return false;
  }

  finish(): void {
    this.setPos(this.toX, this.toY);
  }
}

/** 旋转动画：最短角路径线性插值（官方 RotateTo，无 Ease 包裹）。 */
export class RotateTween implements Tween {
  private elapsed = 0;
  private readonly delta: number;

  constructor(
    private readonly setRotation: (r: number) => void,
    private readonly base: number,
    to: number,
    private readonly durationMS: number,
  ) {
    this.delta = shortestAngleDelta(base, to);
  }

  update(deltaMS: number): boolean {
    this.elapsed = Math.min(this.elapsed + deltaMS, this.durationMS);
    const t = this.durationMS > 0 ? this.elapsed / this.durationMS : 1;
    this.setRotation(this.base + this.delta * t);
    if (this.elapsed >= this.durationMS) {
      this.finish();
      return true;
    }
    return false;
  }

  finish(): void {
    this.setRotation(this.base + this.delta);
  }
}

/** 线性 alpha 动画（官方 FadeOut/AlphaTo；死亡淡出=1→0）。 */
export class AlphaTween implements Tween {
  private elapsed = 0;

  constructor(
    private readonly setAlpha: (a: number) => void,
    private readonly from: number,
    private readonly to: number,
    private readonly durationMS: number,
  ) {}

  update(deltaMS: number): boolean {
    this.elapsed = Math.min(this.elapsed + deltaMS, this.durationMS);
    const t = this.durationMS > 0 ? this.elapsed / this.durationMS : 1;
    this.setAlpha(this.from + (this.to - this.from) * t);
    if (this.elapsed >= this.durationMS) {
      this.finish();
      return true;
    }
    return false;
  }

  finish(): void {
    this.setAlpha(this.to);
  }
}

/**
 * 分段 alpha 动画（受击罩：0→0.5@1/4→0@1，creepActions.js:163-171 的
 * Sequence[DelayTime, AlphaTo, AlphaTo] 解析等价）。keyframes=[进度分数, α]
 * 升序，首尾自动补 (0, from)/(1, last)。
 */
export class AlphaKeyframesTween implements Tween {
  private elapsed = 0;

  constructor(
    private readonly setAlpha: (a: number) => void,
    private readonly keyframes: ReadonlyArray<readonly [number, number]>,
    private readonly durationMS: number,
  ) {}

  update(deltaMS: number): boolean {
    this.elapsed = Math.min(this.elapsed + deltaMS, this.durationMS);
    const f = this.durationMS > 0 ? this.elapsed / this.durationMS : 1;
    this.setAlpha(this.sample(f));
    if (this.elapsed >= this.durationMS) {
      this.finish();
      return true;
    }
    return false;
  }

  finish(): void {
    const last = this.keyframes[this.keyframes.length - 1];
    this.setAlpha(last ? last[1] : 0);
  }

  private sample(f: number): number {
    const kf = this.keyframes;
    if (kf.length === 0) return 0;
    if (f <= kf[0]![0]) {
      // 首键前线性自 0 起（官方 DelayTime 段 α 保持 0）
      return kf[0]![0] > 0 ? (f / kf[0]![0]) * kf[0]![1] : kf[0]![1];
    }
    for (let i = 1; i < kf.length; i++) {
      const [f1, a1] = kf[i]!;
      const [f0, a0] = kf[i - 1]!;
      if (f <= f1) return f1 > f0 ? a0 + ((a1 - a0) * (f - f0)) / (f1 - f0) : a1;
    }
    return kf[kf.length - 1]![1];
  }
}

/**
 * tween 管理器（官方 ActionManager 语义：run/cancel/按真实帧时间 update；
 * cancel 只摘除不 finish——位移停在当前处，下一段移动自此起算）。onEnd 仅
 * 自然结束触发（cancel 不触发——死亡淡出被取消时不销毁视图）。
 */
export class TweenManager {
  private readonly active = new Map<Tween, (() => void) | undefined>();
  /** 迭代期间的增删走缓冲（update 内 cancel 自身/新增的防御）。 */
  private iterating = false;
  private readonly pending: Array<[Tween, (() => void) | undefined]> = [];

  /** 运行一个 tween；同引用重复 run 忽略（ActionHandle 键控语义）。 */
  run(tween: Tween, onEnd?: () => void): void {
    if (this.active.has(tween)) return;
    if (this.iterating) {
      this.pending.push([tween, onEnd]);
      return;
    }
    this.active.set(tween, onEnd);
  }

  cancel(tween: Tween): void {
    this.active.delete(tween);
    // 迭代中 cancel 的 tween 不得经 pending 补录复活（评审 C建议5）
    if (this.pending.length > 0) {
      const i = this.pending.findIndex(([t]) => t === tween);
      if (i >= 0) this.pending.splice(i, 1);
    }
  }

  /** 是否有活动 tween（perf：无动画零开销路径）。 */
  get size(): number {
    return this.active.size + this.pending.length;
  }

  update(deltaMS: number): void {
    if (this.active.size === 0) return;
    this.iterating = true;
    try {
      // JS Map 迭代容忍迭代中 delete（未访问项跳过、当前项安全）——
      // :212 守卫承担取消语义，免每帧快照数组分配（评审 C建议1）
      for (const [tween, onEnd] of this.active) {
        if (!this.active.has(tween)) continue; // 迭代中被 cancel
        if (tween.update(deltaMS)) {
          this.active.delete(tween);
          onEnd?.();
        }
      }
    } finally {
      this.iterating = false;
    }
    // 迭代期间新 run 的补录
    while (this.pending.length > 0) {
      const [tween, onEnd] = this.pending.shift()!;
      if (!this.active.has(tween)) this.active.set(tween, onEnd);
    }
  }
}
