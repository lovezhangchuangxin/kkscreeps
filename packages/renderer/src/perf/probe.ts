/** Internal perf-build bridge. This module is eliminated from the normal library. */
import { BigPool, BindGroup, Container, Texture, TextureSource } from 'pixi.js';
import type { RendererHandle } from '../index.js';
import type { Kernel } from '../core/renderer.js';
import type { LoadedAtlas } from '../assets/atlas.js';
import type { FeedStore } from '../feed/room-feed.js';
import type { RoomScene } from '../scenes/room.js';
import { valuesEqual } from '../objects/part-decl.js';

type Runtime = Record<string, any>;
const bridges = new WeakMap<RendererHandle, PerfProbe>();
const groups = new Map<number, WeakRef<BindGroup>>();
const sourceRefs = new Map<number, WeakRef<TextureSource>>();
const groupIDs = new WeakMap<BindGroup, number>();
let nextGroup = 0,
  sourceChanges = 0,
  destroyedResourceChanges = 0;
// Record weak references only: sampling must never prolong a resource's lifetime.
const setResource = BindGroup.prototype.setResource;
BindGroup.prototype.setResource = function (...args: Parameters<typeof setResource>) {
  if (!groupIDs.has(this)) {
    groupIDs.set(this, ++nextGroup);
    groups.set(nextGroup, new WeakRef(this));
  }
  return setResource.apply(this, args);
};
const groupPrototype = BindGroup.prototype as unknown as Runtime;
const resourceChange = groupPrototype.onResourceChange;
groupPrototype.onResourceChange = function (this: BindGroup, ...args: any[]) {
  sourceChanges++;
  if (args[0].destroyed) destroyedResourceChanges++;
  return resourceChange.apply(this, args); // keep Pixi's warning and original cleanup.
};

export interface PerfProbe {
  kernel: Kernel;
  frame(): Record<string, number>;
  takeApply(): Record<string, number>[];
  takeFeed(): Runtime[];
  counts(): Runtime;
  motion(): Runtime[];
  activity(): Record<string, number>;
  gpuDone(): Promise<void>;
  stop(): void;
  render(): void;
}

