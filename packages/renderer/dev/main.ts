/**
 * dev harness（R1 建，R3 扩）：回放夹具驱动对象渲染（回放控制 1-5 切夹具/
 * 空格/./+/-/F）。?player=<id> 切本玩家视角（缺省 p1；?player=x 全敌对色），
 * ?backend=webgl 强制回退路径。HUD 属 harness 交互辅助，不属渲染器（§2.2）。
 */
import { isDelta, isSnapshot, type FixtureFile } from '@kkscreeps/shared';
import { createRenderer } from '../src/index.js';
import denseEdge from '../fixtures/dense-edge.json';
import plains from '../fixtures/plains.json';
import showcase from '../fixtures/showcase.json';
import standard from '../fixtures/standard.json';
import swampWalls from '../fixtures/swamp-walls.json';

const stageHost = document.getElementById('stage') as HTMLElement;
const hud = document.getElementById('hud') as HTMLElement;

// 文字件（spawn 名/mineral 符号）需 Roboto 先于首帧就绪（缺省回退字体即
// G7 基线漂移）；加载失败不阻塞（allSettled——回退观感属 G7 盯梢面）。
await Promise.allSettled([
  document.fonts.load('55px Roboto'),
  document.fonts.load('bold 82px Roboto'),
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
  showcase,
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

function fixture(): FixtureFile {
  return fixtures.get(current)!;
}

function applyEntry(): void {
  const f = fixture();
  if (index >= f.ticks.length) {
    playing = false;
    stopTimer(); // 末态清定时器（评审 C 建议 1：不留空转僵尸 interval）
    return;
  }
  const entry = f.ticks[index]!;
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
  try {
    applyEntry();
  } catch (e) {
    replayError = e instanceof Error ? e.message : String(e);
  }
  renderHud();
}

function cycleSpeed(): void {
  speedIdx = (speedIdx + 1) % SPEEDS.length;
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
    if (playing) setPlaying(true);
    else renderHud();
  } else {
    const keys = [...fixtures.keys()];
    const i = Number(e.key) - 1;
    if (i >= 0 && i < keys.length) loadFixture(keys[i]!);
  }
});
// pixi ResizePlugin 的 resize 是 rAF 延迟生效——同步 fitRoom 会读到旧视口尺寸，
// 排到同帧之后执行（rAF 回调按注册序，ResizePlugin 先注册）。
window.addEventListener('resize', () => requestAnimationFrame(() => renderer.fitRoom()));
// ?view=<x>,<y>,<zoom>（世界坐标/缩放）：截图钉住相机——注册在 fitRoom 监听
// 之后，同帧 rAF 后执行覆盖 fit（harness 专用，不属渲染器公共面）。
const viewParam = params.get('view');
if (viewParam) {
  const [vx, vy, vz] = viewParam.split(',').map(Number);
  const applyPinned = () => renderer.setCameraState({ x: vx!, y: vy!, zoom: vz! });
  requestAnimationFrame(() => requestAnimationFrame(applyPinned));
  window.addEventListener('resize', () => requestAnimationFrame(applyPinned));
}
renderer.fitRoom();
loadFixture(current);

// ---- HUD ----
function renderHud(): void {
  const s = renderer.getStats();
  const f = fixture();
  const summary = renderer.feed.summary(f.room);
  const state = index >= f.ticks.length ? 'end' : playing ? '▶' : '⏸';
  const lines = [
    `kkscreeps renderer · dev harness (R3)`,
    `backend : ${s.backend}   fps: ${s.fps}   zoom: ${(s.zoom * 100).toFixed(0)}%   atlas: ${s.atlasFrames}f   lighting: ${s.lighting}`,
    `replay  : [${current}] ${index}/${f.ticks.length} ${state} ${SPEEDS[speedIdx]}×   player: ${params.get('player') ?? 'p1'}`,
    `room    : ${f.room} time=${summary?.time ?? '-'} objects=${summary?.objects ?? '-'} visuals=${summary?.visuals ?? 0}`,
    `controls: 1-5 夹具（重按=重放） · 空格 播放/暂停 · . 单步 · +/- 调速 · F 视角复位`,
  ];
  if (replayError) lines.push(`ERROR   : ${replayError}`);
  hud.innerHTML = lines.map((l) => `<div>${l}</div>`).join('');
}
setInterval(renderHud, 250);
renderHud();
