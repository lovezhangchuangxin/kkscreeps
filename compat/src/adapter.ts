// 将 screeps-ok 的测试操作转为 kkscreeps embed 的逐行 JSON 请求。
//
// createAdapter 创建包装对象，同一 JavaScript 全局中的实例共享子进程与世界状态。
// 测试应串行使用并在结束时 teardown；teardown 重置世界，但保留进程。
// 编译产物通过 SCREEPS_OK_ADAPTER 接入套件，同目录 parity.json 声明差异与跳过项。
// 上游接口来源：锁定版本 screeps-ok/src/adapter.ts；进程协议见 ADR-010。

import { client } from './rpc.js';
import { capabilities, type AdapterCapabilities } from './capabilities.js';

// 本地类型描述当前包装接口；完整上游契约以 screeps-ok/src/adapter.ts 为准。

export interface ShardSpec {
  /** 玩家可以是字符串 handle 或 PlayerSpec 对象。来源：screeps-ok/src/adapter.ts 的 ShardSpec。 */
  players: Array<string | { name: string; gcl?: unknown; power?: unknown }>;
  rooms: Array<{
    name: string;
    rcl?: number;
    owner?: string;
    controller?: boolean;
    terrain?: number[];
  }>;
}

export interface PlaceSpec {
  /** 房间对象的位置，x、y 均为 0..49 的整数；市场订单注入不读取此字段。 */
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
  /** 注入测试订单，绕过玩家 API 的创建资格检查与费用。
   * price 单位为 credits，createdTimestamp 为 Unix 时间毫秒数。
   * 字段来源：screeps-ok/src/adapter.ts 的 MarketOrderSpec。 */
  async placeMarketOrder(spec: PlaceSpec): Promise<string> {
    const r = await client().call<{ id: string }>('placeMarketOrder', { spec });
    return r.id;
  }
  placeObject<T extends string>(room: string, type: T, spec: PlaceSpec): Promise<string> {
    return this.place(type, room, spec);
  }

  async setTerrain(room: string, terrain: number[]): Promise<void> {
    await client().call('setTerrain', { room, terrain });
  }

  /** 求值并结算一轮后返回表达式结果；玩家错误可能在世界推进后才抛出。 */
  async runPlayer(userId: string, playerCode: PlayerCode): Promise<unknown> {
    const r = await client().call<PlayerRunResult>('runPlayer', { handle: userId, code: String(playerCode) });
    return mapRunResult(r);
  }

  /** 各玩家观察同一轮世界，合并指令后结算一次；玩家错误会拒绝整个 Promise。
   * 世界结算失败时，重试复用整轮缓存结果并忽略新代码，规则见 EmbedEngine::run_players。 */
  async runPlayers(codesByUser: Record<string, PlayerCode>): Promise<Record<string, unknown>> {
    const codes: Record<string, string> = {};
    for (const [k, v] of Object.entries(codesByUser)) codes[k] = String(v);
    const r = await client().call<Record<string, PlayerRunResult>>('runPlayers', { codes });
    const out: Record<string, unknown> = {};
    for (const [handle, run] of Object.entries(r)) out[handle] = mapRunResult(run);
    return out;
  }

  async tick(count = 1, options?: TickOptions): Promise<void> {
    // 有 signal 时逐 tick 发请求，在下一 tick 开始前检查中止；不取消正在处理的 RPC。
    // 来源：screeps-ok/src/adapter.ts 的 TickOptions.signal。
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
