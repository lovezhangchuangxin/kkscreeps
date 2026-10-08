/** Reproducible R7 runner; public feed/animation inputs and perf-only observation. */
import { createRenderer } from '../src/index.js';
import { getPerf } from '../src/perf/probe.js';
import { bakeCanvas } from '../src/core/bake.js';
import { mergedOutlinePath } from '../src/core/path-merge.js';
import { rampartGroups, rampartRasterBounds } from '../src/objects/rampart-groups.js';
import { RAMPART_COVER_SPEC } from '../src/objects/specs.js';
import { textureFromBaked } from '../src/core/bake.js';
import { Rectangle, RenderTexture, Sprite } from 'pixi.js';
import { VisualStage } from '../src/scenes/visual-stage.js';
import { fullDelta, fullVisuals } from '../src/perf/fixture.js';
import { enableTimestampFeature, makeTimestampTimer } from '../src/perf/gpu-timer.js';
import full from '../fixtures/perf-full-room.json';
import standard from '../fixtures/standard.json';
import official from '../fixtures/official-room.json';
import type { RoomSnapshot, RoomDelta, RoomVisual } from '@kkscreeps/shared';

const base = full.ticks[0]! as RoomSnapshot;
const status = document.querySelector('#status')!;
const query = new URLSearchParams(location.search);
const officialMode = query.get('fixture') === 'official-room';
const requested = query.get('backend') === 'webgl' ? 'webgl' : 'webgpu';
if (query.get('gpu') === 'timestamp') enableTimestampFeature();
const started = performance.now();
let timing = { objectsMs: 0, lightingMs: 0 };
const handle = await createRenderer({
  container: document.querySelector<HTMLElement>('#room')!,
  atlasBaseUrl: new URL('/', location.href).href,
  fontsBaseUrl: new URL('/', location.href).href,
  preference: requested,
  localPlayerId: officialMode ? official.provenance.originalPlayer : 'r7-local',
  tickDurationMs: officialMode ? 2500 : 1000,
  onFrameTiming: (value) => {
    timing = value;
  },
});
const probe = getPerf(handle);
probe.stop();
// Short diagnostics can pin physical 1080p without changing the public API or
// the browser's DPR. Record the actual resolution and atlas choice separately.
if (query.get('resolution') === '1')
  probe.kernel.app.renderer.resize(
    handleContainer().clientWidth,
    handleContainer().clientHeight,
    1,
  );
const initializedMs = performance.now() - started;
const raf = () => new Promise<number>((resolve) => requestAnimationFrame(resolve));
let activeRoom = base.room;
function reset(snapshot: RoomSnapshot = base, visuals = true) {
  activeRoom = snapshot.room;
  handle.feed.applySnapshot(snapshot);
  handle.feed.setVisuals(snapshot.room, visuals ? fullVisuals(0) : []);
  handle.setActiveRoom(snapshot.room);
  handle.fitRoom();
}
reset(officialMode ? (official.ticks[0]! as RoomSnapshot) : base, !officialMode);
handle.advanceAnimation(0.001);
probe.render();
const submittedMs = performance.now() - started;
await probe.gpuDone();
const gpuReadyMs = performance.now() - started;
await raf();
await raf();
const firstFrameMs = performance.now() - started;
const startup = {
  requested,
  actual: handle.getStats().backend,
  initializedMs,
  submittedMs,
  gpuReadyMs,
  firstFrameMs,
  css: { width: handleContainer().clientWidth, height: handleContainer().clientHeight },
  canvas: { width: probe.kernel.app.canvas.width, height: probe.kernel.app.canvas.height },
  resolution: probe.kernel.app.renderer.resolution,
  dpr: devicePixelRatio,
  visibility: document.visibilityState,
  stats: handle.getStats(),
  counts: probe.counts(),
};
function handleContainer() {
  return document.querySelector<HTMLElement>('#room')!;
}
status.textContent = `R7 ${startup.actual} · ${officialMode ? 'official-room 312 objects / 11 creep / 0 visual' : '300 objects / 50 creep / 100 visuals'} · ready`;
if (officialMode)
  for (const id of ['cpu', 'memory', 'parity'])
    document.querySelector<HTMLButtonElement>(`#${id}`)?.remove();

