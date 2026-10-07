/**
 * 对象层舞台：feed 对象列表 → 场景图增量（方案 §2.3/§2.4）。
 *
 * - id 对账：视图按 id 池化复用，移除即拆树销毁（官方 destroy/rebuild 风暴的
 *   反面——部件节点只在首次激活时创建，值变化仅重应用）。
 * - 差量：值相等门控（valuesEqual），只有值变化的部件触达 pixi。
 * - 层路由：slot（对象根局部坐标）/ lighting（世界坐标，共享纹理成员只
 *   detach）/ effects（世界坐标）。
 * - 运动（R4）：creep/powerCreep 位移不直接落坐标——MoveTo easeInOutQuad
 *   插值（tickDuration）+ 朝向 RotateTo（0.2 tick，最短角），真实帧时间推进
 *   （数据断流动画照跑）；overlay 件（lighting/effects 层）逐帧随动 +
 *   lighting 显式 markDirty（成员值变化不触发 RT 重渲的坑——R3 C-B1）。
 * - 事件面（R4）：EVENT_ATTACK/HEAL → data.targetId → 视图受击罩（cover.svg
 *   ADD tint 红/绿/黄，α0→0.5@1/4→0@1 tick）。
 * - 死亡（R4）：运动类型移除走 FadeOut(tickDuration/2) 后销毁（官方
 *   disappearProcessor；dying 表托管，id 复用防御性立即收尾）。
 * - 附属绘制：road 连通（ground 层）、rampart 私有顶罩（effects 层）、
 *   constructedWall 格并入墙烘焙。
 */
import { Container, Graphics, Sprite, Text } from 'pixi.js';
import type { ActionLogEntry, SayEntry, RoomEvent, RoomObjectState } from '@kkscreeps/shared';
import { bakeCanvas, textureFromBaked } from '../core/bake.js';
import type { LightingPass } from '../core/lighting.js';
import { mergedOutlinePath } from '../core/path-merge.js';
import { AlphaKeyframesTween, AlphaTween, MoveTween, RotateTween, TweenManager, type Tween } from '../anim/tweens.js';
import type { PartEnv, PartValues } from './part-decl.js';
import { ownerOf, valuesEqual } from './part-decl.js';
import { OverlayGroup } from './overlay-group.js';
import { EffectsStage, type ActorBinding } from '../anim/effects-stage.js';
import { AmbientScheduler, type Viewport } from '../anim/ambient-scheduler.js';
import { PositionChannels, PropertyChannels } from '../anim/property-channels.js';
import { resolveActions, point } from '../anim/action-resolver.js';
import { HIT_COVER_TYPES } from '../anim/effect-specs.js';
import { ValueTween, continuousKeys } from '../anim/value-tween.js';
import { MOTION_TYPE_KEYS, controllerSummary } from './parts-creeps.js';
import { buildRegistry, type RegisteredDescriptor } from './registry.js';
import type { JoinAssets } from './joins-naturals.js';
import { CREEP_SPEC, ROAD_SPEC } from './specs.js';
import { TERRAIN_SPEC } from '../core/terrain-spec.js';
import { roadNodes } from './road-geometry.js';
import { rampartGroupHash, rampartGroups, rampartRasterBounds, ROOM_GRID, type RampartGroup } from './rampart-groups.js';
import { RAMPART_COVER_SPEC } from './specs.js';
import { ROOM_SIZE } from '../scenes/terrain-geometry.js';
import { resolveTypeKey } from './z-table.js';

export interface ObjectsStageOptions {
  readonly cell: number;
  readonly lighting: LightingPass;
  readonly assets: JoinAssets;
  /** 本玩家 id（userOwnerColor 色板 own 判定；null=无本玩家视角）。 */
  readonly player: string | null;
  /** objects 层容器（类型槽位挂载点）。 */
  readonly objectsLayer: Container;
  /** tick 时长 ms（G6：官方闭源壳传入值不可知——契约参数化，缺省 1000）。 */
  readonly tickDurationMS: number;
}

interface PartEntry {
  node: Container;
  prev: PartValues | null;
  sampled: PartValues | null;
  channels: PropertyChannels;
  tween?: Tween;
}

