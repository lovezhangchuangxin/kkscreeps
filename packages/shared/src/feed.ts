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
  /** 私有（浅灰，仅本玩家对象可见；公开为红）。 */
  private: boolean;
}

/** Explicit renderer adapter, not the engine's compressed u16 action bitmap.
 * Coordinates are room cells. Optional targetId associates a coordinate with a view.
 * Malformed entries received from untyped transports are ignored by the resolver. */
export interface ActionTarget {
  x: number;
  y: number;
  targetId?: string;
}
export interface ActionLogEntry {
  id: string;
  actionLog: {
    attack?: ActionTarget;
    rangedAttack?: ActionTarget;
    heal?: ActionTarget;
    rangedHeal?: ActionTarget;
    harvest?: ActionTarget;
    build?: ActionTarget;
    repair?: ActionTarget;
    upgradeController?: ActionTarget;
    reserveController?: ActionTarget;
    attackController?: ActionTarget;
    claimController?: ActionTarget;
    transferEnergy?: ActionTarget;
    power?: ActionTarget & { powerId?: number };
    runReaction?: { x1: number; y1: number; x2: number; y2: number };
    reverseReaction?: { x1: number; y1: number; x2: number; y2: number };
    rangedMassAttack?: boolean;
    attacked?: boolean;
    healed?: boolean;
    say?: { message: string; isPublic: boolean };
  };
}

/** 增量（每 tick 推送）。 */
export interface RoomDelta {
  time: number;
  room: string;
  upsert: RoomObjectState[];
  removedIds: string[];
  /** 官方 EVENT 形状（动效第一通道）。 */
  events?: RoomEvent[];
  /** 明确形状的动作适配通道；引擎 u16 位图不能直接填入。 */
  actionLogs?: ActionLogEntry[];
  /** 预留：说话通道（引擎缺口 §8.2，G4）。 */
  says?: SayEntry[];
}

/**
 * 玩家绘制（RoomVisual API 的序列化形）。样式字段与默认值对齐官方 API 文档
 * **仓库源** screeps/docs `api/source/RoomVisual.md`（2026-09-30 二轮取证勘正：
 * docs.screeps.com 整页锚点抓取会命中同页 Game.map.visual 的表——radius 10/
 * strokeWidth 0.5 系其值；仓库源权威值 circle radius 0.15、形状 strokeWidth
 * 0.1、text font 分形态/padding 0.3/opacity 1.0。外部复审 GPT-P1 坐实，
 * engine `src/game/rooms.js:1146-1218` 只透传玩家原样 style，默认值由渲染端
 * 应用=我方同位），灰区表 G1 盯梢。
 * 字段名与官方 1:1——玩家按官方文档写的 style 对象可直传；各原语只消费
 * 自己的字段子集，且**默认值逐原语不对称**（映射层逐原语应用，渲染端同位）：
 *   line:   width 0.1 · color #ffffff · opacity 0.5 · lineStyle undefined(实线)
 *   rect:   fill #ffffff · opacity 0.5 · stroke undefined(无) · strokeWidth 0.1
 *   circle: radius 0.15 · fill #ffffff · opacity 0.5 · stroke undefined · strokeWidth 0.1
 *   poly:   fill undefined(无填) · stroke #ffffff · strokeWidth 0.1 · opacity 0.5
 *   text:   color #ffffff · font 0.8（官方未载缺省，我方取官方示例值，G1）·
 *           stroke undefined · strokeWidth 0.15 · backgroundColor undefined ·
 *           backgroundPadding 0.3 · align center · opacity 1.0
 * 长度/坐标单位均为游戏坐标（1 格=1 单位，渲染端 ×cellSize 转像素）。官方
 * 明载：全部坐标以格中心计（(10,10)=该格 creep 中心）、浮点合法、rect 的
 * x/y 为左上角点、text 的 x/y 为基线点、背景启用时文字垂直居中（默认基线）。
 * 颜色格式：官方为 **any web format**——本契约支持 #rgb/#rgba/#rrggbb/
 * #rrggbbaa、rgb()/rgba()、hsl()/hsla()、CSS 命名色全表、'transparent'
 * （渲染端解析；残余系统色等退该字段默认并登记 G1）。lineStyle 仅除 text 外
 * 可声明（undefined=实线，'dashed'|'dotted'）。
 */
