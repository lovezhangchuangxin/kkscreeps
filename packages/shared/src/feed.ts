/**
 * RoomFeed 契约 v0 —— 渲染器与后端变更集同构的输入接口
 * （docs/architecture/frontend.md §四；F2 方案 docs/plans/f2-renderer.md §3）。
 *
 * 契约版本：**v0，未冻结**——M3 网络投影落地时校准（渲染器 feed 层薄适配
 * 吸收漂移）。形状依据 2026-09-30 对 kk-world `world_json` 投影的取证
 * （基线 e7adf47，评审 A 逐字段核对）：
 *   - 对象主键字段是 **`id`**（u64 的 `kk%016x` 字符串序列化），非官方 `_id`；
 *   - 结构类对象 `type:"structure"` + `structureType:"source"|"spawn"|…`；
 *     spawn 为 `type:"spawn"`、工地为 `type:"constructionSite"`（两者也带
 *     structureType：spawn 为自身名、工地为目标工程——structureType 是
 *     **单向蕴含**，不能当 type==="structure" 的判据）；
 *   - 掉落资源 `type:"resource"` + `resourceType`（非 "energy"——R1 评审 B
 *     更正，此前取证头误写）；
 *   - 每对象附投影字段 **`pos:{x,y,roomName}`**（world_json 投影附带，非
 *     官方形状，随信封索引签名直通）；
 *   - Generic 透传桶的 `data` JSON 在投影期**解析展开**为对象顶层键（实测
 *     夹具无嵌套 `data` 键——world_json.rs Generic 臂 JSON 合并后再补
 *     id/room/x/y/pos），R1 评审 C 复核更正；
 *   - 房间事件形状 `{"event":<常量码>,"objectId","data"}`（RoomState.events
 *     的官方 EVENT 形态，动效第一通道；EVENT 码 1..12 已实测与官方
 *     constants.js 逐项相等——kk-core/constants.rs:502-514）。
 * typed 15 类 + Generic（`data` JSON 透传 tower/rampart/lab/link/terminal 等
 * 2B 未转正类型）的字段全集不在此枚举——envelope + 索引签名承载，R3 渲染
 * 描述符按需收窄。
 */

/** 地形：2500 位掩码码（wall=1/swamp=2/lava=4），同 kk-core/terrain.rs。 */
export interface TerrainData {
  codes: number[];
}

/** 对象状态信封：世界 JSON 投影的直通形（typed 字段经索引签名透传）。 */
export interface RoomObjectState {
  /** `kk%016x` 字符串（world_json u64 序列化）。 */
  id: string;
  type: string;
  room?: string;
  x?: number;
  y?: number;
  /** 投影附带字段 pos:{x,y,roomName}（world_json 每对象附，非官方形状）。 */
  pos?: { x: number; y: number; roomName: string };
  /** 结构类的二级类型（type==="structure" 时必有；spawn/constructionSite
   * 也带——单向蕴含，见文件头注）。 */
  structureType?: string;
  /** Generic 透传桶：data JSON 在投影期解析展开后的顶层键
   * （tower/rampart/lab/link/terminal 等 2B 未转正类型——信封上无嵌套
   * data 键，见文件头注）。 */
  data?: Record<string, unknown>;
  /** 类型专属字段（body/store/hits/…）——v0 直通，R3 描述符收窄。 */
  [field: string]: unknown;
}

/** 房间事件（官方 EVENT 形态，RoomState.events 投影；动效第一通道）。 */
export interface RoomEvent {
  /** 官方 EVENT_* 常量码——已实测 kk-core 与官方 constants.js 1..12 逐项
   * 相等（EVENT_ATTACK=1 等）；TS 常量面随常量生成管线落位。 */
  event: number;
  objectId: string;
  data?: unknown;
}

/** 房间快照（首帧/切房全量）。字段=world_json 的官方 camelCase 子集。 */
export interface RoomSnapshot {
  time: number;
  room: string;
  /** 快照携带地形（烘焙门控键）。 */
  terrain: TerrainData;
  objects: RoomObjectState[];
  energyAvailable?: number;
  energyCapacityAvailable?: number;
}

