// screeps-ok 适配器（kkscreeps M0）。
//
// 形态：导出 createAdapter() 的 ESM 模块 + 同目录 parity.json（fixture 约定）。
// 经 `SCREEPS_OK_ADAPTER=<本文件编译产物>` 接入 screeps-ok 套件；
// 引擎侧走 `kkscreeps embed` 子进程（stdin/stdout JSON-RPC，ADR-010）。
//
// M0 基线：createShard/placeXxx（哑世界精确存取）/runPlayer（真实 deno_core 沙箱，
// 恰好推进 1 tick）/tick/getObject/findInRoom/getGameTime/captureConsoleLogs/
// getControllerPos 已实装；游戏规则与 actionLog/raid 类接口未实装（capability 如实声明）。

import { client } from './rpc.js';
import { capabilities, type AdapterCapabilities } from './capabilities.js';

// ---- screeps-ok 契约类型（结构等价的本地声明；规范以 screeps-ok src/adapter.ts 为准） ----

export interface ShardSpec {
  /** 官方契约（adapter.ts:16-19）：玩家可以是字符串 handle 或 PlayerSpec 对象。 */
  players?: Array<string | { name: string; gcl?: unknown; power?: unknown }>;
  rooms?: Array<{
    name: string;
    rcl?: number;
    owner?: string;
    controller?: boolean;
    terrain?: number[];
  }>;
}

export interface PlaceSpec {
  /** 引擎侧必填（缺 pos / 越界坐标会被拒绝）——上游 PlaceSpec 契约。 */
  pos: [number, number];
  owner?: string;
  [key: string]: unknown;
}

export type PlayerCode = string;

/** 上游 TickOptions（screeps-ok src/adapter.ts）的已实装子集：signal（AbortSignal）。
 *  random 注入要求 randomInjection capability（本适配器为 false，未声明）。 */
export interface TickOptions {
  signal?: AbortSignal;
}

export interface PlayerRunResult {
  executed: boolean;
  expr: { ok: true; value: unknown } | { ok: false; kind: string; error: string };
  console: string[];
}

// RunPlayerError：与 screeps-ok src/errors.ts 相同的 Symbol.for 标记
// （上游专门为外部适配器做了 Symbol.hasInstance 判别，无需共享模块图）。
const MARKER = Symbol.for('screeps-ok.RunPlayerError');

export class RunPlayerError extends Error {
  readonly engineMessage: string;
  readonly errorKind: 'syntax' | 'runtime' | 'serialization';
  readonly [MARKER] = true;

  constructor(errorKind: 'syntax' | 'runtime' | 'serialization', engineMessage: string) {
    super(`RunPlayerError [${errorKind}]: ${engineMessage}`);
    this.name = 'RunPlayerError';
    this.errorKind = errorKind;
    this.engineMessage = engineMessage;
  }

  static [Symbol.hasInstance](v: unknown): boolean {
    return (
      typeof v === 'object' &&
      v !== null &&
      (v as Record<symbol, unknown>)[MARKER] === true
    );
  }
}

export class KkscreepsAdapter {
  readonly capabilities: AdapterCapabilities = capabilities;

  async createShard(spec: ShardSpec): Promise<void> {
    await client().call('createShard', spec);
  }

  private async place(kind: string, room: string, spec: PlaceSpec): Promise<string> {
    const r = await client().call<{ id: string }>('placeObject', { room, type: kind, spec });
    return r.id;
  }

