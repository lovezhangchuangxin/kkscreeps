/** Node 24 dependency-free CDP runner for an isolated, headed Chrome profile.
 * Start Vite --mode perf and Chrome --remote-debugging-port=9227 first.
 * Every output embeds its arguments, browser version, timestamp and page startup. */
import { writeFileSync, mkdirSync, rmSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
const packageRoot = process.cwd();
const command = (...args) =>
  execFileSync('rtk', ['proxy', ...args], { cwd: packageRoot, encoding: 'utf8' }).trim();
const digest = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');
const sourceHashes = {};
function hashTree(path) {
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const next = join(path, entry.name);
    if (entry.isDirectory()) hashTree(next);
    else sourceHashes[next] = digest(next);
  }
}
for (const path of ['src', 'dev', 'scripts']) hashTree(path);
const samplingEnvironment = {
  head: command('git', 'rev-parse', 'HEAD'),
  branch: command('git', 'branch', '--show-current'),
  dirty: !!command('git', 'status', '--porcelain'),
  mode: 'Vite --mode perf',
  node: process.version,
  platform: process.platform,
  os: command('sw_vers'),
  power: command('pmset', '-g', 'batt'),
  fixtureSHA256: digest('fixtures/perf-full-room.json'),
  standardSHA256: digest('fixtures/standard.json'),
  sourceSHA256: createHash('sha256').update(JSON.stringify(sourceHashes)).digest('hex'),
  sourceHashes,
};

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const n = args.indexOf(`--${name}`);
  return n < 0 ? fallback : args[n + 1];
};
const action = flag('action', 'startup'),
  backend = flag('backend', 'webgpu');
const output = flag('out', `/tmp/r7-${backend}-${action}.json`);
const version = await (await fetch('http://127.0.0.1:9227/json/version')).json();
const tabs = await (await fetch('http://127.0.0.1:9227/json/list')).json();
const tab =
  tabs.find((t) => t.type === 'page' && t.url.includes('localhost:5174')) ??
  tabs.find((t) => t.type === 'page' && t.url === 'about:blank');
if (!tab) throw new Error('Isolated perf page missing on CDP port 9227');
const ws = new WebSocket(tab.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = reject;
});
let id = 0;
const pending = new Map();
let warnings = 0,
  errors = 0,
  firstWarning;