interface ObjectView {
  readonly id: string;
  readonly typeKey: string;
  readonly root: Container;
  readonly parts: Map<string, PartEntry>;
  /** 世界坐标 overlay 件（lighting/effects 层部件 + 受击罩）——运动/淡出时
   * 逐帧随 root 位移与 alpha 同步。 */
  readonly overlays: OverlayGroup;
  readonly position: PositionChannels;
  x: number;
  y: number;
  /** 运动类型（MOTION_TYPE_KEYS）——位移走插值、移除走淡出。 */
  readonly motion: boolean;
  /** 活动 tween（官方 scope.moveToAction 同位——新动作取消旧动作）。 */
  moveTween?: Tween;
  rotateTween?: Tween;
  fadeTween?: Tween;
  coverTween?: Tween;
  cover?: Container;
  /** 上次朝向目标角（官方 rotation calc 的 props 门控等价——同向移动不重触发）。 */
  lastRotation: number;
}

const SIN_45 = Math.sin(Math.PI / 4);

/**
 * 官方 calculateAngle（helpers/mathHelper.js:5-13）：atan2(dy,dx)+π/2 归一到
 * (−π,π]。sprite rotation=0 朝上，故 +π/2。
 */
function calculateAngle(x0: number, y0: number, x: number, y: number): number {
  let angle = Math.atan2(y - y0, x - x0) + Math.PI / 2;
  if (angle > Math.PI) angle -= 2 * Math.PI;
  else if (angle < -Math.PI) angle += 2 * Math.PI;
  return angle;
}

export class ObjectsStage {
  /** ground 层子容器：road 连通。 */
  readonly roadRoot = new Container({ label: 'objects-road' });
  /** effects 层子容器：rampart 顶罩（先）+ 对象 effects 部件（后）。 */
  readonly coverRoot = new Container({ label: 'objects-cover' });
  readonly effectsRoot = new Container({ label: 'objects-effects' });
  readonly effects: EffectsStage;
  readonly ambient: AmbientScheduler;
  private previous = new Map<string, RoomObjectState>();
  private previousTime = 0;

  private readonly cell: number;
  private readonly view: number;
  private readonly lighting: LightingPass;
  private readonly player: string | null;
  private readonly tickDurationMS: number;
  private readonly objectsLayer: Container;
  private readonly registry: Map<string, RegisteredDescriptor>;
  private readonly assets: JoinAssets;
  private readonly slots = new Map<string, Container>();
  private readonly views = new Map<string, ObjectView>();
  /** 死亡淡出中的视图（官方 disappearProcessor 托管期；id 不在 views）。 */
  private readonly dying = new Map<string, ObjectView>();
  private readonly tweens = new TweenManager();
  /** constructedWall 格（行主序索引）——RoomScene 并入墙烘焙；apply 后读取。
   * 集不变时保留原引用（RoomScene 引用相等跳过重烘焙）。 */
  private wallExtras: ReadonlySet<number> = new Set();
  /** 最近一次 apply 的 constructedWall 格集（只读暴露）。 */
  get constructedWallCells(): ReadonlySet<number> {
    return this.wallExtras;
  }

  private readonly coverHashes = new Map<string, string>();
  private readonly warnedTypes = new Set<string>();
  private roadKey: string | null = null;
  /** 烘焙画布边长（与地形/lighting RT 同规格；C-S4 单源）。 */
  private readonly renderSize = TERRAIN_SPEC.renderSize;

  constructor(options: ObjectsStageOptions) {
    this.cell = options.cell;
    this.view = ROOM_SIZE * options.cell;
    this.lighting = options.lighting;
    this.player = options.player;
    this.tickDurationMS = options.tickDurationMS;
    this.objectsLayer = options.objectsLayer;
    this.assets = options.assets;
    this.registry = buildRegistry(options.assets);
    this.ambient = new AmbientScheduler(this.tickDurationMS);
    this.effects = new EffectsStage(this.assets, this.lighting, this.cell, this.tickDurationMS, id => this.actorBinding(id));
    this.effectsRoot.addChild(this.effects.root);
  }

