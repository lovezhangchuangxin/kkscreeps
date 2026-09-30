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
): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = renderSize;
  canvas.height = renderSize;
  const ctx = canvas.getContext('2d')!;
  ctx.scale(renderSize / view, renderSize / view);
  const path2d = new Path2D(path);
  paint(ctx, path2d);
  return canvas;
}

/** 烘焙产物转纹理（私有 canvas 纹理——销毁时可带 texture/textureSource）。 */
export function textureFromBaked(canvas: HTMLCanvasElement): Texture {
  return Texture.from(canvas);
}
