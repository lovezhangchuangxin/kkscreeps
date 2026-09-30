/**
 * 渲染内核：Pixi Application 生命周期、后端探测、stats 采集。
 * Pixi 对象只在本包内部存在——公共面见 index.ts（ADR-009 第 4 条）。
 */
import { Application, Container } from 'pixi.js';

export type Backend = 'webgpu' | 'webgl';

export interface KernelOptions {
  container: HTMLElement;
  backgroundColor: number;
  preference: 'webgpu' | 'webgl';
}

export interface Kernel {
  app: Application;
  /** 场景挂载点（世界坐标系根）。 */
  stage: Container;
  backend: Backend;
  destroy(): Promise<void>;
}

function detectBackend(app: Application): Backend {
  const name = (app.renderer as { name?: string }).name;
  if (name === 'webgpu' || name === 'webgl') return name;
  // 兜底：v8 renderer.type 1=WebGL 2=WebGPU
  return app.renderer.type === 2 ? 'webgpu' : 'webgl';
}

export async function initKernel(options: KernelOptions): Promise<Kernel> {
  const app = new Application();
  await app.init({
    background: options.backgroundColor,
    resizeTo: options.container,
    preference: options.preference, // webgpu 不可用时 pixi 自动回退 WebGL
    antialias: true,
    resolution: Math.min(window.devicePixelRatio || 1, 2),
    autoDensity: true,
  });
  options.container.appendChild(app.canvas);
  // Pixi ResizePlugin 只监听 window.resize；侧栏/父布局变化由容器观察补齐。
  const observer = new ResizeObserver(() => app.resize());
  observer.observe(options.container);

  const stage = app.stage;
  let destroyed = false;
  const kernel: Kernel = {
    app,
    stage,
    backend: detectBackend(app),
    async destroy() {
      if (destroyed) return;
      destroyed = true;
      observer.disconnect();
      // children 级联拆场景子树；texture 缺省 false——Assets 共享纹理不在此销毁
      // （v8 语义）；Graphics GPU 缓冲由 renderer 拆卸时经 graphicsContextSystem 释放。
      app.destroy(true, { children: true });
    },
  };
  return kernel;
}