  private env(time: number, controller: PartEnv['controller']): PartEnv {
    return {
      cell: this.cell,
      time,
      player: this.player,
      isOwn: (user) => user !== null && user !== undefined && user === this.player,
      controller,
    };
  }

  private slotFor(typeKey: string, z: number): Container {
    let slot = this.slots.get(typeKey);
    if (!slot) {
      slot = new Container({ label: `slot-${typeKey}` });
      slot.zIndex = z;
      this.slots.set(typeKey, slot);
      this.objectsLayer.addChild(slot);
    }
    return slot;
  }

  /**
   * 对账入口（feed.onChange 驱动）。返回 constructedWall 格集是否变化
   * （变化时 RoomScene 需把 wallExtras 并入墙烘焙重烧）。events=本 tick 房间
   * 事件（动效第一通道；R4 消费攻击/治疗罩，R5 扩全表）。kind='snapshot'
   * 时清除瞬态并即时同步坐标，复用同 id/type 节点；缺失项立即拆除。
   * 快照重置语义与节点寿命分开，避免重放/重同步时全量拆树。
   */
  apply(
    objects: readonly RoomObjectState[],
    time: number,
    events: readonly RoomEvent[] = [],
    kind: 'snapshot' | 'delta' = 'delta',
    input: { actionLogs?: readonly ActionLogEntry[]; says?: readonly SayEntry[] } = {},
  ): boolean {
    if (kind === 'snapshot') {
      this.effects.reset();
      this.ambient.reset();
      for (const view of [...this.dying.values()]) this.destroyView(view);
      this.dying.clear();
      for (const view of this.views.values()) this.resetTransientState(view);
    }
    const extras = new Set<number>();
    for (const o of objects) {
      if (o.type === 'structure' && o.structureType === 'constructedWall'
        && typeof o.x === 'number' && typeof o.y === 'number') {
        extras.add(o.y * ROOM_GRID + o.x);
      }
    }
    const wallsChanged = !integerSetEquals(extras, this.wallExtras);
    if (wallsChanged) this.wallExtras = extras;

    this.syncRoad(objects);
    this.syncRampartCover(objects);

    // controllerSummary：O(n) 首中早退，摊在同已 O(n) 的对账趟内（评审 B建议6）
    const env = this.env(time, controllerSummary(objects));
    const wanted = new Map<string, RoomObjectState>();
    for (const o of objects) {
      if (typeof o.id === 'string') wanted.set(o.id, o);
    }

    // id 复用防御：淡出中的视图若被同 id 重建（引擎 id 单调不应发生）——
    // 立即收尾销毁，走全新创建
    for (const id of wanted.keys()) {
      const fading = this.dying.get(id);
      if (fading) this.finishFade(fading);
    }

    // 移除（快照迭代——destroy 同步 splice 活数组会跳格，R2 复审 N1 纪律）
    for (const [id, view] of [...this.views]) {
      if (!wanted.has(id)) {
        this.effects.cancelOwner(id);
        this.ambient.remove(id);
        if (view.motion && kind !== 'snapshot') {
          // 官方 disappearProcessor：FadeOut(tickDuration/2) 后销毁——
          // 移出 views 进 dying 托管
          this.beginFade(id, view);
        } else {
          this.destroyView(view);
        }
        this.views.delete(id);
      }
    }

    for (const [id, state] of wanted) {
      if (state.type === 'structure' && state.structureType === 'road') continue; // road 走连通绘制
      const typeKey = resolveTypeKey(state);
      const desc = this.registry.get(typeKey);
      if (!desc) {
        // 未注册类型：每类型告警一次，不重建不崩溃
        if (!this.warnedTypes.has(typeKey)) {
          this.warnedTypes.add(typeKey);
          console.warn(`[renderer] 未注册对象类型，跳过：${typeKey}`);
        }
        continue;
      }
      let view = this.views.get(id);
      if (view && view.typeKey !== typeKey) {
        // id↔typeKey 漂移防御（R3 §12 ⑪ 的 M3 防御点提前落位）：各夹具世界
        // 的 id 空间独立（都从 1 起编），回放切夹具时同 id 撞不同类型——
        // 旧视图必须拆树重建，否则部件跨类型叠加（实测 storage 部件叠 creep）
        this.destroyView(view);
        this.views.delete(id);
        view = undefined;
      }
      const x = (typeof state.x === 'number' ? state.x : 0) * this.cell;
      const y = (typeof state.y === 'number' ? state.y : 0) * this.cell;
      if (!view) {
        const root = new Container({ label: `obj-${id}` });
        root.position.set(x, y);
        this.slotFor(typeKey, desc.z).addChild(root);
        const overlays = new OverlayGroup(this.lighting);
        view = {
          id,
          typeKey,
          root,
          parts: new Map(),
          overlays,
          position: new PositionChannels(x, y, (px, py) => { root.position.set(px, py); overlays.moveTo(px, py); }),
          x,
          y,
          motion: MOTION_TYPE_KEYS.has(typeKey),
          lastRotation: 0,
        };
        this.views.set(id, view);
      } else if (kind === 'snapshot' || view.x !== x || view.y !== y) {
        const prevGameX = view.x / this.cell;
        const prevGameY = view.y / this.cell;
        view.x = x;
        view.y = y;
        if (view.motion && kind !== 'snapshot') {
          // 官方 actions.moveTo/rotateTo：位置变化 → MoveTo(easeInOutQuad,
          // tickDuration) + RotateTo(0.2 tick)；start=当前渲染位（中途重定位）
          this.beginMove(view, x, y, prevGameX, prevGameY);
        } else {
          view.position.setBase(x, y);
        }
      }
      this.reconcileParts(view, desc, state, env);
      const binding = view;
      this.ambient.sync(id, typeKey, state, time, view.parts, () => binding.root.position);
    }

    if (kind !== 'snapshot') {
      const resolution = resolveActions({ current: wanted, previous: this.previous, events, ...input });
      this.effects.recordSkipped(resolution.skipped);
      for (const hit of resolution.hits) {
        const view = this.views.get(hit.targetId);
        if (view && HIT_COVER_TYPES.has(view.typeKey)) this.triggerCover(view, hit.attacked && hit.healed
          ? CREEP_SPEC.cover.colors.both : hit.attacked ? CREEP_SPEC.cover.colors.attack : CREEP_SPEC.cover.colors.heal);
      }
      this.effects.play(resolution.commands.filter(c => {
        if (c.action !== 'say' || !c.private) return true;
        // No local identity: private speech is invisible (official isPublic||isOwner).
        if (this.player === null) return false;
        const state = wanted.get(c.sourceId) ?? this.previous.get(c.sourceId);
        return state !== undefined && ownerOf(state) === this.player;
      }));
      for (const previous of this.previous.values()) {
        if (previous.type !== 'nuke' || typeof previous.landTime !== 'number' || previous.landTime <= this.previousTime || previous.landTime > time) continue;
        const target = point(previous); if (target) this.effects.nukeBlast(target);
      }
    }
    // 官方 effects 层插入序：displayName（名字 overlay）先于一次性效果创建，
    // say 气泡/光束等盖在名字上。overlay 件创建时追加在 finite-effects 根之后，
    // 故每次对账末尾把根重挂到顶（对既有子节点 addChild=移到末尾，零排序）。
    this.effectsRoot.addChild(this.effects.root);
    this.previous = wanted;
    this.previousTime = time;
    return wallsChanged;
  }

