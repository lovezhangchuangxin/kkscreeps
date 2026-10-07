/**
 * creep body 环算法（官方 engine/src/lib/processors/creepBuildBody.js 的
 * 纯逻辑移植——零 pixi，Vitest 直采）。
 *
 * 官方算法（:84-157）：
 *   1. 过滤 tough/carry/hits≤0（:108-109）；
 *   2. 同类型合并 hits（:111-119）；
 *   3. 合并后按 hits **升序**（:121-129——BODY_PARTS.order 是死代码）；
 *   4. 非 move 自头顶（frontAngle=0）向两侧对称展开、move（backSide）自正下
 *      （backAngle=π）展开（:131-152）；每部件画一对镜像弧：
 *      (ANGLE_SHIFT+s → ANGLE_SHIFT+s+a, 顺时针) 与
 *      (ANGLE_SHIFT−s → ANGLE_SHIFT−(s+a), 逆时针)；
 *   5. 每部件弧长 = PART_ANGLE × hits/100。
 */
import { CREEP_SPEC } from './specs.js';

/** 一条弧段（draw 顺序即官方 bodySprites 追加序）。 */
export interface RingArc {
  color: number;
  startAngle: number;
  endAngle: number;
  /** 官方 createBodyPartSprite 的 anticlockwise 参数。 */
  anticlockwise: boolean;
}

/** 夹具/契约的 body 部件最小读取面（RoomObjectState.body 直通）。 */
export interface BodyPartLike {
  type?: unknown;
  hits?: unknown;
}

/** backSide=true（move）自正下展开——官方 BODY_PARTS 表 :13-43。 */
const BACK_SIDE = new Set(['move']);

/**
 * body → 弧段序列（draw 序）。未知类型跳过（官方 logger.warn 后无色表可查
 * 同样不画——引擎 typed 面不可达，仅脏数据触发）。
 */
export function bodyRingArcs(body: readonly BodyPartLike[] | undefined): RingArc[] {
  const ring = CREEP_SPEC.ring;
  const arcs: RingArc[] = [];
  if (!Array.isArray(body)) return arcs;

  // 过滤 + 同类型合并（保序——官方 reduce 对象序=首见序）
  const merged = new Map<string, number>();
  for (const part of body) {
    const type = typeof part?.type === 'string' ? part.type : '';
    const hits = typeof part?.hits === 'number' ? part.hits : 0;
    if (type === 'tough' || type === 'carry' || hits <= 0) continue;
    merged.set(type, (merged.get(type) ?? 0) + hits);
  }

  // 合并后升序（稳定——JS sort 同值保持首见序）
  const sorted = [...merged.entries()].sort((a, b) => a[1] - b[1]);

  let frontAngle = 0;
  let backAngle = Math.PI;
  for (const [type, hits] of sorted) {
    const color = ring.colors[type];
    if (color === undefined) continue;
    const start = BACK_SIDE.has(type) ? backAngle : frontAngle;
    const sweep = ring.partAngle * (hits / ring.maxPartHits);
    // 一对镜像弧（官方两次 createBodyPartSprite 调用，:140-143）
    arcs.push({ color, startAngle: ring.angleShift + start, endAngle: ring.angleShift + start + sweep, anticlockwise: false });
    arcs.push({ color, startAngle: ring.angleShift - start, endAngle: ring.angleShift - (start + sweep), anticlockwise: true });
    if (BACK_SIDE.has(type)) backAngle += sweep;
    else frontAngle += sweep;
  }
  return arcs;
}

/** 任一 tough 存活→中心 tough 贴图（官方 :154-156）。 */
export function hasLivingTough(body: readonly BodyPartLike[] | undefined): boolean {
  if (!Array.isArray(body)) return false;
  return body.some((p) => p?.type === 'tough' && typeof p.hits === 'number' && p.hits > 0);
}
