/**
 * dev harness（R1 建，R5 扩）：回放夹具驱动对象渲染（回放控制 1-9/0 切夹具/
 * 空格/./+/-/F）。?player=<id> 切本玩家视角（缺省 p1；?player=x 全敌对色），
 * ?backend=webgl 强制回退路径。HUD 属 harness 交互辅助，不属渲染器（§2.2）。
 */
import { isDelta, isSnapshot, type FixtureFile } from '@kkscreeps/shared';
import { createRenderer } from '../src/index.js';
import type { RoomScene } from '../src/scenes/room.js';
import creeps from '../fixtures/creeps.json';
import denseEdge from '../fixtures/dense-edge.json';
import plains from '../fixtures/plains.json';
import showcase from '../fixtures/showcase.json';
import standard from '../fixtures/standard.json';
import swampWalls from '../fixtures/swamp-walls.json';
import actions from '../fixtures/actions.json';
import effects from '../fixtures/effects.json';
import lifecycle from '../fixtures/lifecycle.json';
import pressure from '../fixtures/pressure.json';

const stageHost = document.getElementById('stage') as HTMLElement;
const hud = document.getElementById('hud') as HTMLElement;

// 文字件（spawn 名/mineral 符号）需 Roboto 先于首帧就绪（缺省回退字体即
// G7 基线漂移）；加载失败不阻塞（allSettled——回退观感属 G7 盯梢面）。
await Promise.allSettled([
  document.fonts.load('55px Roboto'),
  document.fonts.load('bold 82px Roboto'),
  document.fonts.load('60px Roboto'),
]);

const params = new URLSearchParams(window.location.search);
const renderer = await createRenderer({
  container: stageHost,
  atlasBaseUrl: new URL('/', window.location.href).href,
  // ?backend=webgl 强制回退路径（§5：WebGL 同场景跑通记数不设门）
  preference: params.get('backend') === 'webgl' ? 'webgl' : 'webgpu',
  // 本玩家视角（G9：userOwnerColor 色板 own 判定）
  localPlayerId: params.get('player') ?? 'p1',
});
// dev 调试句柄（harness 专用，不属公共 API；__room 由 index.ts 以运行时
// 附加字段提供——诊断场景树用，R6 冻结时随公共面裁定去留）
(window as unknown as Record<string, unknown>).__renderer = renderer;

// 夹具装载：入库 JSON 经 shared 守卫校验（防夹具与契约漂移）
const rawFixtures: Record<string, unknown> = {
  plains,
  standard,
  'swamp-walls': swampWalls,
  'dense-edge': denseEdge,
  creeps,
  showcase,
  actions,
  effects,
  lifecycle,
  pressure,
};
const fixtures = new Map<string, FixtureFile>();
for (const [key, value] of Object.entries(rawFixtures)) {
  const f = value as FixtureFile;
  if (!isSnapshot(f.ticks[0])) throw new Error(`夹具 ${key} 首元素不是快照`);
  if (!f.ticks.slice(1).every((t) => isDelta(t))) throw new Error(`夹具 ${key} 存在非增量元素`);
  fixtures.set(key, f);
}

// ---- 回放状态机（G6：tickDuration 缺省 1000ms，契约参数化）----
const TICK_MS = 1000;
const SPEEDS = [0.5, 1, 2, 4] as const;
let current = 'standard';
let index = 0;
let playing = false;
let speedIdx = 1; // 1×
let timer: number | undefined;
let replayError: string | null = null;
let sampleElapsed = 0;
const diagnosticRoom = (renderer as unknown as { __room: RoomScene }).__room;
// ?smoke=1 仅计实际场景更新与 lighting 提交的 CPU 时间；不冒充 GPU 耗时。
const samples = { objects: [] as number[], lighting: [] as number[] };
if (params.has('smoke')) {
  const measure = <T extends object, K extends keyof T>(target: T, key: K, values: number[]) => {
    const original = target[key] as (...args: unknown[]) => unknown;
    target[key] = function (...args: unknown[]) {
      const start = performance.now();
      try { return original.apply(target, args); }
      finally { if (playing && values.length < 10000) values.push(performance.now() - start); }
    } as T[K];
  };
  measure(diagnosticRoom.objects!, 'update', samples.objects);
  measure(diagnosticRoom.lighting, 'update', samples.lighting);
}
function sampleFrame(ms: number): void {
  if (!Number.isFinite(ms) || ms < 0) return;
  diagnosticRoom.objects?.update(ms);
  sampleElapsed += ms;
  renderHud();
}

function fixture(): FixtureFile {
  return fixtures.get(current)!;
}

function applyEntry(): void {
  const f = fixture();
  if (index >= f.ticks.length) {
    playing = false;
    renderer.setPlaybackRate(0);
    stopTimer(); // 末态清定时器（评审 C 建议 1：不留空转僵尸 interval）
    return;
  }
  const entry = f.ticks[index]!;
  sampleElapsed = 0;
  if (isSnapshot(entry)) renderer.feed.applySnapshot(entry);
  else renderer.feed.applyDelta(entry);
  index += 1;
}

/** 定时器回调的兜底（评审 C 建议 2）：fail-fast 抛错 → 停机 + HUD 报错，
 * 不留 1-4Hz 异常循环。 */
function stepGuarded(): void {
  try {
    applyEntry();
  } catch (e) {
    stopTimer();
    playing = false;
    renderer.setPlaybackRate(0);
    replayError = e instanceof Error ? e.message : String(e);
    renderHud();
  }
}