  private reconcileParts(
    view: ObjectView,
    desc: RegisteredDescriptor,
    state: RoomObjectState,
    env: PartEnv,
  ): void {
    for (const decl of desc.parts) {
      const values = decl.values(state, env);
      let entry = view.parts.get(decl.id);
      if (values === null) {
        if (entry) {
          if (entry.tween) this.tweens.cancel(entry.tween);
          entry.tween = undefined;
          // lighting 成员被隐藏也是内容变化——RT 需重渲（C-B1）
          if (entry.node.parent === this.lighting.content && entry.node.visible) {
            this.lighting.markDirty();
          }
          entry.node.visible = false;
          // null 是部件停用，复活必须重应用（包含 values.visible 的自管件）。
          entry.prev = null;
        }
        continue;
      }
      if (!entry) {
        const renderer = desc.renderers.get(decl.id)!;
        const node = renderer.create(env);
        const layer = desc.layerOf(decl.id);
        if (layer === 'slot') {
          view.root.addChild(node);
        } else {
          // overlay 位置=当前渲染位（运动视图中途创建件不打断插值）
          node.position.copyFrom(view.root.position);
          if (layer === 'lighting') {
            this.lighting.addMember(node, false);
          } else {
            this.effectsRoot.addChild(node);
          }
          view.overlays.add(node, layer === 'lighting');
        }
        entry = { node, prev: null, sampled: null, channels: new PropertyChannels(node,
          () => { if (node.parent === this.lighting.content) this.lighting.markDirty(); }) };
        view.parts.set(decl.id, entry);
      }
      if (!valuesEqual(entry.prev, values)) {
        if (entry.tween) this.tweens.cancel(entry.tween);
        const part = entry;
        const write = (sample: PartValues) => {
          part.channels.restore();
          part.node.visible = true;
          desc.renderers.get(decl.id)!.apply(sample, part.node, env);
          part.channels.capture();
          part.sampled = sample;
          if (part.node.parent === this.lighting.content) this.lighting.markDirty();
        };
        const keys = continuousKeys(view.typeKey, decl.id);
        const from = entry.sampled;
        const interpolate = entry.prev !== null && from !== null && keys.some(k => typeof from[k] === 'number' && typeof values[k] === 'number' && from[k] !== values[k]);
        if (interpolate) {
          entry.tween = new ValueTween(from!, values, keys, this.tickDurationMS, write);
          this.tweens.run(entry.tween, () => { part.tween = undefined; });
          entry.tween.update(0);
        } else { entry.tween = undefined; write(values); }
        entry.prev = values;
        // lighting 成员在树内的可见性/几何变化不影响 membership——必须显式
        // 置脏离屏 RT（C-B1：官方每帧渲染层恒新鲜，我方 dirty 门控等价物）
        if (entry.node.parent === this.lighting.content) this.lighting.markDirty();
      }
      // 自管可见性（values.visible 键）的件不外层覆盖（glow 族）
      if (!('visible' in values)) entry.node.visible = true;
    }
  }

