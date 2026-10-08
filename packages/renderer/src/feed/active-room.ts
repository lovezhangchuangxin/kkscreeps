/**
 * 活动房间选择（R6 D3）：展示策略层——FeedStore 保持纯数据仓库（多房状态
 * 照常入库），本控制器决定哪些变更触达场景。
 *
 * 语义（契约，TSDoc 面 RendererHandle.setActiveRoom/activeRoom 同源）：
 *   - 非活动房变更零触达（**离房零 Pixi 写入**——数据面照收：time/对象/
 *     visuals 全部推进仓库）；
 *   - 首个 applySnapshot 自动激活一次（activeRoom 从 null → 首房），此后只
 *     显式切换；
 *   - 切房 = 该房当前仓库状态以快照语义重建（terrain + objects + visuals；
 *     复用「快照清瞬态防幻影移动」纪律——跨房状态跳变不得补间）；
 *   - **离房期间的事件不补播**（events 随增量消费，离房即弃；切回只见当前
 *     状态——官方观战切房同表现）。
 * 纯普通数据编排，无 Pixi 依赖（ADR-009）；单测直采。
 */
import type { FeedChange } from './room-feed.js';

export interface ActiveRoomHost {
  /** 增量/快照变更触达场景（仅活动房会被调用）。 */
  apply(room: string, change: FeedChange): void;
  /** 活动房全量重建（该房当前仓库状态，快照语义）。 */
  rebuild(room: string): void;
}

export class ActiveRoomController {
  private active: string | null = null;

  constructor(
    private readonly host: ActiveRoomHost,
    /** 房间是否已有数据（未知房切换 fail-fast）。 */
    private readonly hasRoom: (name: string) => boolean,
  ) {}

  get activeRoom(): string | null {
    return this.active;
  }

  /** feed.onChange 的过滤入口：首个快照自动激活一次；非活动房变更丢弃。 */
  onChange(room: string, change: FeedChange): void {
    if (this.active === null && change.kind === 'snapshot') this.active = room;
    if (room !== this.active) return;
    this.host.apply(room, change);
  }

  /** 显式切换活动房；未知房间抛错（fail-fast 同 feed 纪律）；同房幂等。 */
  setActive(room: string): void {
    if (!this.hasRoom(room))
      throw new Error(`setActiveRoom 于未知房间 ${room}（房间须先有 feed 数据）`);
    if (room === this.active) return;
    this.active = room;
    this.host.rebuild(room);
  }
}
