/**
 * dev harness（R1 建，R5 扩，R6 迁冻结公共面）：回放夹具驱动对象渲染
 * （1-9/0/v 切夹具 · 空格 播放暂停 · . 单步 · → 动画+50ms · +/- 调速 ·
 * F 复位 · Tab 轮切活动房）。?player=<id> 切本玩家视角（缺省 p1），
 * ?backend=webgl 强制回退路径，?multi=1 多房常驻+双回放（Tab 切房），
 * ?fixture=visual-cases 进 visual 用例集。HUD 属 harness 交互辅助，
 * 不属渲染器（§2.2）。全部经冻结公共面：字体 fontsBaseUrl（G7 库责任）、
 * 定帧 advanceAnimation（原 __room 直驱迁出）、计时 onFrameTiming（smoke）。
 */
import { isDelta, isSnapshot, type FixtureFile, type RoomSnapshot, type RoomVisual } from '@kkscreeps/shared';
import { createRenderer } from '../src/index.js';
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
import visualCases from '../fixtures/visual-cases.json';

const stageHost = document.getElementById('stage') as HTMLElement;
const hud = document.getElementById('hud') as HTMLElement;

const params = new URLSearchParams(window.location.search);
const multi = params.has('multi');
// ?smoke=1 仅计实际场景更新与 lighting 提交的 CPU 时间（onFrameTiming，
// R5 方法学同源）；不冒充 GPU 耗时。
const samples = { objects: [] as number[], lighting: [] as number[] };
let playing = false; // 声明提前：onFrameTiming 闭包引用

const renderer = await createRenderer({
  container: stageHost,
  atlasBaseUrl: new URL('/', window.location.href).href,
  // 字体装载责任在库（G7/D5）：三规格 FontFace 预载，失败回退可验证
  fontsBaseUrl: new URL('/', window.location.href).href,
  // ?backend=webgl 强制回退路径（§5：WebGL 同场景跑通记数不设门）
  preference: params.get('backend') === 'webgl' ? 'webgl' : 'webgpu',
  // 本玩家视角（G9：userOwnerColor 色板 own 判定）
  localPlayerId: params.get('player') ?? 'p1',
  ...(params.has('smoke')
    ? {
        onFrameTiming: (t: { objectsMs: number; lightingMs: number }) => {
          if (playing && samples.objects.length < 10000) {
            samples.objects.push(t.objectsMs);
            samples.lighting.push(t.lightingMs);
          }
        },
      }
    : {}),
});
// dev 调试句柄（公共冻结面本身；无 __room 附加字段——R6 已移除）
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

// visual 用例集（合成，非 FixtureFile）：terrain 复用 plains 快照，空房+setVisuals
const VISUALS_KEY = 'visual-cases';
const visualSnapshot = (): RoomSnapshot => ({
  time: 0,
  room: visualCases.room,
  terrain: (plains.ticks[0] as RoomSnapshot).terrain,
  objects: [],
});
const visualApply = (): void => {
  renderer.feed.applySnapshot(visualSnapshot());
  renderer.feed.setVisuals(visualCases.room, visualCases.visuals as RoomVisual[]);
};

// ---- 回放状态机（G6：tickDuration 缺省 1000ms，契约参数化）----
const TICK_MS = 1000;
const SPEEDS = [0.5, 1, 2, 4] as const;
let current = params.get('fixture') ?? 'standard';
let index = 0;
let speedIdx = 1; // 1×
let timer: number | undefined;
let replayError: string | null = null;
let sampleElapsed = 0;
// 拾取/悬停演示状态（renderHud 经 loadFixture→setPlaying 提前触达，须先于首帧初始化）
let hoverText = '-';
let pickText = '-';

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
  playing = next && (current === VISUALS_KEY || index < fixture().ticks.length);
  stopTimer();
  renderer.setPlaybackRate(playing ? SPEEDS[speedIdx]! : 0);
  if (playing) timer = window.setInterval(stepGuarded, TICK_MS / SPEEDS[speedIdx]!);
  syncSecondary();
  renderHud();
}

function loadFixture(key: string): void {
  const isVisuals = key === VISUALS_KEY;
  if (!isVisuals && !fixtures.has(key)) return;
  stopTimer();
  current = key;
  index = 0;
  replayError = null;
  if (isVisuals) {
    visualApply();
    renderer.setActiveRoom(visualCases.room);
  } else {
    applyEntry(); // 首快照入库
    renderer.setActiveRoom(fixture().room); // 显式切活动房（快照可能只是常驻更新）
  }
  setPlaying(playing);
}

