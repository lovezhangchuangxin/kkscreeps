/** Reproducible R7 runner; public feed/animation inputs and perf-only observation. */
import { createRenderer } from '../src/index.js';
import { getPerf } from '../src/perf/probe.js';
import { fullDelta, fullVisuals } from '../src/perf/fixture.js';
import { enableTimestampFeature, makeTimestampTimer } from '../src/perf/gpu-timer.js';
import full from '../fixtures/perf-full-room.json';
import standard from '../fixtures/standard.json';
import type { RoomSnapshot, RoomDelta, RoomVisual } from '@kkscreeps/shared';

const base = full.ticks[0]! as RoomSnapshot;
const status = document.querySelector('#status')!;
const query = new URLSearchParams(location.search);
const requested = query.get('backend') === 'webgl' ? 'webgl' : 'webgpu';
if (query.get('gpu') === 'timestamp') enableTimestampFeature();
const started = performance.now();
let timing = { objectsMs: 0, lightingMs: 0 };
const handle = await createRenderer({ container: document.querySelector<HTMLElement>('#room')!, atlasBaseUrl: new URL('/', location.href).href, fontsBaseUrl: new URL('/', location.href).href, preference: requested, localPlayerId: 'r7-local', onFrameTiming: value => { timing = value; } });
const probe = getPerf(handle);
probe.stop();
const initializedMs = performance.now() - started;
const raf = () => new Promise<number>(resolve => requestAnimationFrame(resolve));
function reset(snapshot: RoomSnapshot = base) { handle.feed.applySnapshot(snapshot); handle.feed.setVisuals(snapshot.room, fullVisuals(0)); handle.setActiveRoom(snapshot.room); handle.fitRoom(); }
reset(); handle.advanceAnimation(0.001); probe.render();
const submittedMs = performance.now() - started;
await probe.gpuDone(); const gpuReadyMs = performance.now() - started;
await raf(); await raf();
const firstFrameMs = performance.now() - started;
const startup = { requested, actual: handle.getStats().backend, initializedMs, submittedMs, gpuReadyMs, firstFrameMs, css: { width: handleContainer().clientWidth, height: handleContainer().clientHeight }, canvas: { width: probe.kernel.app.canvas.width, height: probe.kernel.app.canvas.height }, resolution: probe.kernel.app.renderer.resolution, dpr: devicePixelRatio, visibility: document.visibilityState, stats: handle.getStats(), counts: probe.counts() };
function handleContainer() { return document.querySelector<HTMLElement>('#room')!; }
status.textContent = `R7 ${startup.actual} · 300 objects / 50 creep / 100 visuals · ready`;

let invalidVisibility = false;
document.addEventListener('visibilitychange', () => { if (document.visibilityState !== 'visible') invalidVisibility = true; });

async function frames(moving: boolean, rate: number, samples: number, warmup = 120, visuals = true) {
  reset(); probe.takeApply(); invalidVisibility = document.visibilityState !== 'visible';
  let last = await raf(), elapsed = 0, nextTick = 0, tick = 0;
  const records: Record<string, unknown>[] = [], warmupRecords: Record<string, unknown>[] = [];
  for (let i = -warmup; i < samples; i++) {
    const now = await raf(), intervalMs = now - last; last = now;
    const start = performance.now(), beforeRenders = handle.getStats().lightingPass.renders;
    elapsed += intervalMs * rate;
    let deltaMs = 0, visualMs = 0;
    while (elapsed >= nextTick) {
      const t = performance.now(); handle.feed.applyDelta(fullDelta(base, ++tick, moving)); deltaMs += performance.now() - t;
      if (visuals) { const t = performance.now(); handle.feed.setVisuals(base.room, fullVisuals(tick)); visualMs += performance.now() - t; }
      nextTick += 1000;
    }
    handle.advanceAnimation(intervalMs * rate); probe.render();
    const fullCpuMs = performance.now() - start;
    const apply = probe.takeApply();
    const record = { i, intervalMs, deltaMs, visualMs, fullCpuMs, ...probe.frame(), ...timing, dirty: handle.getStats().lightingPass.renders > beforeRenders, apply };
    (i >= 0 ? records : warmupRecords).push(record);
  }
  await probe.gpuDone();
  return { kind: 'raf', moving, rate, samples, warmup, visuals, invalidVisibility, records, warmupRecords, counts: probe.counts(), stats: handle.getStats() };
}

