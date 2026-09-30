/**
 * 对象层舞台：feed 对象列表 → 场景图增量（方案 §2.3/§2.4）。
 *
 * - id 对账：视图按 id 池化复用，移除即拆树销毁（官方 destroy/rebuild 风暴的
 *   反面——部件节点只在首次激活时创建，值变化仅重应用；消失动效 R5）。
 * - 差量：值相等门控（valuesEqual），只有值变化的部件触达 pixi。
 * - 层路由：slot（对象根局部坐标）/ lighting（世界坐标，共享纹理成员只
 *   detach）/ effects（世界坐标）。
 * - 附属绘制：road 连通（ground 层，road.js 几何）、rampart 私有顶罩
 *   （effects 层，terrain.js:183-247 按 user 分组合并 path 烘焙）、
 *   constructedWall 格并入墙烘焙（terrain.js:264-266 的 wall 过滤含
 *   constructedWall）。
 */
import { Container, Graphics, Sprite, Text } from 'pixi.js';
import type { RoomObjectState } from '@kkscreeps/shared';
import { bakeCanvas, textureFromBaked } from '../core/bake.js';
import type { LightingPass } from '../core/lighting.js';
import { mergedOutlinePath } from '../core/path-merge.js';
import type { PartEnv, PartValues } from './part-decl.js';
import { valuesEqual } from './part-decl.js';
import { buildRegistry, type RegisteredDescriptor } from './registry.js';
import type { JoinAssets } from './joins-naturals.js';
import { ROAD_SPEC } from './specs.js';
import { TERRAIN_SPEC } from '../core/terrain-spec.js';
import { roadNodes } from './road-geometry.js';
import { rampartGroupHash, rampartGroups, ROOM_GRID, type RampartGroup } from './rampart-groups.js';
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
}

interface PartEntry {
  node: Container;
  prev: PartValues | null;
}

interface ObjectView {
  readonly typeKey: string;
  readonly root: Container;
  readonly parts: Map<string, PartEntry>;
  x: number;
  y: number;
}

const SIN_45 = Math.sin(Math.PI / 4);

export class ObjectsStage {
  /** ground 层子容器：road 连通。 */
  readonly roadRoot = new Container({ label: 'objects-road' });
  /** effects 层子容器：rampart 顶罩（先）+ 对象 effects 部件（后）。 */
  readonly coverRoot = new Container({ label: 'objects-cover' });
  readonly effectsRoot = new Container({ label: 'objects-effects' });

