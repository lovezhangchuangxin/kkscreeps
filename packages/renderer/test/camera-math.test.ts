import { describe, expect, it } from 'vitest';
import {
  clampZoom,
  fitRect,
  panBy,
  screenToWorld,
  worldToScreen,
  zoomAtScreenPoint,
  MAX_ZOOM,
  MIN_ZOOM,
  type CameraState,
} from '../src/core/camera-math.js';

const CAM: CameraState = { x: 2500, y: 2500, zoom: 0.5 };

describe('clampZoom', () => {
  it('夹在 [MIN_ZOOM, MAX_ZOOM]', () => {
    expect(clampZoom(-1)).toBe(MIN_ZOOM);
    expect(clampZoom(0.5)).toBe(0.5);
    expect(clampZoom(100)).toBe(MAX_ZOOM);
  });

  it('支持自定义界限', () => {
    expect(clampZoom(0.5, 0.6, 2)).toBe(0.6);
  });
});

describe('screenToWorld / worldToScreen', () => {
  it('互为逆变换', () => {
    const world = { x: 1234, y: 4321 };
    const screen = worldToScreen(CAM, 1280, 720, world.x, world.y);
    const back = screenToWorld(CAM, 1280, 720, screen.x, screen.y);
    expect(back.x).toBeCloseTo(world.x);
    expect(back.y).toBeCloseTo(world.y);
  });

  it('相机中心落在视口中心', () => {
    const s = worldToScreen(CAM, 1280, 720, CAM.x, CAM.y);
    expect(s).toEqual({ x: 640, y: 360 });
  });
});

describe('panBy', () => {
  it('拖拽方向为内容跟随（右拖 → 相机左移）', () => {
    const next = panBy(CAM, 100, 0);
    expect(next.x).toBeCloseTo(CAM.x - 100 / CAM.zoom);
    expect(next.y).toBeCloseTo(CAM.y);
  });

  it('屏幕位移的世界换算随 zoom 反比', () => {
    expect(panBy(CAM, 100, 0).x).toBeCloseTo(CAM.x - 200);
    expect(panBy({ ...CAM, zoom: 2 }, 100, 0).x).toBeCloseTo(CAM.x - 50);
  });
});

describe('zoomAtScreenPoint', () => {
  it('光标下的世界点缩放前后不动（缩放至光标）', () => {
    const sx = 400;
    const sy = 300;
    const anchor = screenToWorld(CAM, 1280, 720, sx, sy);
    const next = zoomAtScreenPoint(CAM, 1280, 720, sx, sy, 1.6);
    const anchorAfter = screenToWorld(next, 1280, 720, sx, sy);
    expect(anchorAfter.x).toBeCloseTo(anchor.x);
    expect(anchorAfter.y).toBeCloseTo(anchor.y);
  });

  it('缩放倍率被 clamp', () => {
    expect(zoomAtScreenPoint(CAM, 1280, 720, 640, 360, 1e9).zoom).toBe(MAX_ZOOM);
    expect(zoomAtScreenPoint(CAM, 1280, 720, 640, 360, 1e-9).zoom).toBe(MIN_ZOOM);
  });

  it('视口中心缩放不改变相机焦点', () => {
    const next = zoomAtScreenPoint(CAM, 1280, 720, 640, 360, 2);
    expect(next.x).toBeCloseTo(CAM.x);
    expect(next.y).toBeCloseTo(CAM.y);
    expect(next.zoom).toBeCloseTo(CAM.zoom * 2);
  });
});

describe('fitRect', () => {
  it('矩形完整可见且居中（房 5000² 装进 1280×720）', () => {
    const next = fitRect(0, 0, 5000, 5000, 1280, 720, 32);
    const tl = worldToScreen(next, 1280, 720, 0, 0);
    const br = worldToScreen(next, 1280, 720, 5000, 5000);
    expect(tl.x).toBeGreaterThanOrEqual(32 - 0.001);
    expect(tl.y).toBeGreaterThanOrEqual(32 - 0.001);
    expect(br.x).toBeLessThanOrEqual(1280 - 32 + 0.001);
    expect(br.y).toBeLessThanOrEqual(720 - 32 + 0.001);
    expect(next.x).toBeCloseTo(2500);
    expect(next.y).toBeCloseTo(2500);
    expect(next.zoom).toBeCloseTo((720 - 64) / 5000);
  });
});