function stopTimer(): void {
  if (timer !== undefined) {
    clearInterval(timer);
    timer = undefined;
  }
}

function setPlaying(next: boolean): void {
  playing = next && index < fixture().ticks.length;
  stopTimer();
  renderer.setPlaybackRate(playing ? SPEEDS[speedIdx]! : 0);
  if (playing) timer = window.setInterval(stepGuarded, TICK_MS / SPEEDS[speedIdx]!);
  renderHud();
}

function loadFixture(key: string): void {
  if (!fixtures.has(key)) return;
  stopTimer();
  current = key;
  index = 0;
  replayError = null;
  applyEntry(); // 首快照
  setPlaying(playing);
}

function stepOnce(): void {
  stopTimer();
  playing = false;
  renderer.setPlaybackRate(0);
  try {
    applyEntry();
  } catch (e) {
    replayError = e instanceof Error ? e.message : String(e);
  }
  renderHud();
}

function cycleSpeed(): void {
  speedIdx = (speedIdx + 1) % SPEEDS.length;
  renderer.setPlaybackRate(playing ? SPEEDS[speedIdx]! : 0);
  if (playing) setPlaying(true);
  else renderHud();
}

// ---- 输入 ----
window.addEventListener('keydown', (e) => {
  if (e.key === 'f' || e.key === 'F') renderer.fitRoom();
  else if (e.key === ' ') {
    e.preventDefault();
    setPlaying(!playing);
  } else if (e.key === '.') stepOnce();
  else if (e.key === '+' || e.key === '=') cycleSpeed();
  else if (e.key === '-' || e.key === '_') {
    speedIdx = (speedIdx + SPEEDS.length - 1) % SPEEDS.length;
    renderer.setPlaybackRate(playing ? SPEEDS[speedIdx]! : 0);
    if (playing) setPlaying(true);
    else renderHud();
  } else if (e.key === 'ArrowRight' && !playing) {
    e.preventDefault(); sampleFrame(50);
  } else {
    const keys = [...fixtures.keys()];
    const i = e.key === '0' ? 9 : Number(e.key) - 1;
    if (i >= 0 && i < keys.length) loadFixture(keys[i]!);
  }
});
// resize 由 renderer 内部同步，保留世界锚点/缩放；F 显式适配整个房间。
// ?view=<x>,<y>,<zoom>（世界坐标/缩放）：截图钉住初始相机。
const viewParam = params.get('view');
if (viewParam) {
  const [vx, vy, vz] = viewParam.split(',').map(Number);
  const applyPinned = () => renderer.setCameraState({ x: vx!, y: vy!, zoom: vz! });
  requestAnimationFrame(() => requestAnimationFrame(applyPinned));
}
renderer.fitRoom();
loadFixture(params.get('fixture') ?? current);
const initialTick = Number(params.get('tick') ?? 0);
while (index <= initialTick && index < fixture().ticks.length) {
  // Seeking consumes completed intervals, so tick N does not retain all of
  // tick 1..N-1's beams. The selected tick starts with finite-effect age zero.
  if (index > 1) diagnosticRoom.objects?.update(TICK_MS);
  applyEntry();
}
if (params.has('sample')) sampleFrame(Number(params.get('sample')));

// ---- HUD ----
function renderHud(): void {
  const s = renderer.getStats();
  const f = fixture();
  const summary = renderer.feed.summary(f.room);
  const state = index >= f.ticks.length ? 'end' : playing ? '▶' : '⏸';
  const lines = [
    `kkscreeps renderer · dev harness (R5)`,
    `backend : ${s.backend}   fps: ${s.fps}   zoom: ${(s.zoom * 100).toFixed(0)}%   atlas: ${s.atlasFrames}f   lighting: ${s.lighting}`,
    `replay  : [${current}] ${index}/${f.ticks.length} ${state} ${SPEEDS[speedIdx]}×   player: ${params.get('player') ?? 'p1'}`,
    `room    : ${f.room} time=${summary?.time ?? '-'} objects=${summary?.objects ?? '-'} visuals=${summary?.visuals ?? 0}`,
    `effects : active=${s.effects.active} nodes=${s.effects.nodes} texts=${s.effects.privateTexts} skipped=${s.effects.skipped} · ambient=${s.ambient.awake}/${s.ambient.active} sleeping=${s.ambient.sleeping} sample=${sampleElapsed}ms`,
    `controls: 1-9 / 0 压力 · 空格 播放/暂停（动画同步） · . 下一 tick · → 动画 +50ms · +/- 调速 · F 视角复位`,
  ];
  if (replayError) lines.push(`ERROR   : ${replayError}`);
  if (params.has('smoke')) {
    const summary = (values: number[]) => {
      const sorted = [...values].sort((a, b) => a - b);
      return `n=${sorted.length} p50=${(sorted[Math.floor(sorted.length * 0.5)] ?? 0).toFixed(3)}ms p95=${(sorted[Math.floor(sorted.length * 0.95)] ?? 0).toFixed(3)}ms`;
    };
    lines.push(`CPU objects : ${summary(samples.objects)}`, `CPU lighting: ${summary(samples.lighting)}`,
      `lighting RT : renders=${s.lightingPass.renders} dirty=${s.lightingPass.invalidations} members=${s.lightingPass.members} privateSources=${s.lightingPass.privateSources}`);
  }
  hud.innerHTML = lines.map((l) => `<div>${l}</div>`).join('');
}
setInterval(renderHud, 250);
renderHud();
