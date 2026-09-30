import { defineConfig } from 'vite';

// 库构建（R6 D7 冻结形态）：ESM 单入口，pixi/shared 外置（宿主应用自带，
// 复用 workspace shared）。d.ts 由 tsc emitDeclarationOnly 另行产出
// （scripts/build-lib.mjs 编排：先 bundle 后 d.ts——emptyOutDir 只在 bundle 步）。
export default defineConfig({
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    minify: false,
    sourcemap: false,
    lib: {
      entry: 'src/index.ts',
      formats: ['es'],
      fileName: () => 'index.js',
    },
    rollupOptions: {
      external: ['pixi.js', '@kkscreeps/shared'],
    },
  },
});