/** 说话条目（G4：引擎暂无 say 事件面——通道预留，v0 仅夹具手工注入演示）。 */
export interface SayEntry {
  /** 说话者对象 id。 */
  id: string;
  message: string;
  /** 私有（官方 say 私有为浅灰气泡、公开为红）。 */
  private: boolean;
}

/** 增量（每 tick 推送）。 */
export interface RoomDelta {
  time: number;
  room: string;
  upsert: RoomObjectState[];
  removedIds: string[];
  /** 官方 EVENT 形状（动效第一通道）。 */
  events?: RoomEvent[];
  /** 预留：官方 13 键 actionLog 形态（第二通道，v0 不产出——引擎缺口 §8.1）。 */
  actionLogs?: unknown[];
  /** 预留：说话通道（引擎缺口 §8.2，G4）。 */
  says?: SayEntry[];
}

/**
 * 玩家绘制（RoomVisual API 的序列化形；样式黑盒对齐官方，灰区表 G1 盯梢）。
 * 官方 serialize 形：{type, …坐标/几何, …样式}。
 */
export interface VisualStyle {
  fill?: string;
  stroke?: string;
  lineWidth?: number;
  opacity?: number;
  lineJoin?: 'miter' | 'bevel' | 'round';
  font?: string;
  color?: string;
  align?: 'left' | 'center' | 'right';
  backgroundColor?: string;
  backgroundPadding?: number;
  strokeColor?: string;
  strokeWidth?: number;
}

export interface LineV {
  type: 'line';
  points: [{ x: number; y: number }, { x: number; y: number }];
  style?: VisualStyle;
}
export interface CircleV {
  type: 'circle';
  x: number;
  y: number;
  r?: number;
  style?: VisualStyle;
}
export interface RectV {
  type: 'rect';
  x: number;
  y: number;
  w: number;
  h: number;
  style?: VisualStyle;
}
export interface PolyV {
  type: 'poly';
  points: Array<{ x: number; y: number }>;
  style?: VisualStyle;
}
export interface TextV {
  type: 'text';
  x: number;
  y: number;
  text: string;
  style?: VisualStyle;
}
/** RoomVisual 五原语（官方 RoomVisual API：line/circle/rect/poly/text）。 */
export type RoomVisual = LineV | CircleV | RectV | PolyV | TextV;

/** RoomFeed：与后端变更集同构的输入接口（渲染器只对契约编程）。 */
export interface RoomFeed {
  setTerrain(room: string, terrain: TerrainData): void;
  applySnapshot(snapshot: RoomSnapshot): void;
  applyDelta(delta: RoomDelta): void;
  setVisuals(room: string, visuals: RoomVisual[]): void;
}

/** 回放夹具：首快照 + 增量链（方案 §3；ticks[0] 恒为快照，其余为增量）。 */
export interface FixtureFile {
  room: string;
  /** 快照+增量按时序排列；运行时判定用 isSnapshot/isDelta。 */
  ticks: [RoomSnapshot, ...RoomDelta[]];
}

// ---- 运行时判定（结构最小守卫；夹具/回放/未来网络面的入口校验用） ----

export function isRoomEvent(value: unknown): value is RoomEvent {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as RoomEvent).event === 'number' &&
    typeof (value as RoomEvent).objectId === 'string'
  );
}

export function isSnapshot(value: unknown): value is RoomSnapshot {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as RoomSnapshot).time === 'number' &&
    typeof (value as RoomSnapshot).room === 'string' &&
    Array.isArray((value as RoomSnapshot).terrain?.codes)
  );
}

export function isDelta(value: unknown): value is RoomDelta {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as RoomDelta).time === 'number' &&
    typeof (value as RoomDelta).room === 'string' &&
    Array.isArray((value as RoomDelta).upsert) &&
    Array.isArray((value as RoomDelta).removedIds)
  );
}
