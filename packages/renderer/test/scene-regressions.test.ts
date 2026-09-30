import { afterEach, describe, expect, it, vi } from 'vitest';
import { Container, Matrix, Texture, TextureSource } from 'pixi.js';
import { LightingPass } from '../src/core/lighting.js';
import { CameraController } from '../src/core/camera.js';
import { screenToWorld } from '../src/core/camera-math.js';
import { TerrainStage, type TerrainTextures } from '../src/scenes/terrain.js';
import { joinLink } from '../src/objects/joins-structures.js';
import type { PartEnv } from '../src/objects/part-decl.js';

vi.mock('../src/core/bake.js', () => ({
  bakeCanvas: () => ({}),
  textureFromBaked: () => new Texture({ source: new TextureSource({ width: 2048, height: 2048 }) }),
}));
afterEach(() => { vi.restoreAllMocks(); });

describe('场景坐标与状态边界', () => {
  it.each([100, 64])('lighting 从左上边缘到右下边缘都保持世界坐标（cell=%s）', (cell) => {
    const view = cell * 50;
    const pass = new LightingPass(view, cell);
    pass.content.scale.set(2, 3);
    pass.content.position.set(17, 29);
    const points = [-cell / 2, 0, view / 2, view - cell / 2];
    let transform = new Matrix();
    pass.update({ render: (options) => { transform = options.transform!.clone(); } }, 2048, view);
    for (const coordinate of points) {
      expect(pass.display.x + (transform.a * coordinate + transform.tx) * view / 2048).toBeCloseTo(coordinate);
      expect(pass.display.y + (transform.d * coordinate + transform.ty) * view / 2048).toBeCloseTo(coordinate);
    }
    expect(transform.a * points[0]! + transform.tx).toBeCloseTo(0);
    expect(transform.a * points[3]! + transform.tx).toBeCloseTo(2048);
    expect([pass.content.scale.x, pass.content.scale.y, pass.content.x, pass.content.y]).toEqual([2, 3, 17, 29]);
    pass.destroy();
  });

  it('视口 resize 同步绘制与拾取，并重新钳边而不改变缩放', () => {
    const stage = new Container();
    let width = 800;
    const camera = new CameraController(stage, () => ({ width, height: 800 }), {
      bounds: { minX: -50, minY: -50, maxX: 4950, maxY: 4950 },
    });
    camera.setState({ x: 400, y: 1000, zoom: 1 });
    width = 1200;
    camera.syncViewport();
    expect(camera.state.x).toBe(550);
    expect(camera.zoom).toBe(1);
    const sx = stage.x + camera.state.x * stage.scale.x;
    expect(sx).toBe(600);
    expect(screenToWorld(camera.state, width, 800, sx, 400).x).toBe(camera.state.x);
  });

  it('相同空地形不反复 dirty，墙移除只清一次', () => {
    const lighting = { clearMembers: vi.fn(), addMember: vi.fn() } as unknown as LightingPass;
    const textures = Object.fromEntries(['ground', 'ground-mask', 'noise1', 'noise2',
      'exit-top', 'exit-bottom', 'exit-left', 'exit-right'].map((key) => [key, { texture: Texture.WHITE }])) as unknown as TerrainTextures;
    const stage = new TerrainStage(textures, lighting, 5000, 100);
    const empty = new Uint8Array(2500);
    expect(stage.setTerrain(empty).wallRebuilt).toBe(true);
    expect(stage.setTerrain(empty.slice()).wallRebuilt).toBe(false);
    const walls = empty.slice(); walls[510] = 1;
    expect(stage.setTerrain(walls).wallRebuilt).toBe(true);
    expect(stage.setTerrain(empty).wallRebuilt).toBe(true);
    expect(stage.setTerrain(empty.slice()).wallRebuilt).toBe(false);
    expect(lighting.clearMembers).toHaveBeenCalledTimes(3);
    stage.destroy();
  });

  it('link 能量尺寸独立于 atlas 原图尺寸，空满转换不累积比例', () => {
    const texture = new Texture({ source: new TextureSource({ width: 128, height: 128 }) });
    const join = joinLink({ frames: new Map([['link-energy', texture]]), glow: null, creepMask: null }).energy!;
    const env = {} as PartEnv;
    const node = join.create(env);
    for (const scale of [1, 0.5, 0, 1]) {
      join.apply({ scale }, node, env);
      expect(node.width).toBeCloseTo(50 * scale);
      expect(node.height).toBeCloseTo(50 * scale);
    }
    node.destroy(); texture.destroy(true);
  });
});
