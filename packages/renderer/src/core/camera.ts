/**
 * 相机控制器：DOM 事件（滚轮/拖拽）→ camera-math → Pixi stage 变换。
 * 事件挂在 canvas DOM 层而非 Pixi 命中系统——相机与对象拾取（R2）互不干扰。
 */
import type { Container } from 'pixi.js';
import {
  type CameraState,
  MAX_ZOOM,
  MIN_ZOOM,
  panBy,
  zoomAtScreenPoint,
} from './camera-math';

export interface CameraOptions {
  minZoom?: number;
  maxZoom?: number;
  initial?: CameraState;
}

export class CameraController {
  private cam: CameraState;
  private readonly minZoom: number;
  private readonly maxZoom: number;
  private readonly stage: Container;
  private readonly getViewSize: () => { width: number; height: number };
  private canvas: HTMLCanvasElement | null = null;
  private dragging = false;
  private lastSx = 0;
  private lastSy = 0;

  private readonly onWheel = (e: WheelEvent) => {
    e.preventDefault();
    // deltaMode 1=line（Firefox 惯例，1 行≈33px），2=page（已绝迹，按 line 量级兜住）
    const delta = e.deltaMode === 1 ? e.deltaY * 33 : e.deltaY;
    const { width, height } = this.getViewSize();
    const rect = this.canvas!.getBoundingClientRect();
    // e.clientX 相对视口，光标锚点需换算到 canvas 局部坐标。
    const sx = e.clientX - rect.left;
    const sy = e.clientY - rect.top;
    const factor = Math.exp(-delta * 0.0015);
    this.cam = zoomAtScreenPoint(this.cam, width, height, sx, sy, factor, this.minZoom, this.maxZoom);
    this.apply();
  };
  private readonly onPointerDown = (e: PointerEvent) => {
    this.dragging = true;
    this.lastSx = e.clientX;
    this.lastSy = e.clientY;
    this.canvas!.setPointerCapture(e.pointerId);
    this.canvas!.style.cursor = 'grabbing';
  };
  private readonly onPointerMove = (e: PointerEvent) => {
    if (!this.dragging) return;
    this.cam = panBy(this.cam, e.clientX - this.lastSx, e.clientY - this.lastSy);
    this.lastSx = e.clientX;
    this.lastSy = e.clientY;
    this.apply();
  };
  private readonly endDrag = (e: PointerEvent) => {
    if (!this.dragging) return;
    this.dragging = false;
    // pointercancel 路径浏览器可能已隐式释放捕获，先查再放
    if (this.canvas!.hasPointerCapture(e.pointerId)) this.canvas!.releasePointerCapture(e.pointerId);
    this.canvas!.style.cursor = 'grab';
  };
  private readonly onContextMenu = (e: MouseEvent) => e.preventDefault();

  constructor(stage: Container, getViewSize: () => { width: number; height: number }, options: CameraOptions = {}) {
    this.stage = stage;
    this.getViewSize = getViewSize;
    this.minZoom = options.minZoom ?? MIN_ZOOM;
    this.maxZoom = options.maxZoom ?? MAX_ZOOM;
    this.cam = options.initial ?? { x: 0, y: 0, zoom: 1 };
  }

  /** 接管 canvas 的相机输入。 */
  attach(canvas: HTMLCanvasElement): void {
    this.canvas = canvas;
    canvas.addEventListener('wheel', this.onWheel, { passive: false });
    canvas.addEventListener('pointerdown', this.onPointerDown);
    canvas.addEventListener('pointermove', this.onPointerMove);
    canvas.addEventListener('pointerup', this.endDrag);
    canvas.addEventListener('pointercancel', this.endDrag);
    canvas.addEventListener('contextmenu', this.onContextMenu);
    canvas.style.cursor = 'grab';
    this.apply();
  }

  detach(): void {
    if (!this.canvas) return;
    const canvas = this.canvas;
    canvas.removeEventListener('wheel', this.onWheel);
    canvas.removeEventListener('pointerdown', this.onPointerDown);
    canvas.removeEventListener('pointermove', this.onPointerMove);
    canvas.removeEventListener('pointerup', this.endDrag);
    canvas.removeEventListener('pointercancel', this.endDrag);
    canvas.removeEventListener('contextmenu', this.onContextMenu);
    canvas.style.cursor = '';
    this.canvas = null;
  }

  get state(): Readonly<CameraState> {
    return this.cam;
  }

  get zoom(): number {
    return this.cam.zoom;
  }

  /** 相机状态直接置位（如 harness 的 fit 按钮走 fitRect 后调这里）。 */
  setState(next: CameraState): void {
    this.cam = { ...next };
    this.apply();
  }

  /** cam → stage 变换：world (x,y) 落在视口中心。 */
  apply(): void {
    const { width, height } = this.getViewSize();
    this.stage.scale.set(this.cam.zoom);
    this.stage.position.set(width / 2 - this.cam.x * this.cam.zoom, height / 2 - this.cam.y * this.cam.zoom);
  }
}