  private destroyView(view: ObjectView): void {
    this.effects.cancelOwner(view.id);
    this.ambient.remove(view.id);
    // 活动 tween 全部摘除（官方 cancelAction 语义——不 finish 不留悬空回调）
    this.cancelViewTweens(view);
    for (const entry of [...view.parts.values()]) {
      const node = entry.node;
      if (node.parent === this.lighting.content) this.lighting.removeMember(node);
      // Text 的自绘 canvas 纹理是私有源，可精确销毁（C-S2：v8 destroy 需显式
      // 标志，否则等 GCSystem 60s 兜底）；其余共享 atlas/standalone 纹理
      // 绝不带 texture 销毁（R2 评审⑤纪律）
      if (node instanceof Text) {
        node.destroy({ children: true, context: true, style: true, texture: true, textureSource: true });
      } else {
        node.destroy({ children: true, context: true });
      }
    }
    view.parts.clear();
    view.overlays.clear();
    if (view.cover) {
      view.cover.destroy({ children: true, context: true });
      view.cover = undefined;
    }
    view.root.destroy({ children: true, context: true });
  }

  // ---- 运动/受击罩/死亡淡出（R4；官方 actions.moveTo 族 + creepActions
  //      cover + disappearProcessor 的编排层） ----

  /** ticker 驱动（真实帧时间——数据断流动画照跑，〔官拆〕§7）。 */
  update(deltaMS: number, viewport?: Viewport): void {
    if (deltaMS <= 0) return;
    this.tweens.update(deltaMS);
    this.ambient.update(deltaMS, viewport);
    this.effects.update(deltaMS);
  }