async function serial(samples: number, warmup = 120) {
  reset(); probe.takeApply(); let tick = 0;
  const records: Record<string, unknown>[] = [];
  for (let i = -warmup; i < samples; i++) {
    await probe.gpuDone();
    if ((i + warmup) % 15 === 0) { handle.feed.applyDelta(fullDelta(base, ++tick)); handle.feed.setVisuals(base.room, fullVisuals(tick)); }
    const start = performance.now(); handle.advanceAnimation(1000 / 15); probe.render(); await probe.gpuDone();
    const completionUpperMs = performance.now() - start;
    const apply = probe.takeApply();
    if (i >= 0) records.push({ i, completionUpperMs, ...probe.frame(), ...timing, apply });
  }
  return { kind: 'serial-completion-upper-bound', samples, warmup, records, counts: probe.counts() };
}

async function timestamps(samples: number, warmup = 120) {
  if (requested !== 'webgpu') throw new Error('Native timestamp entry currently requires WebGPU');
  const device = (probe.kernel.app.renderer as unknown as { gpu: { device: Record<string, any> } }).gpu.device;
  const timer = makeTimestampTimer(device); if (!timer) throw new Error('timestamp-query was not enabled; reload with gpu=timestamp');
  reset(); probe.takeApply(); let tick = 0; const records = [];
  try {
    for (let i = -warmup; i < samples; i++) {
      await probe.gpuDone();
      if ((i + warmup) % 15 === 0) { handle.feed.applyDelta(fullDelta(base, ++tick)); handle.feed.setVisuals(base.room, fullVisuals(tick)); }
      timer.begin(); handle.advanceAnimation(1000 / 15); probe.render(); const frame = probe.frame();
      const gpu = await timer.finish(); probe.takeApply();
      if (i >= 0) records.push({ i, ...gpu, ...frame });
    }
  } finally { timer.destroy(); }
  return { kind: 'native-timestamp-all-render-passes', includesUploadCopyCommands: false, samples, warmup, records, counts: probe.counts() };
}

async function snapshots(samples: number, rekey = false, terrain = false) {
  reset(); probe.takeApply(); const records: Record<string, unknown>[] = [];
  for (let i = 0; i < samples; i++) {
    const snapshot = { ...base, time: i, objects: rekey ? base.objects.map(o => ({ ...o, id: `${o.id}-${i % 2}` })) : base.objects,
      terrain: terrain ? { codes: base.terrain.codes.map((c, n) => n === 1 ? i % 2 : c) } : base.terrain };
    await probe.gpuDone(); const t = performance.now(); handle.feed.applySnapshot(snapshot); const snapshotMs = performance.now() - t;
    const v = performance.now(); handle.feed.setVisuals(base.room, fullVisuals(i)); const visualMs = performance.now() - v;
    handle.advanceAnimation(0.001); probe.render(); const frame = probe.frame(); await probe.gpuDone();
    records.push({ i, snapshotMs, visualMs, combinedApplyMs: snapshotMs + visualMs, ...frame, apply: probe.takeApply() });
  }
  return { kind: 'snapshots', rekey, terrain, records, counts: probe.counts() };
}

function hover(samples: number) {
  reset(); probe.takeApply(); const records = [];
  for (let i = 0; i < samples; i++) { const t = performance.now(); handle.pickAt(i % 1920, (i * 7) % 1080); records.push(performance.now() - t); }
  return { kind: 'pickAt', samples, records };
}

async function ramparts(users: number) {
  const snapshot = { ...base, objects: base.objects.map(o => o.structureType === 'rampart' && !o.isPublic ? { ...o, user: `r7-owner-${Number(o.id.split('-').at(-1)) % users}`, owner: `r7-owner-${Number(o.id.split('-').at(-1)) % users}` } : o) };
  const t = performance.now(); reset(snapshot); const applyMs = performance.now() - t;
  handle.advanceAnimation(0.001); probe.render(); await probe.gpuDone();
  return { kind: 'ramparts', users, applyMs, counts: probe.counts() };
}

