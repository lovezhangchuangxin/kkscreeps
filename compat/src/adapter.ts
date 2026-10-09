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

/** 上游 TickOptions（screeps-ok src/adapter.ts）的已实装子集：signal
 *  （AbortSignal）与 random（注入序列，值域 [0,1)，整次 tick 调用共享，
 *  引擎处理器随机调用逐值消费、耗尽报错回滚该拍）。 */
export interface TickOptions {
  signal?: AbortSignal;
  random?: number[];
}

/** 上游 InvaderRaidRoomStateSpec：逐键种子，未提供的键保持不变。 */
export interface InvaderRaidRoomStateSpec {
  harvestedEnergy?: number;
  raidGoal?: number | null;
  active?: boolean;
  status?: string;
  controllerReservation?: { owner: string; ticksToEnd?: number } | null;
}

/** 上游 InvaderRaidSpawnerOptions：注入随机序列（消耗序同官方
 *  genInvaders 的 Math.random 调用点；耗尽报错）。 */
export interface InvaderRaidSpawnerOptions {
  random?: readonly number[];
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
    return typeof v === 'object' && v !== null && (v as Record<symbol, unknown>)[MARKER] === true;
  }
}

// 管理快照的 owner 是套件 handle；玩家 VM 中 owner.username 保持公开用户名。
// 来源：vanilla 的 NPC_HANDLES 与 snapshots.ts 的 snapOwner/resolvePlayerReverse 反向映射。
function snapshotNpcHandle(value: unknown): unknown {
  if (value === null || typeof value !== 'object') return value;
  const snapshot = value as Record<string, unknown>;
  if (snapshot.owner === 'Invader') return { ...snapshot, owner: 'sk' };
  if (snapshot.owner === 'Source Keeper') return { ...snapshot, owner: 'srcKeeper' };
  return value;
}

export class KkscreepsAdapter {
  readonly capabilities: AdapterCapabilities = capabilities;

  async createShard(spec: ShardSpec): Promise<void> {
    await client().call('createShard', spec);
  }

  private async place(kind: string, room: string, spec: PlaceSpec): Promise<string> {
    // 套件 sk/srcKeeper 对应官方用户 2/3；本引擎保存公开用户名，
    // 别名只在适配边界转换，不能让普通短玩家名获得 NPC 语义。
    // 来源：锁定 vanilla/index.ts 的 NPC_HANDLES。
    const owner =
      spec.owner === 'sk' ? 'Invader' : spec.owner === 'srcKeeper' ? 'Source Keeper' : spec.owner;
    const normalized = owner === spec.owner ? spec : { ...spec, owner };
    const r = await client().call<{ id: string }>('placeObject', {
      room,
      type: kind,
      spec: normalized,
    });
    return r.id;
  }

  placeCreep(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('creep', room, spec);
  }
  placeStructure(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('structure', room, spec);
  }
  placeSite(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('constructionSite', room, spec);
  }
  placeSource(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('structure', room, { structureType: 'source', ...spec });
  }
  placeMineral(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('structure', room, { structureType: 'mineral', ...spec });
  }
  placeFlag(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('flag', room, spec);
  }
  placeTombstone(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('tombstone', room, spec);
  }
  placeRuin(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('ruin', room, spec);
  }
  placeDroppedResource(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('resource', room, spec);
  }
  placePowerCreep(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('powerCreep', room, spec);
  }
  placeNuke(room: string, spec: PlaceSpec): Promise<string> {
    return this.place('nuke', room, spec);
  }
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
    const r = await client().call<PlayerRunResult>('runPlayer', {
      handle: userId,
      code: String(playerCode),
    });
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
    // random：tick({random}) 注入序列整次调用共享（值域校验与耗尽报错在引擎侧）。
    const { signal, random } = options ?? {};
    if (!signal) {
      await client().call('tick', random ? { count, random } : { count });
      return;
    }
    // signal 逐拍模式：每拍开始前检查中止；random 序列跨整次调用共享——
    // 逐拍按引擎回报的 randomConsumed 切片续传，不重复消费。
    let remaining = random ? [...random] : undefined;
    for (let i = 0; i < count; i++) {
      if (signal.aborted) throw signal.reason ?? new Error('tick 序列已中止');
      // 序列耗尽后仍发送 random: []（空数组）：引擎按注入序列耗尽报错
      // 回滚，与非 signal 单发模式的 exhausted 语义一致，不静默回落派生流。
      const res = await client().call<{ randomConsumed?: number }>(
        'tick',
        remaining !== undefined ? { count: 1, random: remaining } : { count: 1 },
      );
      const used = res?.randomConsumed;
      if (remaining !== undefined && typeof used === 'number' && used > 0) {
        remaining = remaining.slice(used);
      }
    }
  }

  async getObject(id: string): Promise<unknown> {
    return snapshotNpcHandle(await client().call('getObject', { id }));
  }

  async findInRoom(room: string, type: number): Promise<unknown[]> {
    const snapshots = await client().call<unknown[]>('findInRoom', { room, type });
    return snapshots.map(snapshotNpcHandle);
  }

  async getGameTime(): Promise<number> {
    return await client().call<number>('getGameTime', {});
  }

  /** 上一成功结算拍的房间动作记录导出（runPlayer/tick 结算后调用）。
   * 形状为套件 RoomActionLogCapture：{room, tick, objects}，objects 只含
   * 本拍有动作且结算尾仍存活的对象；tick 报当前世界时间（动作产物属于
   * 已推进前的那一拍，与官方 room-history 键位对齐）。 */
  async captureActionLog(room: string): Promise<unknown> {
    return await client().call('captureActionLog', { room });
  }

  async setInvaderRaidState(room: string, spec: InvaderRaidRoomStateSpec): Promise<void> {
    await client().call('setInvaderRaidState', { room, spec });
  }
  async runInvaderRaidSpawner(options?: InvaderRaidSpawnerOptions): Promise<void> {
    // random 缺省时键不发送，引擎用世界种子派生流（确定性回放）。
    await client().call('runInvaderRaidSpawner', { random: options?.random });
  }
  async clearInvaderRaidCreeps(room: string): Promise<void> {
    await client().call('clearInvaderRaidCreeps', { room });
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