export function attachPerf(
  handle: RendererHandle,
  kernel: Kernel,
  room: RoomScene,
  feed: FeedStore,
  atlas: LoadedAtlas | null,
): void {
  const undo: Array<() => void> = [];
  const wrap = (
    target: Runtime,
    key: string,
    fn: (old: (...args: any[]) => any, args: any[]) => any,
  ) => {
    const original = target[key];
    target[key] = function (this: Runtime, ...args: any[]) {
      return fn(original.bind(this), args);
    };
    undo.push(() => {
      target[key] = original;
    });
  };
  let sceneMs = 0,
    mainMs = 0,
    tweenMs = 0,
    tweenSize = 0;
  let applyParts: Record<string, number> = {},
    applies: Record<string, number>[] = [];
  let gateEvaluated = 0,
    gateNull = 0,
    gateWrites = 0,
    gateEqual = 0,
    gateChanged = 0,
    animatedWrites = 0;
  let inApply = false,
    tweenCreated = 0,
    tweenEnded = 0,
    tweenCancelled = 0;
  let feedRows: Runtime[] = [],
    feedParts: Record<string, number> | null = null;
  let frames = 0,
    viewsCreated = 0,
    viewsDestroyed = 0;
  const nodeIDs = new WeakMap<object, number>();
  let nextNode = 0;
  const partCounts: Record<
    string,
    { evaluations: number; equal: number; changed: number; null: number; writes: number }
  > = {};
  const stage = room.objects as unknown as Runtime | null;
  const profiling = new URLSearchParams(location.search).get('profile') === '1';
  if (stage && profiling) {
    const instrumented = new WeakSet<object>();
    for (const descriptor of stage.registry.values()) {
      for (const declaration of descriptor.parts)
        if (!instrumented.has(declaration)) {
          instrumented.add(declaration);
          wrap(declaration, 'values', (old, args) => {
            const state = args[0],
              key = `${state.type === 'structure' ? state.structureType : state.type}/${declaration.id}`;
            const count = (partCounts[key] ??= {
              evaluations: 0,
              equal: 0,
              changed: 0,
              null: 0,
              writes: 0,
            });
            gateEvaluated++;
            count.evaluations++;
            const value = old(...args);
            if (value === null) {
              gateNull++;
              count.null++;
            } else if (
              valuesEqual(stage.views.get(state.id)?.parts.get(declaration.id)?.prev ?? null, value)
            ) {
              gateEqual++;
              count.equal++;
            } else {
              gateChanged++;
              count.changed++;
            }
            return value;
          });
        }
      for (const [partID, renderer] of descriptor.renderers)
        if (!instrumented.has(renderer)) {
          instrumented.add(renderer);
          wrap(renderer, 'apply', (old, args) => {
            gateWrites++;
            if (!inApply) animatedWrites++;
            const key = `${descriptor.typeKey.startsWith('s:') ? descriptor.typeKey.slice(2) : descriptor.typeKey}/${partID}`;
            (partCounts[key] ??= { evaluations: 0, equal: 0, changed: 0, null: 0, writes: 0 })
              .writes++;
            return old(...args);
          });
        }
    }
    for (const [target, key] of [
      [stage.ambient, 'sync'],
      [stage.effects, 'play'],
    ] as const)
      wrap(target, key, (old, args) => {
        const t = performance.now();
        try {
          return old(...args);
        } finally {
          applyParts[`$${key}`] = (applyParts[`$${key}`] ?? 0) + performance.now() - t;
        }
      });
    for (const key of [
      'syncRoad',
      'syncRampartCover',
      'reconcileParts',
      'resetTransientState',
      'destroyView',
      'beginMove',
    ])
      wrap(stage, key, (old, args) => {
        const t = performance.now();
        try {
          return old(...args);
        } finally {
          applyParts[key] = (applyParts[key] ?? 0) + performance.now() - t;
        }
      });
    wrap(stage, 'apply', (old, args) => {
      applyParts = {};
      const t = performance.now();
      const evaluated = gateEvaluated,
        writes = gateWrites,
        nulls = gateNull;
      const previousRoots = new Set(
        [...stage.views.values(), ...stage.dying.values()].map((view: Runtime) => view.root),
      );
      inApply = true;
      try {
        return old(...args);
      } finally {
        inApply = false;
        const created = [...stage.views.values(), ...stage.dying.values()].filter(
          (view: Runtime) => !previousRoots.has(view.root),
        ).length;
        const destroyed = [...previousRoots].filter((root: Runtime) => root.destroyed).length;
        viewsCreated += created;
        viewsDestroyed += destroyed;
        applies.push({
          ...applyParts,
          totalMs: performance.now() - t,
          viewsCreated: created,
          viewsDestroyed: destroyed,
          gateEvaluated: gateEvaluated - evaluated,
          gateWrites: gateWrites - writes,
          gateNull: gateNull - nulls,
          snapshot: Number(args[3] === 'snapshot'),
        });
      }
    });
    wrap(stage.tweens, 'run', (old, args) => {
      const size = stage.tweens.size;
      const done = args[1];
      const result = old(args[0], () => {
        tweenEnded++;
        done?.();
      });
      tweenCreated += Math.max(0, stage.tweens.size - size);
      return result;
    });
    wrap(stage.tweens, 'cancel', (old, args) => {
      const size = stage.tweens.size;
      const result = old(...args);
      tweenCancelled += Math.max(0, size - stage.tweens.size);
      return result;
    });
    wrap(stage.tweens, 'update', (old, args) => {
      tweenSize = stage.tweens.size;
      const t = performance.now();
      try {
        return old(...args);
      } finally {
        tweenMs = performance.now() - t;
      }
    });
  }
  if (profiling) {
    for (const key of ['setTerrain', 'applyObjects', 'setVisuals', 'setHover'])
      wrap(room as unknown as Runtime, key, (old, args) => {
        const t = performance.now();
        try {
          return old(...args);
        } finally {
          if (feedParts) feedParts[key] = (feedParts[key] ?? 0) + performance.now() - t;
        }
      });
    wrap(feed as unknown as Runtime, 'notify', (old, args) => {
      const t = performance.now();
      try {
        return old(...args);
      } finally {
        if (feedParts) feedParts.notifyMs = (feedParts.notifyMs ?? 0) + performance.now() - t;
      }
    });
    for (const key of ['applySnapshot', 'applyDelta', 'setVisuals', 'setTerrain'])
      wrap(feed as unknown as Runtime, key, (old, args) => {
        feedParts = {};
        const parts = feedParts,
          t = performance.now();
        try {
          return old(...args);
        } finally {
          const totalMs = performance.now() - t;
          feedRows.push({
            operation: key,
            totalMs,
            mapOrCopyMs: totalMs - (parts.notifyMs ?? 0),
            ...parts,
          });
          feedParts = null;
        }
      });
  }
  wrap(room as unknown as Runtime, 'update', (old, args) => {
    frames++;
    tweenSize = stage?.tweens.size ?? 0;
    const t = performance.now();
    try {
      return old(...args);
    } finally {
      sceneMs = performance.now() - t;
      mainMs = 0;
    }
  });
  wrap(kernel.app.renderer as unknown as Runtime, 'render', (old, args) => {
    // Pixi fills options.target in-place: classify before dispatch, not after it.
    const isMain = !args[0]?.target;
    const t = performance.now();
    try {
      return old(...args);
    } finally {
      if (isMain) mainMs += performance.now() - t;
    }
  });
  const renderer = kernel.app.renderer as unknown as Runtime;
  const gpuDone = async () => {
    if (kernel.backend === 'webgpu') {
      await renderer.gpu.device.queue.onSubmittedWorkDone();
    } else {
      const gl = renderer.gl as WebGL2RenderingContext;
      if (!gl.fenceSync) throw new Error('GPU completion requires WebGL2 fenceSync');
      const fence = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
      if (!fence) throw new Error('Cannot create WebGL fence');
      gl.flush();
      try {
        for (;;) {
          const status = gl.clientWaitSync(fence, 0, 0);
          if (status === gl.WAIT_FAILED) throw new Error('WebGL fence failed');
          if (status !== gl.TIMEOUT_EXPIRED) break;
          await new Promise((resolve) => setTimeout(resolve, 0));
        }
      } finally {
        gl.deleteSync(fence);
      }
    }
  };
  const probe: PerfProbe = {
    kernel,
    frame: () => ({
      sceneMs,
      mainMs,
      cpuRenderMs: sceneMs + mainMs,
      objectsMs: room.lastStageMs.objectsMs,
      lightingMs: room.lastStageMs.lightingMs,
      tweenMs,
      tweenSize,
    }),
    takeFeed: () => {
      const records = feedRows;
      feedRows = [];
      return records;
    },
    takeApply: () => {
      const records = applies;
      applies = [];
      return records;
    },
    counts: () => {
      const textures = new Set<Texture>(),
        sources = new Set<TextureSource>();
      const scan = (node: Container) => {
        const texture = (node as unknown as { texture?: Texture }).texture;
        if (texture instanceof Texture && !texture.destroyed) {
          textures.add(texture);
          sources.add(texture.source);
        }
        for (const child of node.children ?? []) scan(child);
      };
      scan(room.root);
      scan(room.lighting.content);
      const managedSources = Object.values(
        renderer.texture?._managedTextures?.items ?? {},
      ) as Array<TextureSource | null>;
      for (const source of managedSources) if (source && !source.destroyed) sources.add(source);
      for (const texture of atlas?.frames.values() ?? []) {
        textures.add(texture);
        sources.add(texture.source);
      }
      for (const { texture } of atlas?.standalone.values() ?? []) {
        textures.add(texture);
        sources.add(texture.source);
      }
      for (const source of sources) sourceRefs.set(source.uid, new WeakRef(source));
      let liveSources = 0,
        sourceBytes = 0;
      for (const [id, ref] of sourceRefs) {
        const source = ref.deref();
        if (!source) {
          sourceRefs.delete(id);
          continue;
        }
        if (!source.destroyed) {
          liveSources++;
          sourceBytes += source.pixelWidth * source.pixelHeight * 4;
        }
      }
      let liveGroups = 0,
        nullSlots = 0,
        destroyedSlots = 0;
      for (const [id, ref] of groups) {
        const group = ref.deref();
        if (!group) {
          groups.delete(id);
          continue;
        }
        if (!group.resources) continue;
        liveGroups++;
        for (const resource of Object.values(group.resources)) {
          if (!resource) nullSlots++;
          else if (resource.destroyed) destroyedSlots++;
        }
      }
      let overlays = 0;
      if (stage)
        for (const view of [...stage.views.values(), ...stage.dying.values()])
          overlays += view.overlays.members.size;
      const lighting = room.lighting as unknown as Runtime;
      return {
        views: stage?.views.size ?? 0,
        dying: stage?.dying.size ?? 0,
        tweens: stage?.tweens.size ?? 0,
        overlays,
        lighting: {
          invalidations: lighting.invalidations,
          renders: lighting.renders,
          members: lighting.content.children?.length ?? 0,
          privateSources:
            lighting.ownedMembers.size + Number(!!lighting.rt && !lighting.rt.destroyed),
        },
        effects: room.objects?.effects.stats,
        ambient: room.objects?.ambient.stats,
        textures: textures.size,
        sources: liveSources,
        estimatedSourceBytes: sourceBytes,
        coverSources: stage?.coverRoot.children.length ?? 0,
        estimatedCoverBytes: (stage?.coverRoot.children ?? []).reduce(
          (n: number, sprite: Runtime) =>
            n + sprite.texture.source.pixelWidth * sprite.texture.source.pixelHeight * 4,
          0,
        ),
        gpuManagedTextures: managedSources.filter(Boolean).length,
        gpuManagedTextureSlots: managedSources.length,
        gpuManagedTextureNullSlots: managedSources.filter((source) => source === null).length,
        pools: BigPool.stats(),
        graphicsContexts: Object.values(
          renderer.graphicsContext?._managedContexts?.items ?? {},
        ).filter(Boolean).length,
        graphicsContextSlots: Object.keys(renderer.graphicsContext?._managedContexts?.items ?? {})
          .length,
        canvasTextActive: Object.values(renderer.canvasText?._activeTextures ?? {}).filter(Boolean)
          .length,
        bindGroups: liveGroups,
        nullSlots,
        destroyedSlots,
        sourceChanges,
        destroyedResourceChanges,
        profiling,
        frames,
        viewsCreated,
        viewsDestroyed,
        gateEvaluated,
        gateNull,
        gateEqual,
        gateChanged,
        gateWrites,
        animatedWrites,
        // Checkpoints must retain their sampled values, not the mutable counters.
        partCounts: Object.fromEntries(
          Object.entries(partCounts).map(([key, value]) => [key, { ...value }]),
        ),
        tweenCreated,
        tweenEnded,
        tweenCancelled,
        tweenActive: stage?.tweens.active.size ?? 0,
        tweenPending: stage?.tweens.pending.length ?? 0,
        feedRooms: feed.roomNames().length,
        feedObjects: feed
          .roomNames()
          .reduce((n, name) => n + (feed.summary(name)?.objects ?? 0), 0),
        feedPoolLeases: null,
        atlasFrameRefs: atlas?.frames.size ?? 0,
        standaloneLeases: atlas?.standalone.size ?? 0,
      };
    },
    activity: () => {
      let overlays = 0;
      if (stage)
        for (const view of [...stage.views.values(), ...stage.dying.values()])
          overlays += view.overlays.members.size;
      const effects = room.objects?.effects.stats;
      return {
        dying: stage?.dying.size ?? 0,
        overlays,
        tweens: stage?.tweens.size ?? 0,
        finiteActive: effects?.active ?? 0,
        finiteNodes: effects?.nodes ?? 0,
        finiteTexts: effects?.texts ?? 0,
      };
    },
    motion: () =>
      stage
        ? [...stage.views.values()]
            .filter((v: Runtime) => v.motion)
            .map((view: Runtime) => {
              if (!nodeIDs.has(view.root)) nodeIDs.set(view.root, ++nextNode);
              return {
                id: view.id,
                node: nodeIDs.get(view.root),
                x: view.root.x,
                y: view.root.y,
                rotation: view.parts.get('main')?.node.rotation ?? 0,
                alpha: view.root.alpha,
                overlays: [...view.overlays.members.keys()].map((node: Container) => ({
                  x: node.x,
                  y: node.y,
                  alpha: node.alpha,
                  visible: node.visible,
                })),
              };
            })
        : [],
    gpuDone,
    stop: () => kernel.app.stop(),
    render: () => kernel.app.render(),
  };
  const destroy = handle.destroy.bind(handle);
  handle.destroy = async () => {
    try {
      await destroy();
    } finally {
      for (const restore of undo.splice(0)) restore();
      bridges.delete(handle);
    }
  };
  bridges.set(handle, probe);
}

export function getPerf(handle: RendererHandle): PerfProbe {
  const probe = bridges.get(handle);
  if (!probe) throw new Error('Use Vite --mode perf for the internal measurement bridge');
  return probe;
}