ws.onmessage = ({ data }) => {
  const message = JSON.parse(data);
  if (message.id) {
    const p = pending.get(message.id);
    pending.delete(message.id);
    message.error ? p.reject(new Error(JSON.stringify(message.error))) : p.resolve(message.result);
  } else if (message.method === 'Runtime.consoleAPICalled') {
    const event = message.params,
      text = event.args.map((a) => a.value ?? a.description ?? '').join(' ');
    if (event.type === 'warning') {
      warnings++;
      if (!firstWarning && text.includes('BindGroup'))
        firstWarning = { text, stack: event.stackTrace };
    }
    if (event.type === 'error') {
      errors++;
      process.stderr.write(text + '\n');
    }
  } else if (message.method === 'Runtime.exceptionThrown') {
    errors++;
    process.stderr.write(JSON.stringify(message.params.exceptionDetails) + '\n');
  }
};
function cdp(method, params = {}) {
  return new Promise((resolve, reject) => {
    const next = ++id;
    pending.set(next, { resolve, reject });
    ws.send(JSON.stringify({ id: next, method, params }));
  });
}
async function evaluate(expression) {
  const r = await cdp('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
await cdp('Runtime.enable');
await cdp('Page.enable');
await cdp('Page.bringToFront');
await cdp('Emulation.setDeviceMetricsOverride', {
  width: 1920,
  height: 1080,
  deviceScaleFactor: 1,
  mobile: false,
});
const profile = flag('profile', '0');
const gpuTimer = flag('gpu', 'none');
if (
  flag('reload', 'false') === 'true' ||
  !tab.url.includes(`backend=${backend}&profile=${profile}&gpu=${gpuTimer}`)
) {
  const oldOrigin = await evaluate('performance.timeOrigin');
  await cdp('Network.enable');
  await cdp('Network.setCacheDisabled', { cacheDisabled: flag('cold', 'false') === 'true' });
  await cdp('Page.navigate', {
    url: `http://localhost:5174/perf.html?smoke=1&backend=${backend}&profile=${profile}&gpu=${gpuTimer}`,
  });
  for (let n = 0; n < 200; n++) {
    if (await evaluate(`performance.timeOrigin !== ${oldOrigin}`)) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}
for (let n = 0; n < 200; n++) {
  if (await evaluate('!!window.r7')) break;
  await new Promise((resolve) => setTimeout(resolve, 100));
  if (n === 199) throw new Error('Perf startup timed out');
}
const startup = await evaluate('window.r7.startup');
const transitionLogs = { warnings, errors, firstWarning };
warnings = 0;
errors = 0;
firstWarning = undefined;
let data;
if (action === 'startup') data = startup;
else if (action === 'memory')
  data = {
    heap: await cdp('Runtime.getHeapUsage'),
    dom: await cdp('Memory.getDOMCounters'),
    counts: await evaluate('window.r7.counts()'),
  };
else if (action === 'stop') {
  data = { heap: await cdp('Runtime.getHeapUsage'), counts: await evaluate('window.r7.counts()') };
  await cdp('Page.navigate', { url: 'about:blank' });
} else if (action === 'raf')
  data = await evaluate(
    `window.r7.frames(${flag('moving', 'true')},${Number(flag('rate', 4))},${Number(flag('samples', 1800))},${Number(flag('warmup', 120))},${flag('visuals', 'true')})`,
  );
else if (action === 'serial')
  data = await evaluate(
    `window.r7.serial(${Number(flag('samples', 1800))},${Number(flag('warmup', 120))})`,
  );
else if (action === 'timestamps')
  data = await evaluate(
    `window.r7.timestamps(${Number(flag('samples', 1800))},${Number(flag('warmup', 120))})`,
  );
else if (action === 'snapshots')
  data = await evaluate(
    `window.r7.snapshots(${Number(flag('samples', 120))},${flag('rekey', 'false')},${flag('terrain', 'false')})`,
  );
else if (action === 'hoverMove')
  data = await evaluate(`window.r7.hoverMove(${Number(flag('samples', 180))})`);
else if (action === 'hover')
  data = await evaluate(`window.r7.hover(${Number(flag('samples', 10000))})`);
else if (action === 'ramparts') {
  data = [];
  for (const concentrated of [false, true])
    for (const users of [1, 4, 16, 40])
      data.push(await evaluate(`window.r7.ramparts(${users},${concentrated})`));
} else if (['cropParity', 'recovery', 'clear', 'destroy', 'counts'].includes(action))
  data = await evaluate(`window.r7.${action}()`);
else if (action === 'soak') {
  const kind = flag('kind', 'pressure'),
    total = Number(flag('ticks', 10000)),
    warmup = Number(flag('warmup', 1000));
  const checkpoints = [],
    windows = [],
    warmupWindows = [];
  const partial = `${output}.partial.json`;
  const savePartial = (state, tick) => {
    mkdirSync(dirname(output), { recursive: true });
    writeFileSync(
      partial,
      JSON.stringify({
        timestamp: new Date().toISOString(),
        args,
        samplingEnvironment,
        browser: version.Browser,
        startup,
        state,
        tick,
        kind,
        total,
        warmup,
        checkpoints,
        windows,
        warmupWindows,
        warnings,
        errors,
        firstWarning,
      }) + '\n',
    );
  };
  await evaluate(`window.r7.initSoak(${JSON.stringify(kind)})`);
  savePartial('warmup', 0);
  for (let tick = 100; tick <= warmup; tick += 100) {
    const state = await evaluate('window.r7.soakChunk(100)');
    warmupWindows.push({ tick, heap: await cdp('Runtime.getHeapUsage'), counts: state.counts });
    await cdp('Runtime.discardConsoleEntries');
    savePartial('warmup', tick);
  }
  if (kind === 'standard') await evaluate(`window.r7.initSoak(${JSON.stringify(kind)})`);
  const checkpoint = async (tick) => {
    await evaluate('window.r7.drainCheckpoint()');
    await cdp('Runtime.discardConsoleEntries');
    await cdp('HeapProfiler.collectGarbage');
    await evaluate('new Promise(resolve=>setTimeout(resolve,0))');
    await cdp('HeapProfiler.collectGarbage');
    const heap = [];
    for (let i = 0; i < 5; i++) {
      heap.push((await cdp('Runtime.getHeapUsage')).usedSize);
      await evaluate('new Promise(resolve=>setTimeout(resolve,0))');
    }
    heap.sort((a, b) => a - b);
    const counts = await evaluate('window.r7.counts()');
    checkpoints.push({
      tick,
      replayTick: tick + (kind === 'pressure' ? warmup : 0),
      phase: (tick + (kind === 'pressure' ? warmup : 0)) % (kind === 'pressure' ? 100 : 2000),
      heapSamples: heap,
      heapMedian: heap[2],
      counts,
    });
    savePartial('sampling', tick);
    process.stdout.write(
      JSON.stringify({
        checkpoint: tick,
        heapMedian: heap[2],
        views: counts.views,
        bindGroups: counts.bindGroups,
        nullSlots: counts.nullSlots,
      }) + '\n',
    );
  };
  await checkpoint(0);
  for (let tick = 100; tick <= total; tick += 100) {
    const state = await evaluate('window.r7.soakChunk(100)');
    const heap = [];
    for (let i = 0; i < 5; i++) {
      heap.push((await cdp('Runtime.getHeapUsage')).usedSize);
      await evaluate('new Promise(resolve=>setTimeout(resolve,0))');
    }
    heap.sort((a, b) => a - b);
    windows.push({
      tick,
      heapMin: heap[0],
      heapMedian: heap[2],
      heapMax: heap[4],
      dying: state.counts.dying,
      overlays: state.counts.overlays,
      effects: state.counts.effects,
      peaks: state.peaks,
    });
    await cdp('Runtime.discardConsoleEntries');
    if (tick % 1000 === 0) await checkpoint(tick);
  }
  data = {
    kind,
    total,
    warmup,
    animationStepsPerTick: 15,
    animationDeltaMS: 1000 / 15,
    checkpointDrainMS: 0.001,
    checkpoints,
    windows,
    warmupWindows,
    baselineReset: await evaluate('window.r7.clear()'),
    destroyed: await evaluate('window.r7.destroy()'),
  };
  rmSync(partial);
} else throw new Error(`Unknown action ${action}`);
const record = {
  timestamp: new Date().toISOString(),
  args,
  samplingEnvironment,
  transitionLogs,
  browser: version.Browser,
  userAgent: version['User-Agent'],
  startup,
  data,
  warnings,
  errors,
  firstWarning,
};
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, JSON.stringify(record) + '\n');
if (flag('screenshot', 'false') !== 'false') {
  const image = await cdp('Page.captureScreenshot', { captureBeyondViewport: false });
  writeFileSync(flag('screenshot'), Buffer.from(image.data, 'base64'));
}
console.log(
  JSON.stringify({
    output,
    startupMs: startup.firstFrameMs,
    backend: startup.actual,
    warnings,
    errors,
    samples: data?.records?.length,
    counts: data?.counts,
  }),
);
ws.close();
