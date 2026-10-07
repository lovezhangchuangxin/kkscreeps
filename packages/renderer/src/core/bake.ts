/**
 * canvas 烘焙公共件（terrain.ts 与 objects-stage.ts 共用；R3 从 terrain.ts
 * 抽出——rampart 顶罩同走 path→Path2D→canvas 管线）。
 */
import { Texture } from 'pixi.js';

export function bakeCanvas(
  path: string,
  view: number,
  renderSize: number,
  paint: (ctx: CanvasRenderingContext2D, path2d: Path2D) => void,
  region = { x: 0, y: 0, width: renderSize, height: renderSize },
  reuse?: HTMLCanvasElement,
): HTMLCanvasElement {
  const canvas = reuse ?? document.createElement('canvas');
  // Assigning dimensions resets pixels and all 2D state, also for same-size reuse.
  canvas.width = region.width;
  canvas.height = region.height;
  const ctx = canvas.getContext('2d')!;
  // Integer raster offset preserves the original full-room sampling phase.
  ctx.setTransform(renderSize / view, 0, 0, renderSize / view, -region.x, -region.y);
  const path2d = new Path2D(path);
  paint(ctx, path2d);
  return canvas;
}

/** 烘焙产物转纹理（私有 canvas 纹理——销毁时可带 texture/textureSource）。 */
export function textureFromBaked(canvas: HTMLCanvasElement): Texture {
  return Texture.from(canvas, true);
}