let invalidVisibility = false,
  deviceLost = false;
probe.kernel.app.canvas.addEventListener('webglcontextlost', () => {
  deviceLost = true;
});
const device = (
  probe.kernel.app.renderer as unknown as {
    gpu?: { device: { lost: Promise<unknown>; adapterInfo?: unknown; features: Set<string> } };
  }
).gpu?.device;
if (device)
  void device.lost.then(() => {
    deviceLost = true;
  });
const rawAdapter = device?.adapterInfo as Record<string, unknown> | undefined;
const adapterInfo = rawAdapter
  ? Object.fromEntries(
      ['vendor', 'architecture', 'device', 'description', 'isFallbackAdapter'].map((key) => [
        key,
        rawAdapter[key] ?? null,
      ]),
    )
  : null;
Object.assign(startup, {
  adapterInfo,
  gpuFeatures: device ? [...device.features] : [],
  webglRenderer:
    requested === 'webgl'
      ? (() => {
          const gl = (probe.kernel.app.renderer as unknown as { gl: WebGL2RenderingContext }).gl;
          const extension = gl.getExtension('WEBGL_debug_renderer_info');
          return extension
            ? gl.getParameter(extension.UNMASKED_RENDERER_WEBGL)
            : gl.getParameter(gl.RENDERER);
        })()
      : null,
});
document.addEventListener('visibilitychange', () => {
  if (document.visibilityState !== 'visible') invalidVisibility = true;
});

async function frames(
  moving: boolean,
  rate: number,
  samples: number,
  warmup = 120,
  visuals = true,
) {
  reset();
  probe.takeApply();
  invalidVisibility = document.visibilityState !== 'visible';
  let last = await raf(),
    elapsed = 0,
    nextTick = 0,
    tick = 0;
  const records: Record<string, unknown>[] = [],
    warmupRecords: Record<string, unknown>[] = [];
  for (let i = -warmup; i < samples; i++) {
    const now = await raf(),
      intervalMs = now - last;
    last = now;
    const start = performance.now(),
      beforeRenders = handle.getStats().lightingPass.renders;
    elapsed += intervalMs * rate;
    let deltaMs = 0,
      visualMs = 0;
    while (elapsed >= nextTick) {
      const t = performance.now();
      handle.feed.applyDelta(fullDelta(base, ++tick, moving));
      deltaMs += performance.now() - t;
      if (visuals) {
        const t = performance.now();
        handle.feed.setVisuals(base.room, fullVisuals(tick));
        visualMs += performance.now() - t;
      }
      nextTick += 1000;
    }
    handle.advanceAnimation(intervalMs * rate);
    probe.render();
    const fullCpuMs = performance.now() - start;
    const apply = probe.takeApply();
    const record = {
      i,
      feed: probe.takeFeed(),
      intervalMs,
      deltaMs,
      visualMs,
      fullCpuMs,
      ...probe.frame(),
      ...timing,
      dirty: handle.getStats().lightingPass.renders > beforeRenders,
      apply,
    };
    (i >= 0 ? records : warmupRecords).push(record);
  }
  await probe.gpuDone();
  return {
    kind: 'raf',
    moving,
    rate,
    samples,
    warmup,
    visuals,
    invalidVisibility,
    deviceLost,
    records,
    warmupRecords,
    counts: probe.counts(),
    stats: handle.getStats(),
  };
}