  private actorBinding(id: string): ActorBinding | undefined {
    const view = this.views.get(id); if (!view) return undefined;
    return {
      get position() { return view.root.position; },
      get moving() { return view.moveTween !== undefined; },
      offset: (x, y) => view.position.setOffset(x, y),
      turn: (angle, duration) => {
        const main = view.parts.get('main'); if (!main) return;
        if (view.rotateTween) this.tweens.cancel(view.rotateTween);
        view.lastRotation = angle;
        view.rotateTween = new RotateTween(r => main.channels.setRotation(r), main.node.rotation, angle, duration);
        this.tweens.run(view.rotateTween, () => { view.rotateTween = undefined; });
      },
      towerAim: angle => {
        const gun = view.parts.get('rotatable')?.node.getChildByLabel('gun');
        if (gun) this.ambient.aim(id, angle, gun);
      },
      towerGlow: factor => view.parts.get('glowBig')?.channels.sample(factor),
    };
  }

  /** 位置变化 → 移动插值 + 朝向（官方 Ease(MoveTo, EASE_IN_OUT_QUAD) +
   * RotateTo 0.2 tick；新动作取消旧动作，start=当前渲染位）。 */
  private beginMove(
    view: ObjectView,
    toX: number,
    toY: number,
    prevGameX: number,
    prevGameY: number,
  ): void {
    if (view.moveTween) this.tweens.cancel(view.moveTween);
    this.effects.cancelOwner(view.id, 'bite');
    const base = view.position.base;
    view.moveTween = new MoveTween(
      (px, py) => {
        view.position.setBase(px, py);
      },
      base.x,
      base.y,
      toX,
      toY,
      this.tickDurationMS,
    );
    this.tweens.run(view.moveTween, () => { view.moveTween = undefined; });

    const angle = calculateAngle(prevGameX, prevGameY, toX / this.cell, toY / this.cell);
    if (angle !== view.lastRotation) {
      view.lastRotation = angle;
      const main = view.parts.get('main')?.node;
      if (main) {
        if (view.rotateTween) this.tweens.cancel(view.rotateTween);
        view.rotateTween = new RotateTween(
          (r) => {
            view.parts.get('main')!.channels.setRotation(r);
          },
          main.rotation,
          angle,
          this.tickDurationMS * 0.2,
        );
        this.tweens.run(view.rotateTween, () => { view.rotateTween = undefined; });
      }
    }
  }

  private triggerCover(view: ObjectView, tint: number): void {
    if (!view.cover) {
      const coverTexture = this.assets.frames.get('cover');
      if (!coverTexture) return; // 缺帧降级：无罩
      const node = new Sprite(coverTexture);
      node.label = 'hit-cover';
      node.anchor.set(0.5, 0.5);
      node.blendMode = 'add';
      node.alpha = 0;
      node.position.copyFrom(view.root.position);
      this.effectsRoot.addChild(node);
      view.cover = node;
      view.overlays.add(node);
    }
    const cover = view.cover;
    cover.tint = tint;
    // 官方 childCover 是 mainContainer 子件（creepActions.js:74）——随其
    // safemode α 暗化；我方罩是 effects 层独立节点（cover.svg 纯圆、不随
    // main 旋转零观感差），以 main 当前 α 作乘数传导（R4 评审 A-B3/S3）
    if (view.coverTween) this.tweens.cancel(view.coverTween);
    view.coverTween = new AlphaKeyframesTween(
      (a) => {
        // main 的 safemode 状态可能在罩播放期间改变，按当前值传导。
        cover.alpha = a * (view.parts.get('main')?.node.alpha ?? 1);
      },
      [
        [CREEP_SPEC.cover.fadeIn, CREEP_SPEC.cover.alpha],
        [1, 0],
      ],
      // 官方三处 call-site 传 tickDuration*0.9（creepActions.js:299-306，
      // 评审 A-B2）——峰值 0.225td、总长 0.9td
      this.tickDurationMS * CREEP_SPEC.cover.durationScale,
    );
    this.tweens.run(view.coverTween);
  }