let soakTick = 0, soakKind: 'standard' | 'pressure' = 'pressure';
function initSoak(kind: 'standard' | 'pressure') {
  soakKind = kind; soakTick = 0;
  reset(kind === 'pressure' ? base : standard.ticks[0]! as RoomSnapshot);
  probe.takeApply(); return probe.counts();
}
async function soakChunk(ticks: number) {
  for (let n = 0; n < ticks; n++) {
    const tick = ++soakTick;
    if (soakKind === 'pressure') {
      handle.feed.applyDelta(fullDelta(base, tick, true, true));
      handle.feed.setVisuals(base.room, fullVisuals(tick));
      if (tick % 100 === 25 || tick % 100 === 75) handle.feed.setTerrain(base.room, { codes: base.terrain.codes.map((c, i) => i === 1 ? Number(tick % 100 === 25) : c) });
    } else {
      const phase = (tick % 2000) % 30, origin = tick - phase;
      if (phase === 0 || tick % 2000 === 0) reset({ ...standard.ticks[0]! as RoomSnapshot, time: tick });
      else {
        const source = standard.ticks[phase]! as unknown as RoomDelta;
        handle.feed.applyDelta({ ...source, time: tick, upsert: source.upsert.map(o => {
          const copy = { ...o }; for (const key of ['decayTime', 'landTime', 'cooldownTime', 'downgradeTime', 'spawnTime']) if (typeof copy[key] === 'number') copy[key] += origin; return copy;
        }) });
        handle.feed.setVisuals(standard.room, fullVisuals(tick));
      }
    }
    for (let step = 0; step < 15; step++) { handle.advanceAnimation(1000 / 15); probe.render(); }
    await probe.gpuDone(); probe.takeApply();
    if (n % 25 === 0) await new Promise(resolve => setTimeout(resolve, 0));
  }
  return { tick: soakTick, kind: soakKind, counts: probe.counts(), stats: handle.getStats() };
}

async function recovery() {
  const records = [];
  for (const deltaMS of [0, 1000 / 60, 250, 1000, 10000, 60000]) {
    reset(); handle.feed.applyDelta(fullDelta(base, 1, true, true));
    const before = probe.motion(), framesBefore = probe.counts().frames;
    const start = performance.now(); handle.advanceAnimation(deltaMS); const advanceMs = performance.now() - start;
    probe.render(); await probe.gpuDone(); const jumped = probe.motion(), counts = probe.counts();
    const t = performance.now(); handle.advanceAnimation(1000 / 60); probe.render(); await probe.gpuDone(); const recoveryUpperMs = performance.now() - t;
    reset(); handle.feed.applyDelta(fullDelta(base, 1, true, true));
    let remaining = deltaMS;
    while (remaining > 0) { const step = Math.min(remaining, 1000 / 60); handle.advanceAnimation(step); remaining -= step; }
    probe.render(); await probe.gpuDone(); const smallSteps = probe.motion();
    records.push({ deltaMS, advanceMs, framesBefore, framesAfter: counts.frames, before, jumped, smallSteps, counts, recoveryUpperMs }); probe.takeApply();
  }
  return { kind: 'large-delta-recovery', records };
}
async function clear() {
  handle.feed.applySnapshot({ ...base, time: 0, terrain: { codes: new Array(2500).fill(0) }, objects: [] }); handle.advanceAnimation(60000); probe.render(); await probe.gpuDone(); probe.takeApply(); return probe.counts();
}
const api = { startup, frames, serial, timestamps, snapshots, hover, ramparts, initSoak, soakChunk, recovery, clear, counts: () => probe.counts(), destroy: async () => { await handle.destroy(); return probe.counts(); }, gpuDone: probe.gpuDone };
declare global { interface Window { r7: typeof api; } }
window.r7 = api;
void (full.visuals as RoomVisual[]); // fixture type checked alongside the dynamic replacement generator.