async function serial(samples: number, warmup = 120) {
  reset();
  probe.takeApply();
  let tick = 0;
  const records: Record<string, unknown>[] = [],
    warmupRecords: Record<string, unknown>[] = [];
  invalidVisibility = document.visibilityState !== 'visible';
  for (let i = -warmup; i < samples; i++) {
    await probe.gpuDone();
    if ((i + warmup) % 15 === 0) {
      handle.feed.applyDelta(fullDelta(base, ++tick));
      handle.feed.setVisuals(base.room, fullVisuals(tick));
    }
    const start = performance.now();
    handle.advanceAnimation(1000 / 15);
    probe.render();
    await probe.gpuDone();
    const completionUpperMs = performance.now() - start;
    const apply = probe.takeApply();
    (i >= 0 ? records : warmupRecords).push({
      i,
      completionUpperMs,
      ...probe.frame(),
      ...timing,
      apply,
    });
  }
  return {
    kind: 'serial-completion-upper-bound',
    samples,
    warmup,
    invalidVisibility,
    deviceLost,
    records,
    warmupRecords,
    counts: probe.counts(),
  };
}

async function timestamps(samples: number, warmup = 120) {
  if (requested !== 'webgpu') throw new Error('Native timestamp entry currently requires WebGPU');
  const device = (probe.kernel.app.renderer as unknown as { gpu: { device: Record<string, any> } })
    .gpu.device;
  const timer = makeTimestampTimer(device);
  if (!timer) throw new Error('timestamp-query was not enabled; reload with gpu=timestamp');
  reset();
  probe.takeApply();
  let tick = 0;
  const records: Record<string, unknown>[] = [],
    warmupRecords: Record<string, unknown>[] = [];
  invalidVisibility = document.visibilityState !== 'visible';
  try {
    for (let i = -warmup; i < samples; i++) {
      await probe.gpuDone();
      if ((i + warmup) % 15 === 0) {
        handle.feed.applyDelta(fullDelta(base, ++tick));
        handle.feed.setVisuals(base.room, fullVisuals(tick));
      }
      timer.begin();
      handle.advanceAnimation(1000 / 15);
      probe.render();
      const frame = probe.frame();
      const gpu = await timer.finish();
      probe.takeApply();
      (i >= 0 ? records : warmupRecords).push({ i, ...gpu, ...frame });
    }
  } finally {
    timer.destroy();
  }
  return {
    kind: 'native-timestamp-all-render-passes',
    includesUploadCopyCommands: false,
    samples,
    warmup,
    invalidVisibility,
    deviceLost,
    records,
    warmupRecords,
    counts: probe.counts(),
  };
}

async function snapshots(samples: number, rekey = false, terrain = false) {
  reset();
  probe.takeApply();
  const records: Record<string, unknown>[] = [];
  for (let i = 0; i < samples; i++) {
    const snapshot = {
      ...base,
      time: i,
      objects: rekey ? base.objects.map((o) => ({ ...o, id: `${o.id}-${i % 2}` })) : base.objects,
      terrain: terrain
        ? { codes: base.terrain.codes.map((c, n) => (n === 1 ? i % 2 : c)) }
        : base.terrain,
    };
    await probe.gpuDone();
    const t = performance.now();
    handle.feed.applySnapshot(snapshot);
    const snapshotMs = performance.now() - t;
    const v = performance.now();
    handle.feed.setVisuals(base.room, fullVisuals(i));
    const visualMs = performance.now() - v;
    const frameStart = performance.now();
    handle.advanceAnimation(0.001);
    probe.render();
    const frame = probe.frame();
    await probe.gpuDone();
    const completed = performance.now();
    records.push({
      i,
      snapshotMs,
      visualMs,
      combinedApplyMs: snapshotMs + visualMs,
      frameCompletionUpperMs: completed - frameStart,
      applyAndFrameCompletionUpperMs: completed - t,
      feed: probe.takeFeed(),
      ...frame,
      apply: probe.takeApply(),
    });
  }
  return { kind: 'snapshots', rekey, terrain, records, counts: probe.counts() };
}