export interface VisualStyle {
  /** 线宽（游戏坐标；line 默认 0.1）。 */
  width?: number;
  /** 线色（line）与文字色（text）；any web format，默认 #ffffff。 */
  color?: string;
  /** 填充色：rect/circle 默认 #ffffff，poly 默认无填充。 */
  fill?: string;
  /** 描边色：poly 默认 #ffffff，rect/circle/text 默认无描边。 */
  stroke?: string;
  /** 描边宽（游戏坐标）：形状默认 0.1，text 默认 0.15。 */
  strokeWidth?: number;
  /** 不透明度：形状默认 0.5，text 默认 1.0。 */
  opacity?: number;
  /** 线型：undefined=实线。 */
  lineStyle?: 'dashed' | 'dotted';
  /** circle 半径（游戏坐标，默认 0.15）。 */
  radius?: number;
  /**
   * text 字体（官方 font 字段，number|string）：
   *   0.7 —— 游戏坐标相对尺寸；
   *   '20px' —— 绝对像素；
   *   '0.7 serif' / 'bold italic 1.5 Times New Roman' —— CSS 字体串
   *   （style/variant/weight 前缀 + 尺寸 + 族）。
   * 官方未载缺省值——渲染端取 0.8 游戏坐标（官方示例值，G1 登记）。
   */
  font?: number | string;
  /** 文字背景色（默认无背景）。**背景启用时文字垂直居中**（官方明载；
   * 默认按基线点定位）。 */
  backgroundColor?: string;
  /** 文字背景内边距（游戏坐标，默认 0.3）。 */
  backgroundPadding?: number;
  /** 文字对齐（默认 center）。 */
  align?: 'left' | 'center' | 'right';
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
  /** 半径在 style.radius（游戏坐标，默认 10——官方 circle(x,y,style) 同形）。 */
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

/** 结构最小守卫（isSnapshot/isDelta 同纪律——只验形状不验值域；样式由渲染
 * 端映射层防御性处理）。几何坐标须有限值：Infinity/NaN 会使下游几何/分段
 * 路径失控（外部复审 GPT-P1：dashSegments 对无限段长死循环）。 */
export function isRoomVisual(value: unknown): value is RoomVisual {
  if (typeof value !== 'object' || value === null) return false;
  const v = value as RoomVisual;
  const num = (n: unknown): boolean => typeof n === 'number' && Number.isFinite(n);
  const isPoint = (p: unknown): boolean =>
    typeof p === 'object' && p !== null && num((p as { x: unknown }).x) && num((p as { y: unknown }).y);
  switch (v.type) {
    case 'line':
      return Array.isArray(v.points) && v.points.length === 2 && v.points.every(isPoint);
    case 'circle':
      return num(v.x) && num(v.y);
    case 'rect':
      return num(v.x) && num(v.y) && num(v.w) && num(v.h);
    case 'poly':
      return Array.isArray(v.points) && v.points.length >= 1 && v.points.every(isPoint);
    case 'text':
      return num(v.x) && num(v.y) && typeof v.text === 'string';
    default:
      return false;
  }
}

/** RoomFeed：与后端变更集同构的输入接口（渲染器只对契约编程）。
 * 顺序契约（备忘）：**先 applySnapshot 后 setVisuals**——快照清空该房
 * visuals（每 tick 全量下发语义），先 setVisuals 会被随后的快照丢弃。 */
export interface RoomFeed {
  setTerrain(room: string, terrain: TerrainData): void;
  applySnapshot(snapshot: RoomSnapshot): void;
  applyDelta(delta: RoomDelta): void;
  setVisuals(room: string, visuals: readonly RoomVisual[]): void;
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
