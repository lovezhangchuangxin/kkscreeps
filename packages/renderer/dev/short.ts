/** UI-operated short diagnostics, never a substitute for the frozen R7 matrix. */
import './perf.js';

const result = document.querySelector<HTMLPreElement>('#result')!;
const buttons = [...document.querySelectorAll<HTMLButtonElement>('button')];
const pause = () => new Promise(resolve => setTimeout(resolve, 100));
const heap = () => (performance as Performance & { memory?: { usedJSHeapSize: number } }).memory?.usedJSHeapSize ?? null;
const sample = async () => {
  const heapBytes: Array<number | null> = [];
  for (let i = 0; i < 5; i++) { heapBytes.push(heap()); await pause(); }
  return { heapBytes, counts: window.r7.counts() };
};
const run = async (kind: string, work: () => Promise<unknown>) => {
  buttons.forEach(b => { b.disabled = true; });
  const started = performance.now();
  result.textContent = `running ${kind}`;
  try {
    const data = await work();
    result.textContent = JSON.stringify({ kind, elapsedMs: performance.now() - started,
      environment: { userAgent: navigator.userAgent, dpr: devicePixelRatio, date: new Date().toISOString(), visibility: document.visibilityState },
      methodology: { forcedGC: false, heap: 'five unforced samples spaced 100ms; null when unavailable', sourceBytes: 'RGBA8 texel estimate, excludes driver allocations', longSoak: false },
      startup: window.r7.startup, data });
    document.querySelector('#status')!.textContent = `${kind} complete`;
  } catch (error) { result.textContent = `ERROR ${error instanceof Error ? error.stack : String(error)}`; }
  finally { buttons.forEach(b => { b.disabled = kind === 'destroy'; }); }
};
document.querySelector('#cpu')?.addEventListener('click', () => void run('short-cpu', async () => ({
  snapshots: await window.r7.snapshots(40), frames: await window.r7.frames(true, 4, 180, 30),
})));
document.querySelector('#memory')?.addEventListener('click', () => void run('short-pressure', async () => {
  window.r7.initSoak('pressure');
  const warmup = await window.r7.soakChunk(100);
  const windows: unknown[] = [{ tick: warmup.tick, sample: await sample() }];
  for (let i = 0; i < 2; i++) {
    const activity = await window.r7.soakChunk(50);
    windows.push({ tick: activity.tick, activity, sample: await sample() });
  }
  const reset = await window.r7.clear();
  return { warmupTicks: 100, measuredTicks: 100, stepsPerTick: 15, matchingEndpointPhase: 'tick 100 and 200 = pressure phase 0', windows, reset, afterReset: await sample() };
}));
document.querySelector('#parity')?.addEventListener('click', () => void run('cover-reuse-parity', () => window.r7.coverReuseParity()));
document.querySelector('#visual')?.addEventListener('click', () => void run('visual-fingerprint', () => window.r7.visualFingerprint()));
document.querySelector('#visualBench')?.addEventListener('click', () => void run('visual-benchmark', () => window.r7.visualBenchmark()));
document.querySelector('#visualMiss')?.addEventListener('click', () => void run('visual-miss-benchmark', () => window.r7.visualBenchmark(true)));
document.querySelector('#recovery')?.addEventListener('click', () => void run('delta-recovery', () => window.r7.recovery()));
document.querySelector('#matrix')?.addEventListener('click', () => void run('short-matrix', async () => ({
  static: await window.r7.frames(false, 1, 90, 30),
  moving1x: await window.r7.frames(true, 1, 90, 30),
  moving4x: await window.r7.frames(true, 4, 90, 30),
  hover: await window.r7.hover(1000), hoverMove: await window.r7.hoverMove(200),
})));
document.querySelector('#gpu')?.addEventListener('click', () => void run('short-gpu', async () => ({
  serial: await window.r7.serial(60, 20),
  timestamps: new URLSearchParams(location.search).get('gpu') === 'timestamp' ? await window.r7.timestamps(60, 20) : null,
})));
document.querySelector('#repeat')?.addEventListener('click', () => void run('short-cpu-repeat', async () => {
  const trials = [];
  for (let i = 0; i < 3; i++) trials.push({ trial: i + 1, snapshots: await window.r7.snapshots(40), frames: await window.r7.frames(true, 4, 180, 30) });
  return { trials };
}));
document.querySelector('#rebuild')?.addEventListener('click', () => void run('short-rebuild', async () => ({
  rekey: await window.r7.snapshots(30, true), terrain: await window.r7.snapshots(30, false, true),
})));

document.querySelector('#official')!.addEventListener('click', () => {
  const url = new URL(location.href);
  if (url.searchParams.get('fixture') !== 'official-room') { url.searchParams.set('fixture', 'official-room'); location.assign(url); return; }
  void run('official-replay', () => window.r7.officialReplay());
});
document.querySelector('#destroy')!.addEventListener('click', () => void run('destroy', async () => { const before = await sample(); await window.r7.destroy(); return { before, after: await sample(), reloadRequired: true }; }));

// Enable only after all handlers are installed (perf module initializes asynchronously).
buttons.forEach(button => { button.disabled = false; });