async function hoverMove(samples: number) {
  reset();
  const canvas = probe.kernel.app.canvas,
    bounds = canvas.getBoundingClientRect();
  const records = [];
  let last = await raf(),
    tick = 0;
  for (let i = 0; i < samples; i++) {
    const now = await raf(),
      intervalMs = now - last;
    last = now;
    const t = performance.now();
    for (let n = 0; n < 10; n++)
      canvas.dispatchEvent(
        new PointerEvent('pointermove', {
          clientX: bounds.left + ((i * 13 + n) % 1920),
          clientY: bounds.top + ((i * 19 + n) % 1080),
          pointerId: 1,
          isPrimary: true,
        }),
      );
    const pointerMs = performance.now() - t;
    let cameraMs = 0,
      dataMs = 0;
    if (i % 30 === 0) {
      const c = performance.now();
      handle.setCameraState({ x: 2450 + (i % 60 ? 100 : 0), y: 2450, zoom: 0.216 });
      cameraMs = performance.now() - c;
    }
    if (i % 15 === 0) {
      const c = performance.now();
      handle.feed.applyDelta(fullDelta(base, ++tick));
      dataMs = performance.now() - c;
    }
    handle.advanceAnimation(intervalMs * 4);
    probe.render();
    probe.takeApply();
    probe.takeFeed();
    records.push({ i, pointerMs, cameraMs, dataMs, intervalMs, ...probe.frame() });
  }
  await probe.gpuDone();
  return {
    kind: 'DOM-hover-camera-data',
    samples,
    pointerMovesPerFrame: 10,
    records,
    counts: probe.counts(),
  };
}
function hover(samples: number) {
  reset();
  probe.takeApply();
  const records = [];
  for (let i = 0; i < samples; i++) {
    const t = performance.now();
    handle.pickAt(i % 1920, (i * 7) % 1080);
    records.push(performance.now() - t);
  }
  return { kind: 'pickAt', samples, records };
}

async function ramparts(users: number, concentrated = false) {
  const snapshot = {
    ...base,
    objects: base.objects.map((o) =>
      o.structureType === 'rampart' && !o.isPublic
        ? {
            ...o,
            ...(concentrated
              ? {
                  x: 20 + (Number(o.id.split('-').at(-1)) % 8),
                  y: 20 + Math.floor(Number(o.id.split('-').at(-1)) / 8),
                }
              : {}),
            user: `r7-owner-${Number(o.id.split('-').at(-1)) % users}`,
            owner: `r7-owner-${Number(o.id.split('-').at(-1)) % users}`,
          }
        : o,
    ),
  };
  const t = performance.now();
  reset(snapshot);
  const applyMs = performance.now() - t;
  handle.advanceAnimation(0.001);
  probe.render();
  await probe.gpuDone();
  return { kind: 'ramparts', users, concentrated, applyMs, counts: probe.counts() };
}

function shiftDeadlines(object: RoomSnapshot['objects'][number], offset: number) {
  const copy = { ...object };
  for (const key of [
    'decayTime',
    'landTime',
    'cooldownTime',
    'downgradeTime',
    'spawnTime',
    'safeMode',
  ])
    if (typeof copy[key] === 'number' && copy[key] > 0) copy[key] += offset;
  if (copy.spawning && typeof copy.spawning === 'object') {
    const spawn = { ...(copy.spawning as Record<string, unknown>) };
    if (typeof spawn.spawnTime === 'number') spawn.spawnTime += offset;
    copy.spawning = spawn;
  }
  if (copy.effects && typeof copy.effects === 'object')
    copy.effects = Object.fromEntries(
      Object.entries(copy.effects).map(([key, value]) => [
        key,
        value &&
        typeof value === 'object' &&
        'endTime' in value &&
        typeof value.endTime === 'number'
          ? { ...value, endTime: value.endTime + offset }
          : value,
      ]),
    );
  return copy;
}
let soakTick = 0,
  soakKind: 'standard' | 'pressure' = 'pressure';