  placeCreep(room: string, spec: PlaceSpec): Promise<string> { return this.place('creep', room, spec); }
  placeStructure(room: string, spec: PlaceSpec): Promise<string> { return this.place('structure', room, spec); }
  placeSite(room: string, spec: PlaceSpec): Promise<string> { return this.place('constructionSite', room, spec); }
  placeSource(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('structure', room, { structureType: 'source', ...spec });
  }
  placeMineral(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('structure', room, { structureType: 'mineral', ...spec });
  }
  placeFlag(room: string, spec: PlaceSpec): Promise<string> { return this.place('flag', room, spec); }
  placeTombstone(room: string, spec: PlaceSpec): Promise<string> { return this.place('tombstone', room, spec); }
  placeRuin(room: string, spec: PlaceSpec): Promise<string> { return this.place('ruin', room, spec); }
  placeDroppedResource(room: string, spec: PlaceSpec): Promise<string> { return this.place('resource', room, spec); }
  placePowerCreep(room: string, spec: PlaceSpec): Promise<string> { return this.place('powerCreep', room, spec); }
  placeNuke(room: string, spec: PlaceSpec): Promise<string> { return this.place('nuke', room, spec); }
  placeMarketOrder(_spec: PlaceSpec): Promise<string> {
    return Promise.reject(new Error('placeMarketOrder 未实装（M0）'));
  }
  placeObject<T extends string>(room: string, type: T, spec: PlaceSpec): Promise<string> {
    return this.place(type, room, spec);
  }

  async setTerrain(room: string, terrain: number[]): Promise<void> {
    await client().call('setTerrain', { room, terrain });
  }

  async runPlayer(userId: string, playerCode: PlayerCode): Promise<unknown> {
    const r = await client().call<PlayerRunResult>('runPlayer', { handle: userId, code: String(playerCode) });
    return mapRunResult(r);
  }

  async runPlayers(codesByUser: Record<string, PlayerCode>): Promise<Record<string, unknown>> {
    const codes: Record<string, string> = {};
    for (const [k, v] of Object.entries(codesByUser)) codes[k] = String(v);
    const r = await client().call<Record<string, PlayerRunResult>>('runPlayers', { codes });
    const out: Record<string, unknown> = {};
    for (const [handle, run] of Object.entries(r)) out[handle] = mapRunResult(run);
    return out;
  }

  async tick(count = 1, options?: TickOptions): Promise<void> {
    // 官方契约（adapter.ts:338-342）：signal 每 tick 前检查，中止即抛 reason、
    // 不再开始下一 tick——真结算有 per-tick 循环，超时围栏必须可中止
    const signal = options?.signal;
    if (!signal) {
      await client().call('tick', { count });
      return;
    }
    for (let i = 0; i < count; i++) {
      if (signal.aborted) throw signal.reason ?? new Error('tick 序列已中止');
      await client().call('tick', { count: 1 });
    }
  }

  async getObject(id: string): Promise<unknown> {
    return await client().call('getObject', { id });
  }

  async findInRoom(room: string, type: number): Promise<unknown[]> {
    return await client().call<unknown[]>('findInRoom', { room, type });
  }

  async getGameTime(): Promise<number> {
    return await client().call<number>('getGameTime', {});
  }

  async captureActionLog(_room: string): Promise<never> {
    throw new Error('captureActionLog 未实装（M0；actionLogCapture capability = false）');
  }

  async setInvaderRaidState(): Promise<never> {
    throw new Error('invader raid spawner 未实装（capability = false）');
  }
  async runInvaderRaidSpawner(): Promise<never> {
    throw new Error('invader raid spawner 未实装（capability = false）');
  }
  async clearInvaderRaidCreeps(): Promise<never> {
    throw new Error('invader raid spawner 未实装（capability = false）');
  }

  async getControllerPos(room: string): Promise<{ x: number; y: number } | null> {
    return await client().call<{ x: number; y: number } | null>('getControllerPos', { room });
  }

  async captureConsoleLogs(handle: string): Promise<string[]> {
    return await client().call<string[]>('captureConsoleLogs', { handle });
  }

  async teardown(): Promise<void> {
    // 世界重置、沙箱回收；子进程保留复用（进程退出时统一回收）
    await client().reset();
  }
}

function mapRunResult(r: PlayerRunResult): unknown {
  if (!r.executed) {
    // 契约：代码未被执行 ≠ undefined —— 抛普通 Error（不是 RunPlayerError）
    throw new Error(`玩家代码未被执行: ${r.expr && 'error' in r.expr ? r.expr.error : '未知原因'}`);
  }
  if (r.expr.ok) return r.expr.value;
  const kind = r.expr.kind as 'syntax' | 'runtime' | 'serialization';
  throw new RunPlayerError(kind, r.expr.error);
}

export function createAdapter(): Promise<KkscreepsAdapter> {
  return Promise.resolve(new KkscreepsAdapter());
}