function stepOnce(): void {
  if (current === VISUALS_KEY) return;
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

// ---- ?multi=1：多房常驻 + 副回放（隔离演示：非活动房照常收数零渲染） ----
const SECONDARY_KEY = 'creeps';
let secondaryIndex = 0;
let secondaryTimer: number | undefined;
function stepSecondary(): void {
  const f = fixtures.get(SECONDARY_KEY)!;
  if (secondaryIndex >= f.ticks.length) {
    if (secondaryTimer !== undefined) clearInterval(secondaryTimer);
    secondaryTimer = undefined;
    return;
  }
  const entry = f.ticks[secondaryIndex++]!;
  if (isSnapshot(entry)) renderer.feed.applySnapshot(entry);
  else renderer.feed.applyDelta(entry);
  renderHud();
}
function syncSecondary(): void {
  if (!multi) return;
  const want = playing && current !== SECONDARY_KEY;
  if (want && secondaryTimer === undefined) {
    secondaryTimer = window.setInterval(stepSecondary, TICK_MS);
    secondaryIndex = 0;
  } else if (!want && secondaryTimer !== undefined) {
    clearInterval(secondaryTimer);
    secondaryTimer = undefined;
  }
}
if (multi) {
  for (const [key, f] of fixtures) {
    if (key === current) continue;
    renderer.feed.applySnapshot(f.ticks[0]); // 全夹具常驻（数据面）
  }
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
  } else if (e.key === 'Tab') {
    e.preventDefault();
    const rooms = renderer.feed.roomNames();
    if (rooms.length === 0) return;
    const at = rooms.indexOf(renderer.activeRoom ?? '');
    renderer.setActiveRoom(rooms[(at + 1) % rooms.length]!);
    renderHud();
  } else if (e.key === 'v' || e.key === 'V') {
    loadFixture(VISUALS_KEY);
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
loadFixture(current);
const initialTick = Number(params.get('tick') ?? 0);
while (current !== VISUALS_KEY && index <= initialTick && index < fixture().ticks.length) {
  // Seeking consumes completed intervals, so tick N does not retain all of
  // tick 1..N-1's beams. The selected tick starts with finite-effect age zero.
  if (index > 1) renderer.advanceAnimation(TICK_MS);
  applyEntry();
}
if (params.has('sample')) sampleFrame(Number(params.get('sample')));

/** 定帧动画推进（?sample= / →键）：公共面 advanceAnimation（原 __room 直驱迁出）。 */
function sampleFrame(ms: number): void {
  if (!Number.isFinite(ms) || ms < 0) return;
  renderer.advanceAnimation(ms);
  sampleElapsed += ms;
  renderHud();
}

// ---- 拾取/悬停演示（R6 D4）----
renderer.onHover((r) => {
  hoverText = r === null || r.cell === null ? '-' : `(${r.cell.x},${r.cell.y}) ${r.objectId ?? ''}`;
  renderHud();
});
renderer.onPick((r) => {
  pickText = r.cell === null ? '-' : `(${r.cell.x},${r.cell.y}) ${r.objectId ?? '-'}`;
  renderHud();
});

// ---- HUD ----
function renderHud(): void {
  const s = renderer.getStats();
  const isVisuals = current === VISUALS_KEY;
  const activeRoom = renderer.activeRoom;
  const summary = activeRoom !== null ? renderer.feed.summary(activeRoom) : undefined;
  const total = isVisuals ? visualCases.visuals.length : fixture().ticks.length;
  const state = isVisuals ? 'static' : index >= total ? 'end' : playing ? '▶' : '⏸';
  const canvas = stageHost.querySelector('canvas');
  const lines = [
    `kkscreeps renderer · dev harness (R6)${multi ? ' · multi' : ''}`,
    `backend : ${s.backend}   fps: ${s.fps}   zoom: ${(s.zoom * 100).toFixed(0)}%   atlas: ${s.atlasFrames}f   lighting: ${s.lighting}${canvas ? `   canvas: ${canvas.width}×${canvas.height}` : ''}`,
    `fonts   : ${s.fonts}   visuals: ${s.visuals}   effects: active=${s.effects.active} nodes=${s.effects.nodes} texts=${s.effects.texts} skipped=${s.effects.skipped}`,
    `ambient : awake=${s.ambient.awake}/${s.ambient.active} sleeping=${s.ambient.sleeping}   sample=${sampleElapsed}ms`,
    `replay  : [${current}] ${index}/${total} ${state} ${SPEEDS[speedIdx]}×   player: ${params.get('player') ?? 'p1'}`,
    `active  : ${activeRoom ?? '-'} time=${summary?.time ?? '-'} objects=${summary?.objects ?? '-'} rooms=${renderer.feed.roomNames().length}${multi ? ` · secondary[${SECONDARY_KEY}]=${secondaryIndex}` : ''}`,
    `cursor  : hover=${hoverText}   pick=${pickText}`,
    `controls: 1-9/0/v 夹具 · 空格 播放 · . 下一 tick · → 动画+50ms · +/- 调速 · F 复位 · Tab 切活动房`,
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