function initSoak(kind: 'standard' | 'pressure') {
  soakKind = kind;
  soakTick = 0;
  reset(kind === 'pressure' ? base : (standard.ticks[0]! as RoomSnapshot));
  probe.takeApply();
  return probe.counts();
}
async function soakChunk(ticks: number) {
  const peaks: Record<string, number> = {};
  const sampleActivity = () => {
    for (const [key, count] of Object.entries(probe.activity()))
      peaks[key] = Math.max(peaks[key] ?? 0, count);
  };
  for (let n = 0; n < ticks; n++) {
    const tick = ++soakTick;
    if (soakKind === 'pressure') {
      const delta = fullDelta(base, tick, true, true);
      if (tick % 100 === 25 || tick % 100 === 75)
        delta.upsert.push(
          ...base.objects
            .filter((o) => o.structureType === 'rampart' && !o.isPublic)
            .map((o) => ({
              ...o,
              user: `r7-soak-owner-${(Number(o.id.split('-').at(-1)) + Number(tick % 100 === 25)) % 4}`,
            })),
        );
      if (tick % 10 === 5)
        delta.actionLogs = delta.upsert
          .filter((o) => o.type === 'creep')
          .slice(0, 5)
          .map((o) => ({
            id: o.id,
            actionLog: {
              build: {
                x: 10,
                y: 10,
                targetId: base.objects.find((v) => v.type === 'constructionSite')!.id,
              },
            },
          }));
      handle.feed.applyDelta(delta);
      handle.feed.setVisuals(base.room, fullVisuals(tick));
      if (tick % 100 === 25 || tick % 100 === 75)
        handle.feed.setTerrain(base.room, {
          codes: base.terrain.codes.map((c, i) => (i === 1 ? Number(tick % 100 === 25) : c)),
        });
    } else {
      const phase = (tick % 2000) % 30,
        origin = tick - phase;
      if (phase === 0 || tick % 2000 === 0)
        reset({
          ...(standard.ticks[0]! as RoomSnapshot),
          time: tick,
          objects: (standard.ticks[0]! as RoomSnapshot).objects.map((o) => shiftDeadlines(o, tick)),
        });
      else {
        const source = standard.ticks[phase]! as unknown as RoomDelta;
        handle.feed.applyDelta({
          ...source,
          time: tick,
          upsert: source.upsert.map((o) => shiftDeadlines(o, origin)),
        });
        handle.feed.setVisuals(standard.room, fullVisuals(tick));
      }
    }
    sampleActivity();
    for (let step = 0; step < 15; step++) {
      handle.advanceAnimation(1000 / 15);
      probe.render();
      if (step === 0 || step === 7 || step === 14) sampleActivity();
    }
    await probe.gpuDone();
    probe.takeApply();
    probe.takeFeed();
    if (n % 25 === 0) await new Promise((resolve) => setTimeout(resolve, 0));
  }
  return {
    tick: soakTick,
    kind: soakKind,
    peaks,
    counts: probe.counts(),
    stats: handle.getStats(),
  };
}