  private readonly cell: number;
  private readonly view: number;
  private readonly lighting: LightingPass;
  private readonly player: string | null;
  private readonly objectsLayer: Container;
  private readonly registry: Map<string, RegisteredDescriptor>;
  private readonly slots = new Map<string, Container>();
  private readonly views = new Map<string, ObjectView>();
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
    this.objectsLayer = options.objectsLayer;
    this.registry = buildRegistry(options.assets);
  }

  private env(time: number): PartEnv {
    return {
      cell: this.cell,
      time,
      player: this.player,
      isOwn: (user) => user !== null && user !== undefined && user === this.player,
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
   * （变化时 RoomScene 需把 wallExtras 并入墙烘焙重烧）。
   */
  apply(objects: readonly RoomObjectState[], time: number): boolean {
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

    const env = this.env(time);
    const wanted = new Map<string, RoomObjectState>();
    for (const o of objects) {
      if (typeof o.id === 'string') wanted.set(o.id, o);
    }

    // 移除（快照迭代——destroy 同步 splice 活数组会跳格，R2 复审 N1 纪律）
    for (const [id, view] of [...this.views]) {
      if (!wanted.has(id)) {
        this.destroyView(view);
        this.views.delete(id);
      }
    }

    for (const [id, state] of wanted) {
      if (state.type === 'structure' && state.structureType === 'road') continue; // road 走连通绘制
      let view = this.views.get(id);
      const typeKey = resolveTypeKey(state);
      const desc = this.registry.get(typeKey);
      if (!desc) {
        // 未落位类型（creep/powerCreep R4）：每类型告警一次，不重建不崩溃
        if (!this.warnedTypes.has(typeKey)) {
          this.warnedTypes.add(typeKey);
          console.warn(`[renderer] 未注册对象类型，跳过：${typeKey}`);
        }
        continue;
      }
      const x = (typeof state.x === 'number' ? state.x : 0) * this.cell;
      const y = (typeof state.y === 'number' ? state.y : 0) * this.cell;
      if (!view) {
        const root = new Container({ label: `obj-${id}` });
        root.position.set(x, y);
        this.slotFor(typeKey, desc.z).addChild(root);
        view = { typeKey, root, parts: new Map(), x, y };
        this.views.set(id, view);
      } else if (view.x !== x || view.y !== y) {
        view.x = x;
        view.y = y;
        view.root.position.set(x, y);
        let movedLighting = false;
        for (const entry of view.parts.values()) {
          if (entry.node.parent !== view.root) {
            entry.node.position.set(x, y);
            if (entry.node.parent === this.lighting.content) movedLighting = true;
          }
        }
        // lighting 成员位移不换 membership——RT 需重渲（清偿复审 R2；R3 静态
        // 对象无位移，R4 creep 运动起此路径生效）
        if (movedLighting) this.lighting.markDirty();
      }
      this.reconcileParts(view, desc, state, env);
    }
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
          // lighting 成员被隐藏也是内容变化——RT 需重渲（C-B1）
          if (entry.node.parent === this.lighting.content && entry.node.visible) {
            this.lighting.markDirty();
          }
          entry.node.visible = false;
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
          node.position.set(view.x, view.y);
          if (layer === 'lighting') this.lighting.addMember(node, false);
          else this.effectsRoot.addChild(node);
        }
        entry = { node, prev: null };
        view.parts.set(decl.id, entry);
      }
      if (!valuesEqual(entry.prev, values)) {
        desc.renderers.get(decl.id)!.apply(values, entry.node, env);
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
    for (const entry of [...view.parts.values()]) {
      const node = entry.node;
      if (node.parent === this.lighting.content) this.lighting.removeMember(node);
      // Text 的自绘 canvas 纹理是私有源，可精确销毁（C-S2：v8 destroy 需显式
      // 标志，否则等 GCSystem 60s 兜底）；其余共享 atlas/standalone 纹理
      // 绝不带 texture 销毁（R2 评审⑤纪律）
      if (node instanceof Text) {
        node.destroy({ children: true, style: true, texture: true, textureSource: true });
      } else {
        node.destroy({ children: true });
      }
    }
    view.parts.clear();
    view.root.destroy({ children: true });
  }

  // ---- road 连通（road.js 几何；集合变化即全量重画——格数小，官方亦按集重建） ----

  private syncRoad(objects: readonly RoomObjectState[]): void {
    const nodes = roadNodes(objects);
    const key = nodes.map((n) => `${n.x},${n.y}`).join(';');
    if (key === this.roadKey) return;
    this.roadKey = key;
    for (const child of [...this.roadRoot.children]) child.destroy({ children: true });

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
      this.coverHashes.set(key, hash);
      const old = this.coverRoot.getChildByLabel(`cover-${key}`);
      const baked = this.bakeCover(group);
      if (old) old.destroy({ children: true, texture: true, textureSource: true });
      this.coverRoot.addChild(baked);
    }
    for (const key of [...this.coverHashes.keys()]) {
      if (!seen.has(key)) {
        this.coverHashes.delete(key);
        const old = this.coverRoot.getChildByLabel(`cover-${key}`);
        if (old) old.destroy({ children: true, texture: true, textureSource: true });
      }
    }
  }

  private bakeCover(group: RampartGroup): Sprite {
    const codes = new Uint8Array(ROOM_GRID * ROOM_GRID);
    for (const idx of group.cells) codes[idx] = 1;
    // 顶罩每次组变化即重建（hash 门控防重复）；首参 hash 不复用（重建走 compare 外层）
    const merged = mergedOutlinePath(codes, (c) => c !== 0, { cell: this.cell });
    const half = this.cell / 2;
    // paint-order:stroke——描边在下（外半环可见），官方 stroke-width 25
    const canvas = bakeCanvas(merged.path ?? '', this.view, this.renderSize, (ctx, p) => {
      ctx.lineWidth = RAMPART_COVER_SPEC.strokeWidth;
      ctx.strokeStyle = cssColor(group.stroke);
      ctx.stroke(p);
      ctx.fillStyle = cssColor(group.fill);
      ctx.fill(p);
    });
    const sprite = new Sprite(textureFromBaked(canvas));
    sprite.label = `cover-${group.user ?? ''}`;
    sprite.position.set(-half, -half);
    sprite.setSize(this.view, this.view);
    sprite.alpha = RAMPART_COVER_SPEC.alpha;
    sprite.blendMode = RAMPART_COVER_SPEC.blend;
    return sprite;
  }

  destroy(): void {
    for (const view of [...this.views.values()]) this.destroyView(view);
    this.views.clear();
    for (const slot of [...this.slots.values()]) slot.destroy({ children: true });
    this.slots.clear();
    for (const child of [...this.roadRoot.children]) child.destroy({ children: true });
    // 顶罩为烘焙私有 canvas 纹理——可带纹理销毁
    for (const child of [...this.coverRoot.children]) {
      child.destroy({ children: true, texture: true, textureSource: true });
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
