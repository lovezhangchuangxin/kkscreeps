/**
 * 渲染描述符的纯逻辑面（方案 §2.3：表在编译期存在、无运行时求值）。
 *
 * 与官方 metadata 的对应：PartDecl ≈ processor 声明（values ≈
 * calculations+payload 的派生值，keys ≈ props 差量声明）。差量实现为
 * **字段级值相等门控**：values 每 tick 重算（纯函数、微秒级），节点仅在
 * 值变化时重应用——比官方 props 深比较更强（键不变但派生值变也能捕获），
 * 键声明保留为文档与审计面（单测断言键集与官方 props 对齐）。
 *
 * 本文件与 values/specs 同族必须保持零 pixi 导入（Vitest 单测直采）。
 */
import type { RoomObjectState } from '@kkscreeps/shared';

/** 部件计算出的普通值（仅原始值与数值数组——参与相等判定与单测断言）。 */
export type PartValue = number | string | boolean | readonly number[];
export type PartValues = Record<string, PartValue>;

/** 部件求值环境（纯数据；time=当前 gameTime——快照/增量的 time）。 */
export interface PartEnv {
  /** 世界像素/格（官方 demo=100）。 */
  readonly cell: number;
  /** 当前 gameTime（cooldown/decay/spawning 相对量的基准）。 */
  readonly time: number;
  /** 本玩家 id（userOwnerColor 模式的 own 判定；无本玩家=null）。 */
  readonly player: string | null;
  /** own 判定（playerColor 色板分支用；player===null 时恒 false）。 */
  readonly isOwn: (user: string | null | undefined) => boolean;
}

/**
 * 部件声明（纯逻辑半件）。pixi 半件（create/apply）由描述符 join 层
 * （parts-runtime.ts + descriptors/*.ts）按 partId 挂接——join 不全在
 * registry 构建期抛错（防静默缺件）。
 */
export interface PartDecl {
  /** 类型内唯一 id（join 键；官方 processor id 对应）。 */
  readonly id: string;
  /** 差量键（state 顶层字段；官方 props 对应）——文档/审计面：实际差量
   * 由值相等门控承担，键集与官方 props 的逐项对齐审计待官方 metadata
   * 入库后补测（评审 B-S1；当前 keys 为声明性文档）。 */
  readonly keys?: readonly string[];
  /**
   * 派生值。返回 null = 件不显示（官方 when=false 语义，节点隐藏保留复用）。
   * 必须是 state+env 的纯函数（同输入同输出，无副作用）。
   */
  readonly values: (state: RoomObjectState, env: PartEnv) => PartValues | null;
}

/** pixi 半件：节点构造（首次显示一次）与值应用（值变化时）。 */
export interface PartRenderer {
  create(env: PartEnv): import('pixi.js').Container;
  apply(values: PartValues, node: import('pixi.js').Container, env: PartEnv): void;
}

/** 对象类型描述符 = z 槽位 + 部件表（join 后的完整形态，pixi 侧消费）。 */
export interface ObjectDescriptor {
  /** 对象类型键（resolveTypeKey）。 */
  readonly typeKey: string;
  /** objects 层 z 槽位（Z_TABLE；road 等特殊件不经此路径）。 */
  readonly z: number;
  readonly parts: readonly PartDecl[];
}

/** 值相等判定（diff 门控）：键集一致且逐键相等（数组逐元素）。 */
export function valuesEqual(a: PartValues | null, b: PartValues | null): boolean {
  if (a === b) return true;
  if (a === null || b === null) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  for (const k of ka) {
    const va = a[k];
    const vb = b[k];
    if (Array.isArray(va) || Array.isArray(vb)) {
      if (!Array.isArray(va) || !Array.isArray(vb) || va.length !== vb.length) return false;
      for (let i = 0; i < va.length; i++) {
        if (va[i] !== vb[i]) return false;
      }
    } else if (va !== vb) {
      return false;
    }
  }
  return true;
}

// ---- 契约字段读取小帮手（world_json 投影字段直通，防御性收窄） ----

export function num(v: unknown): number {
  return typeof v === 'number' ? v : 0;
}
export function optNum(v: unknown): number | undefined {
  return typeof v === 'number' ? v : undefined;
}
export function str(v: unknown): string {
  return typeof v === 'string' ? v : '';
}
/**
 * 归属字段：官方消费面是 `user`；kk 投影冗余 `owner`（R1 §12 登记的映射点——
 * constructionSite/spawn 等夹具只有 owner）。
 */
export function ownerOf(state: RoomObjectState): string | null {
  if (typeof state.user === 'string') return state.user;
  if (typeof state.owner === 'string') return state.owner;
  return null;
}
/** 官方 store 直读（store 缺位=空仓）。 */
export function storeOf(state: RoomObjectState): Record<string, number> {
  const store = state.store;
  if (typeof store === 'object' && store !== null) {
    return store as Record<string, number>;
  }
  return {};
}
/** store 容量：官方 storeCapacity（storage/container/terminal 族）。 */
export function storeCapacityOf(state: RoomObjectState): number {
  return optNum(state.storeCapacity) ?? 0;
}
/** storeCapacityResource.energy（extension/tower/spawn/link 族容量）。 */
export function energyCapacityOf(state: RoomObjectState): number {
  const cap = state.storeCapacityResource;
  if (typeof cap === 'object' && cap !== null) {
    return num((cap as Record<string, unknown>).energy);
  }
  return 0;
}