async function recovery() {
  const records = [];
  for (const deltaMS of [0, 1000 / 60, 250, 1000, 10000, 60000]) {
    reset();
    handle.feed.applyDelta(fullDelta(base, 1, true, true));
    const before = probe.motion(),
      framesBefore = probe.counts().frames;
    const start = performance.now();
    handle.advanceAnimation(deltaMS);
    const advanceMs = performance.now() - start;
    probe.render();
    await probe.gpuDone();
    const jumped = probe.motion(),
      counts = probe.counts();
    const t = performance.now();
    handle.advanceAnimation(1000 / 60);
    probe.render();
    await probe.gpuDone();
    const recoveryUpperMs = performance.now() - t;
    reset();
    handle.feed.applyDelta(fullDelta(base, 1, true, true));
    let remaining = deltaMS;
    while (remaining > 0) {
      const step = Math.min(remaining, 1000 / 60);
      handle.advanceAnimation(step);
      remaining -= step;
    }
    probe.render();
    await probe.gpuDone();
    const smallSteps = probe.motion();
    records.push({
      deltaMS,
      advanceMs,
      framesBefore,
      framesAfter: counts.frames,
      before,
      jumped,
      smallSteps,
      counts,
      recoveryUpperMs,
    });
    probe.takeApply();
  }
  return { kind: 'large-delta-recovery', records };
}
async function clear() {
  handle.feed.applySnapshot({
    ...base,
    room: activeRoom,
    time: 0,
    terrain: { codes: new Array(2500).fill(0) },
    objects: [],
  });
  handle.advanceAnimation(60000);
  probe.render();
  await probe.gpuDone();
  probe.takeApply();
  return probe.counts();
}
function cropParity() {
  const cases = [
    [0],
    [49],
    [2450],
    [2499],
    [0, 1, 50, 51],
    [510, 511, 560, 562],
    [0, 49, 2450, 2499],
    Array.from({ length: 40 }, (_, i) => 110 + i),
  ];
  const records = [];
  for (const cell of [64, 100])
    for (const renderSize of [1024, 2048])
      for (const cells of cases) {
        const view = cell * 50,
          codes = new Uint8Array(2500);
        for (const i of cells) codes[i] = 1;
        const path = mergedOutlinePath(codes, (c) => c !== 0, { cell }).path ?? '';
        const paint = (ctx: CanvasRenderingContext2D, p: Path2D) => {
          ctx.lineWidth = 25;
          ctx.strokeStyle = '#ff8844';
          ctx.stroke(p);
          ctx.fillStyle = '#4488ff';
          ctx.fill(p);
        };
        const original = bakeCanvas(path, view, renderSize, paint),
          bounds = rampartRasterBounds(cells, cell, view, renderSize, 25);
        const cropped = bakeCanvas(path, view, renderSize, paint, bounds);
        const restored = document.createElement('canvas');
        restored.width = restored.height = renderSize;
        const ctx = restored.getContext('2d')!;
        ctx.drawImage(cropped, bounds.x, bounds.y);
        const a = original.getContext('2d')!.getImageData(0, 0, renderSize, renderSize).data,
          b = ctx.getImageData(0, 0, renderSize, renderSize).data;
        let differingChannels = 0,
          maxDifference = 0;
        for (let i = 0; i < a.length; i++)
          if (a[i] !== b[i]) {
            differingChannels++;
            maxDifference = Math.max(maxDifference, Math.abs(a[i]! - b[i]!));
          }
        records.push({ cell, renderSize, cells, bounds, differingChannels, maxDifference });
      }
  return records;
}
async function coverReuseParity() {
  const records = [];
  let previous: Sprite | undefined;
  let previousCanvas: HTMLCanvasElement | undefined,
    previousSource: Sprite['texture']['source'] | undefined;
  const prototype = base.objects.find((o) => o.structureType === 'rampart')!;
  for (const cells of [[510], [510, 511, 560, 561], [2499], [0, 49, 2450, 2499], []]) {
    const objects = cells.map((cell, i) => ({
      ...prototype,
      id: `r7-reuse-${i}`,
      user: 'r7-reuse',
      owner: 'r7-reuse',
      isPublic: false,
      x: cell % 50,
      y: Math.floor(cell / 50),
    }));
    reset({ ...base, objects });
    handle.advanceAnimation(0.001);
    probe.render();
    await probe.gpuDone();
    const node = probe.kernel.stage.getChildByLabel('cover-r7-reuse', true);
    if (cells.length === 0) {
      records.push({
        cells,
        removed: node === null,
        oldSpriteDestroyed: previous?.destroyed,
        oldSourceDestroyed: previousSource?.destroyed,
      });
      continue;
    }
    if (!(node instanceof Sprite)) throw new Error('Missing reused cover');
    const source = node.texture.source,
      canvas = source.resource as HTMLCanvasElement;
    const region = rampartRasterBounds(cells, 100, 5000, 2048, RAMPART_COVER_SPEC.strokeWidth);
    const group = rampartGroups(objects, { isOwn: () => false })[0]!;
    const codes = new Uint8Array(2500);
    cells.forEach((i) => {
      codes[i] = 1;
    });
    const path = mergedOutlinePath(codes, (c) => c !== 0, { cell: 100 }).path ?? '';
    const freshCanvas = bakeCanvas(
      path,
      5000,
      2048,
      (ctx, p) => {
        ctx.lineWidth = RAMPART_COVER_SPEC.strokeWidth;
        ctx.strokeStyle = `#${group.stroke.toString(16).padStart(6, '0')}`;
        ctx.stroke(p);
        ctx.fillStyle = `#${group.fill.toString(16).padStart(6, '0')}`;
        ctx.fill(p);
      },
      region,
    );
    const fresh = textureFromBaked(freshCanvas);
    const a = probe.kernel.app.renderer.extract.pixels(node.texture),
      b = probe.kernel.app.renderer.extract.pixels(fresh);
    let differences = 0,
      maxDifference = 0;
    for (let i = 0; i < a.pixels.length; i++)
      if (a.pixels[i] !== b.pixels[i]) {
        differences++;
        maxDifference = Math.max(maxDifference, Math.abs(a.pixels[i]! - b.pixels[i]!));
      }
    records.push({
      cells,
      region,
      sourceUID: source.uid,
      sameSprite: previous ? node === previous : true,
      sameSource: previousSource ? source === previousSource : true,
      sameCanvas: previous ? canvas === previousCanvas : true,
      textureWidth: node.texture.width,
      textureHeight: node.texture.height,
      width: node.width,
      height: node.height,
      gpuSizeMatches: a.width === b.width && a.height === b.height,
      differences,
      maxDifference,
    });
    fresh.destroy(true);
    previous = node;
    previousCanvas = canvas;
    previousSource = source;
  }
  return records;
}
async function officialReplay() {
  if (query.get('fixture') !== 'official-room')
    throw new Error(
      'Reload with ?fixture=official-room for original player and 2500ms tick duration',
    );
  // Same complete recording as the interactive demo, public feed only; no synthetic visuals.
  const snapshot = official.ticks[0]! as RoomSnapshot;
  reset(snapshot, false);
  const records = [];
  probe.takeApply();
  probe.takeFeed();
  for (const delta of official.ticks.slice(1)) {
    const start = performance.now();
    handle.feed.applyDelta(delta as RoomDelta);
    const applyMs = performance.now() - start;
    for (let i = 0; i < 15; i++) {
      handle.advanceAnimation(2500 / 15);
      probe.render();
    }
    await probe.gpuDone();
    records.push({ time: delta.time, applyMs, counts: probe.counts() });
    probe.takeApply();
    probe.takeFeed();
  }
  return {
    room: official.room,
    player: official.provenance.originalPlayer,
    tickDurationMs: 2500,
    ticks: official.ticks.length,
    stepsPerTick: 15,
    syntheticVisuals: 0,
    startupScene: 'official recording initial snapshot with 0 synthetic visuals',
    records,
    final: probe.counts(),
    reset: await clear(),
  };
}
async function visualFingerprint() {
  const stage = new VisualStage(640 / 50);
  const text: RoomVisual = {
    type: 'text',
    x: 10,
    y: 10,
    text: 'Room 中文',
    style: {
      color: 'transparent',
      stroke: '#ff000080',
      backgroundColor: '#00000080',
      opacity: 0.5,
    },
  };
  const cases: readonly RoomVisual[][] = [
    fullVisuals(0),
    fullVisuals(1),
    fullVisuals(2),
    [text],
    [{ ...text, text: 'reset', style: {} }],
    [text],
    [
      {
        type: 'rect',
        x: 10,
        y: 10,
        w: 3,
        h: 2,
        style: { fill: 'transparent', stroke: '#ffffff', lineStyle: 'dashed' },
      },
    ],
    [],
  ];
  const records = [];
  try {
    for (const [index, visuals] of cases.entries()) {
      stage.setVisuals(visuals);
      const readback = probe.kernel.app.renderer.extract.pixels({
        target: stage.root,
        frame: new Rectangle(0, 0, 640, 640),
        resolution: 1,
      });
      const bytes = Uint8Array.from(readback.pixels);
      const digest = await crypto.subtle.digest('SHA-256', bytes);
      const compressed = new Uint8Array(
        await new Response(
          new Blob([bytes]).stream().pipeThrough(new CompressionStream('gzip')),
        ).arrayBuffer(),
      );
      let encoded = '';
      for (const byte of compressed) encoded += String.fromCharCode(byte);
      records.push({
        index,
        count: stage.count,
        width: readback.width,
        height: readback.height,
        nonzeroChannels: bytes.reduce((n, b) => n + Number(b !== 0), 0),
        rgbaGzipBase64: btoa(encoded),
        sha256: [...new Uint8Array(digest)].map((n) => n.toString(16).padStart(2, '0')).join(''),
      });
    }
    return {
      method:
        'isolated VisualStage, 640x640 RGBA GPU readback; same ordered replacement inputs before/after optimization',
      records,
    };
  } finally {
    stage.destroy();
    await probe.gpuDone();
  }
}
async function visualBenchmark(unique = false) {
  const records = [];
  // ABBA order limits monotonic warm-up/order effects; each arm owns its stage.
  for (const rebuild of [true, false, false, true]) {
    const stage = new VisualStage(640 / 50),
      target = RenderTexture.create({ width: 640, height: 640, resolution: 1 });
    try {
      for (let i = -20; i < 60; i++) {
        await probe.gpuDone();
        const visuals = fullVisuals(i + 20);
        if (unique)
          for (const visual of visuals) {
            const offset = (i + 20) * 0.071;
            if (visual.type === 'line' || visual.type === 'poly')
              visual.points = visual.points.map((point) => ({ x: point.x + offset, y: point.y }));
            else visual.x += offset;
          }
        const started = performance.now();
        if (rebuild) stage.setVisuals([]);
        stage.setVisuals(visuals);
        const updated = performance.now();
        probe.kernel.app.renderer.render({ container: stage.root, target, clear: true });
        const submitted = performance.now();
        await probe.gpuDone();
        if (i >= 0)
          records.push({
            rebuild,
            i,
            updateMs: updated - started,
            renderSubmitMs: submitted - updated,
            completionUpperMs: performance.now() - started,
          });
      }
    } finally {
      stage.destroy();
      target.destroy(true);
      await probe.gpuDone();
    }
  }
  return {
    unique,
    method:
      'ABBA 4 arms; each 20 warm-up + 60 samples; isolated 100 visuals into 640x640 RT; rebuild arm clears all entries before replacement; completion includes GPU wait',
    records,
  };
}
const api = {
  startup,
  visualBenchmark,
  visualFingerprint,
  cropParity,
  coverReuseParity,
  officialReplay,
  hoverMove,
  frames,
  serial,
  timestamps,
  snapshots,
  hover,
  ramparts,
  initSoak,
  soakChunk,
  recovery,
  clear,
  drainCheckpoint: async () => {
    handle.advanceAnimation(0.001);
    probe.render();
    await probe.gpuDone();
    return probe.counts();
  },
  counts: () => probe.counts(),
  destroy: async () => {
    await handle.destroy();
    return probe.counts();
  },
  gpuDone: probe.gpuDone,
};
declare global {
  interface Window {
    r7: typeof api;
  }
}
window.r7 = api;
void (full.visuals as RoomVisual[]); // fixture type checked alongside the dynamic replacement generator.
if (query.has('zoom')) {
  const result = document.createElement('pre');
  result.id = 'zoom-result';
  Object.assign(result.style, {
    position: 'fixed',
    inset: '30px 4px 4px',
    overflow: 'auto',
    color: 'white',
    background: '#111',
  });
  document.body.append(result);
  try {
    if (!officialMode) throw new Error('Zoom regression requires fixture=official-room');
    const { verifyZoom } = await import('./zoom-verification.js');
    result.textContent = JSON.stringify(
      await verifyZoom(handle, probe, official.ticks[0]! as RoomSnapshot),
      null,
      2,
    );
    status.textContent = 'Zoom GPU regression complete';
  } catch (error) {
    result.textContent = String(error);
    status.textContent = 'Zoom GPU regression failed';
  }
}