  /** 死亡淡出（官方 disappear：FadeOut(tickDuration/2) 后销毁；root 与
   * overlay 件同步淡出，lighting 逐帧 markDirty）。overlay 以**捕捉底 α 作
   * 乘法**——safemode 暗化（α0.5）的视图死亡时不得跳亮回 1（官方=层级 α
   * 连乘，评审 A 二轮）；罩 tween 一并取消（避免与 fade 的末写者冲突），
   * 罩冻结于当前 α 随淡出衰减。 */
  private beginFade(id: string, view: ObjectView): void {
    this.dying.set(id, view);
    if (view.fadeTween) this.tweens.cancel(view.fadeTween);
    if (view.coverTween) this.tweens.cancel(view.coverTween);
    for (const entry of view.parts.values()) {
      if (entry.tween) this.tweens.cancel(entry.tween);
      entry.tween = undefined;
    }
    const fadeOverlays = view.overlays.captureFade();
    view.fadeTween = new AlphaTween(
      (a) => {
        view.root.alpha = a;
        fadeOverlays(a);
      },
      view.root.alpha,
      0,
      this.tickDurationMS / 2,
    );
    this.tweens.run(view.fadeTween, () => {
      // 自然结束：销毁（onEnd 在 tweenManager 迭代内——destroyView 的 cancel
      // 对已摘除的自身 tween 是 no-op）
      this.dying.delete(id);
      this.destroyView(view);
    });
  }

  /** id 复用防御的立即收尾（cancel 淡出 → 即刻销毁）。 */
  private finishFade(view: ObjectView): void {
    for (const [id, dying] of [...this.dying]) {
      if (dying === view) {
        this.dying.delete(id);
        this.destroyView(view);
      }
    }
  }

  /** 快照清瞬态、保留同 id/type 的节点；坐标跳转在对账阶段即时应用。 */
  private resetTransientState(view: ObjectView): void {
    this.cancelViewTweens(view);
    view.moveTween = view.rotateTween = view.fadeTween = view.coverTween = undefined;
    if (view.cover) {
      view.overlays.remove(view.cover);
      view.cover.destroy({ children: true, context: true });
      view.cover = undefined;
    }
    view.root.alpha = 1;
    view.position.setOffset(0, 0);
    view.lastRotation = 0;
    const main = view.parts.get('main')?.node;
    if (main) view.parts.get('main')!.channels.setRotation(0);
    for (const entry of view.parts.values()) { entry.channels.reset(); entry.prev = null; entry.tween = undefined; }
  }

  private cancelViewTweens(view: ObjectView): void {
    for (const entry of view.parts.values()) if (entry.tween) this.tweens.cancel(entry.tween);
    if (view.moveTween) this.tweens.cancel(view.moveTween);
    if (view.rotateTween) this.tweens.cancel(view.rotateTween);
    if (view.fadeTween) this.tweens.cancel(view.fadeTween);
    if (view.coverTween) this.tweens.cancel(view.coverTween);
  }

  // ---- road 连通（road.js 几何；集合变化即全量重画——格数小，官方亦按集重建） ----

  private syncRoad(objects: readonly RoomObjectState[]): void {
    const nodes = roadNodes(objects);
    const key = nodes.map((n) => `${n.x},${n.y}`).join(';');
    if (key === this.roadKey) return;
    this.roadKey = key;
    for (const child of [...this.roadRoot.children]) child.destroy({ children: true, context: true });

    const r = ROAD_SPEC.radiusFactor * this.cell;
    const rb = r * SIN_45;
    const l = this.cell;
    for (const node of nodes) {
      const g = new Graphics();
      g.position.set(node.x * this.cell, node.y * this.cell);
      g.circle(0, 0, r).fill({ color: ROAD_SPEC.color });
      if (node.mm) {
        g.poly([rb, -rb, -rb, rb, -rb - l, rb - l, rb - l, -rb - l]).fill({ color: ROAD_SPEC.color });
      }
      if (node.zm) {
        g.poly([r, 0, -r, 0, -r, -l, r, -l]).fill({ color: ROAD_SPEC.color });
      }
      if (node.pm) {
        g.poly([-rb, -rb, rb, rb, rb + l, rb - l, -rb + l, -rb - l]).fill({ color: ROAD_SPEC.color });
      }
      if (node.mz) {
        g.poly([0, r, 0, -r, -l, -r, -l, r]).fill({ color: ROAD_SPEC.color });
      }
      this.roadRoot.addChild(g);
    }
  }

