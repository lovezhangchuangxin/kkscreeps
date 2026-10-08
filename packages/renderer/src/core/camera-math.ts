/**
 * 相机数学（纯函数，无 Pixi 依赖——Vitest 单测面）。
 *
 * 约定：相机状态 {x,y,zoom} 表示「世界点 (x,y) 位于视口中心」；
 * world→screen：sx = (wx - cam.x) * zoom + vw/2。
 * R0 提供缩放（至光标）/平移/fit；钳边与拾取的完整视口在 R2 收口（方案 §7）。
 */

export interface CameraState {
  /** 视口中心对准的世界坐标。 */
  x: number;
  y: number;
  /** 屏幕像素 / 世界像素。 */
  zoom: number;
}

export const MIN_ZOOM = 0.05;
export const MAX_ZOOM = 4;

export function clampZoom(zoom: number, min = MIN_ZOOM, max = MAX_ZOOM): number {
  return Math.min(max, Math.max(min, zoom));
}

export function screenToWorld(
  cam: CameraState,
  viewWidth: number,
  viewHeight: number,
  sx: number,
  sy: number,
): { x: number; y: number } {
  return {
    x: (sx - viewWidth / 2) / cam.zoom + cam.x,
    y: (sy - viewHeight / 2) / cam.zoom + cam.y,
  };
}

export function worldToScreen(
  cam: CameraState,
  viewWidth: number,
  viewHeight: number,
  wx: number,
  wy: number,
): { x: number; y: number } {
  return {
    x: (wx - cam.x) * cam.zoom + viewWidth / 2,
    y: (wy - cam.y) * cam.zoom + viewHeight / 2,
  };
}

/** 平移：屏幕像素位移换算为世界位移（zoom 无关手感：拖 1px = 世界 1/zoom）。 */
export function panBy(cam: CameraState, dxScreen: number, dyScreen: number): CameraState {
  return { ...cam, x: cam.x - dxScreen / cam.zoom, y: cam.y - dyScreen / cam.zoom };
}

/**
 * 以屏幕点为锚缩放：缩放前后光标下的世界点不动。
 * factor<1 缩小（滚轮向下），>1 放大。
 */
export function zoomAtScreenPoint(
  cam: CameraState,
  viewWidth: number,
  viewHeight: number,
  sx: number,
  sy: number,
  factor: number,
  min = MIN_ZOOM,
  max = MAX_ZOOM,
): CameraState {
  const before = screenToWorld(cam, viewWidth, viewHeight, sx, sy);
  const zoom = clampZoom(cam.zoom * factor, min, max);
  const after = screenToWorld({ ...cam, zoom }, viewWidth, viewHeight, sx, sy);
  return { zoom, x: cam.x + (before.x - after.x), y: cam.y + (before.y - after.y) };
}

/** 将世界矩形适配进视口（留 paddingPx 边距），返回新相机状态。 */
export function fitRect(
  x: number,
  y: number,
  width: number,
  height: number,
  viewWidth: number,
  viewHeight: number,
  paddingPx = 32,
  min = MIN_ZOOM,
  max = MAX_ZOOM,
): CameraState {
  const zoom = clampZoom(
    Math.min((viewWidth - paddingPx * 2) / width, (viewHeight - paddingPx * 2) / height),
    min,
    max,
  );
  return { x: x + width / 2, y: y + height / 2, zoom };
}

/**
 * 钳边：视野矩形不得越出世界矩形（R2 完整视口，方案 §7）。
 * 视口宽/高超过矩形时轴心锁定矩形中心（缩小态居中，不跟随平移）。
 */
export function clampToRect(
  cam: CameraState,
  viewWidth: number,
  viewHeight: number,
  minX: number,
  minY: number,
  maxX: number,
  maxY: number,
): CameraState {
  const halfW = viewWidth / 2 / cam.zoom;
  const halfH = viewHeight / 2 / cam.zoom;
  const cx =
    halfW * 2 >= maxX - minX
      ? (minX + maxX) / 2
      : Math.min(maxX - halfW, Math.max(minX + halfW, cam.x));
  const cy =
    halfH * 2 >= maxY - minY
      ? (minY + maxY) / 2
      : Math.min(maxY - halfH, Math.max(minY + halfH, cam.y));
  return { ...cam, x: cx, y: cy };
}
