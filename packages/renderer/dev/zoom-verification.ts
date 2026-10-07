/** Short GPU regression, opt in via perf.html?fixture=official-room&zoom=1.
 * Replays the former automatic sprite-mask path only at safe low zooms. */
import { MaskFilter, RenderTexture, TexturePool, TilingSprite } from 'pixi.js';
import type { RoomSnapshot } from '@kkscreeps/shared';
import type { RendererHandle } from '../src/index.js';
import type { PerfProbe } from '../src/perf/probe.js';

export async function verifyZoom(handle: RendererHandle, probe: PerfProbe, snapshot: RoomSnapshot) {
  const started = performance.now();
  const renderer = probe.kernel.app.renderer;
  const original = { width: renderer.screen.width, height: renderer.screen.height, resolution: renderer.resolution };
  const allocations: Array<{ width: number; height: number }> = [];
  const getTexture = TexturePool.getOptimalTexture;
  TexturePool.getOptimalTexture = ((...args: unknown[]) => {
    const texture = Reflect.apply(getTexture, TexturePool, args) as ReturnType<typeof getTexture>;
    allocations.push({ width: texture.source.pixelWidth, height: texture.source.pixelHeight });
    return texture;
  }) as typeof getTexture;
  const runtime = renderer as unknown as { gpu?: { device: GPUDevice }; gl?: WebGL2RenderingContext };
  const textureLimit = runtime.gpu?.device.limits.maxTextureDimension2D
    ?? runtime.gl!.getParameter(runtime.gl!.MAX_TEXTURE_SIZE) as number;
  let deviceLost = false;
  if (runtime.gpu) void runtime.gpu.device.lost.then(() => { deviceLost = true; });
  const contextLost = () => { deviceLost = true; };
  probe.kernel.app.canvas.addEventListener('webglcontextlost', contextLost);
  const masks = () => probe.kernel.stage.getChildByLabel('layer-terrain', true)!.children
    .filter((node): node is TilingSprite => node instanceof TilingSprite && node.label.includes('swamp-noise'))
    .map(noise => ({ noise, filter: noise.filters![0] as MaskFilter }));
  async function capture(width: number, height: number, resolution: number) {
    allocations.length = 0;
    const target = RenderTexture.create({ width, height, resolution, antialias: true });
    try {
      renderer.render({ container: probe.kernel.stage, target, clear: true });
      await probe.gpuDone();
      const result = renderer.extract.pixels(target);
      return { ...result, allocations: [...allocations] };
    } finally { target.destroy(true); }
  }
  const parity = [], zoom = [], lifecycle = [];
  try {
    for (const resolution of [1, 2]) {
      renderer.resize(320, 240, resolution);
      for (const scale of [0.13, 0.2]) {
        handle.setCameraState({ x: 1950.25, y: 2250.75, zoom: scale });
        const pairs = masks();
        if (pairs.length !== 2) throw new Error(`Expected two swamp masks, found ${pairs.length}`);
        for (const { noise, filter } of pairs) { noise.filters = null; noise.mask = filter.sprite; }
        let before;
        try { before = await capture(320, 240, resolution); }
        finally {
          for (const { noise, filter } of pairs) {
            noise.mask = null; filter.sprite.renderable = false; filter.sprite.measurable = false;
            noise.filters = [filter];
          }
        }
        const after = await capture(320, 240, resolution);
        if (before.width !== after.width || before.height !== after.height || before.pixels.length !== after.pixels.length) {
          throw new Error('Pixel comparison dimensions differ');
        }
        let differingChannels = 0, maxDifference = 0;
        for (let i = 0; i < before.pixels.length; i++) {
          const delta = Math.abs(before.pixels[i]! - after.pixels[i]!);
          if (delta) differingChannels++;
          maxDifference = Math.max(maxDifference, delta);
        }
        const visible = pairs.map(({ noise }) => noise.visible);
        for (const { noise } of pairs) noise.visible = false;
        let control;
        try { control = await capture(320, 240, resolution); }
        finally { pairs.forEach(({ noise }, i) => { noise.visible = visible[i]!; }); }
        let noiseControlDifferences = 0;
        for (let i = 0; i < after.pixels.length; i++) if (after.pixels[i] !== control.pixels[i]) noiseControlDifferences++;
        if (noiseControlDifferences === 0) throw new Error('Swamp noise did not contribute to the image');
        parity.push({ resolution, scale, width: after.width, height: after.height, comparedChannels: after.pixels.length,
          differingChannels, maxDifference, noiseControlDifferences, before: before.allocations, after: after.allocations });
      }
      for (const [width, height] of [[320, 240], [451, 287], [1920, 1080]]) {
        renderer.resize(width!, height!, resolution);
        for (const scale of [1.3, 2, 4]) for (const [x, y] of [[1950.25, 2250.75], [0, 0], [4950, 0], [0, 4950], [4950, 4950]]) {
          if (width === 1920 && scale !== 4) continue;
          handle.setCameraState({ x: x!, y: y!, zoom: scale });
          const image = await capture(width!, height!, resolution);
          let nonBlackPixels = 0;
          for (let i = 0; i < image.pixels.length; i += 4) if (image.pixels[i] || image.pixels[i + 1] || image.pixels[i + 2]) nonBlackPixels++;
          const maxWidth = Math.max(0, ...image.allocations.map(t => t.width));
          const maxHeight = Math.max(0, ...image.allocations.map(t => t.height));
          const viewportBounded = maxWidth <= 2 ** Math.ceil(Math.log2(width! * resolution))
            && maxHeight <= 2 ** Math.ceil(Math.log2(height! * resolution));
          const stage = probe.kernel.stage;
          const actualCamera = { x: (width! / 2 - stage.x) / stage.scale.x, y: (height! / 2 - stage.y) / stage.scale.y, zoom: stage.scale.x };
          zoom.push({ width, height, resolution, scale, requestedCamera: { x, y }, actualCamera, nonBlackPixels,
            maxWidth, maxHeight, viewportBounded, withinDeviceLimit: maxWidth <= textureLimit && maxHeight <= textureLimit });
        }
      }
    }
    for (let i = 0; i < 3; i++) {
      const old = masks().map(pair => pair.filter);
      handle.feed.applySnapshot({ ...snapshot, objects: [], terrain: { codes: Array<number>(2500).fill(0) } });
      handle.advanceAnimation(0.001); probe.render(); await probe.gpuDone();
      const removed = old.every(filter => filter.resources === null);
      handle.feed.applySnapshot(snapshot); handle.advanceAnimation(50); probe.render(); await probe.gpuDone();
      lifecycle.push({ cycle: i, oldFiltersReleased: removed, rebuiltMasks: masks().length, lighting: handle.getStats().lighting });
    }
    return { backend: probe.kernel.backend, textureLimit, deviceLost, elapsedMs: performance.now() - started,
      environment: { userAgent: navigator.userAgent, physicalDpr: devicePixelRatio, original }, parity, zoom, lifecycle,
      note: 'CPU/GPU timing and memory soak are not measured by this regression.' };
  } finally {
    TexturePool.getOptimalTexture = getTexture;
    probe.kernel.app.canvas.removeEventListener('webglcontextlost', contextLost);
    renderer.resize(original.width, original.height, original.resolution);
    handle.fitRoom(); probe.render();
  }
}