  // ---- rampart 私有顶罩（terrain.js:183-247；按 user 分组、hash 门控烘焙） ----

  private syncRampartCover(objects: readonly RoomObjectState[]): void {
    const groups = rampartGroups(objects, {
      isOwn: (user) => user !== null && user !== undefined && user === this.player,
    });
    const seen = new Set<string>();
    for (const group of groups) {
      const key = group.user ?? '';
      seen.add(key);
      const hash = rampartGroupHash(group);
      if (this.coverHashes.get(key) === hash) continue;
      const child = this.coverRoot.getChildByLabel(`cover-${key}`);
      const old = child instanceof Sprite ? child : undefined;
      const baked = this.bakeCover(group, old);
      this.coverHashes.set(key, hash);
      // Preserve the prior changed-group append order while retaining private source UID.
      old?.removeFromParent();
      this.coverRoot.addChild(baked);
    }
    for (const key of [...this.coverHashes.keys()]) {
      if (!seen.has(key)) {
        this.coverHashes.delete(key);
        const old = this.coverRoot.getChildByLabel(`cover-${key}`);
        if (old) old.destroy({ children: true, context: true, texture: true, textureSource: true });
      }
    }
  }

  private bakeCover(group: RampartGroup, previous?: Sprite): Sprite {
    const codes = new Uint8Array(ROOM_GRID * ROOM_GRID);
    for (const idx of group.cells) codes[idx] = 1;
    // 顶罩组变化时重画已有私有 canvas/source；hash 门控防重复。
    const merged = mergedOutlinePath(codes, (c) => c !== 0, { cell: this.cell });
    const half = this.cell / 2;
    // paint-order:stroke——描边在下（外半环可见），官方 stroke-width 25
    const region = rampartRasterBounds(group.cells, this.cell, this.view, this.renderSize, RAMPART_COVER_SPEC.strokeWidth);
    const canvas = bakeCanvas(merged.path ?? '', this.view, this.renderSize, (ctx, p) => {
      ctx.lineWidth = RAMPART_COVER_SPEC.strokeWidth;
      ctx.strokeStyle = cssColor(group.stroke);
      ctx.stroke(p);
      ctx.fillStyle = cssColor(group.fill);
      ctx.fill(p);
    }, region, previous?.texture.source.resource as HTMLCanvasElement | undefined);
    let sprite = previous;
    if (sprite) {
      sprite.texture.source.update();
      sprite.texture.update();
    } else {
      const texture = textureFromBaked(canvas);
      texture.dynamic = true; // Sprite observes frame-size updates when crop bounds change.
      sprite = new Sprite(texture);
    }
    sprite.label = `cover-${group.user ?? ''}`;
    const scale = this.view / this.renderSize;
    sprite.position.set(region.x * scale - half, region.y * scale - half);
    sprite.setSize(region.width * scale, region.height * scale);
    sprite.alpha = RAMPART_COVER_SPEC.alpha;
    sprite.blendMode = RAMPART_COVER_SPEC.blend;
    return sprite;
  }

  destroy(): void {
    this.effects.destroy();
    this.ambient.reset();
    this.previous.clear();
    for (const view of [...this.dying.values()]) this.destroyView(view);
    this.dying.clear();
    for (const view of [...this.views.values()]) this.destroyView(view);
    this.views.clear();
    for (const slot of [...this.slots.values()]) slot.destroy({ children: true, context: true });
    this.slots.clear();
    for (const child of [...this.roadRoot.children]) child.destroy({ children: true, context: true });
    // 顶罩为烘焙私有 canvas 纹理——可带纹理销毁
    for (const child of [...this.coverRoot.children]) {
      child.destroy({ children: true, context: true, texture: true, textureSource: true });
    }
    this.coverHashes.clear();
  }
}

/** 0xRRGGBB → css 颜色串（canvas 烘焙侧）。 */
function cssColor(rgb: number): string {
  return `#${rgb.toString(16).padStart(6, '0')}`;
}

function integerSetEquals(a: ReadonlySet<number>, b: ReadonlySet<number>): boolean {
  if (a.size !== b.size) return false;
  for (const v of a) {
    if (!b.has(v)) return false;
  }
  return true;
}
