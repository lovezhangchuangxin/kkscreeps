/**
 * dev harness（R0 骨架）：底色房 + 相机 + HUD。
 * R1 起加载回放夹具驱动（播放/暂停/单步/调速/切房，方案 §7）。
 * HUD 属 harness 交互辅助，不属渲染器（方案 §2.2）。
 */
import { createRenderer } from '../src/index.js';

const stageHost = document.getElementById('stage') as HTMLElement;
const hud = document.getElementById('hud') as HTMLElement;

const renderer = await createRenderer({
  container: stageHost,
  atlasBaseUrl: new URL('/', window.location.href).href,
  // ?backend=webgl 强制回退路径（§5：WebGL 同场景跑通记数不设门）
  preference: new URLSearchParams(window.location.search).get('backend') === 'webgl' ? 'webgl' : 'webgpu',
});

window.addEventListener('keydown', (e) => {
  if (e.key === 'f' || e.key === 'F') renderer.fitRoom();
});
// pixi ResizePlugin 的 resize 是 rAF 延迟生效——同步 fitRoom 会读到旧视口尺寸，
// 排到同帧之后执行（rAF 回调按注册序，ResizePlugin 先注册）。
window.addEventListener('resize', () => requestAnimationFrame(() => renderer.fitRoom()));
renderer.fitRoom();

function renderHud(): void {
  const s = renderer.getStats();
  const lines = [
    `kkscreeps renderer · dev harness (R0)`,
    `backend : ${s.backend}`,
    `fps     : ${s.fps}`,
    `zoom    : ${(s.zoom * 100).toFixed(0)}%`,
    `atlas   : ${s.atlasFrames} frames`,
    `controls: 拖拽平移 · 滚轮缩放 · F 复位`,
  ];
  hud.innerHTML = lines.map((l) => `<div>${l}</div>`).join('');
}
setInterval(renderHud, 250);
renderHud();
