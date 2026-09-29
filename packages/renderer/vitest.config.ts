import { defineConfig } from 'vitest/config';

// 单测只覆盖纯逻辑（资产清单/atlas 几何/相机数学），不加载 Pixi 运行时——
// 渲染验证走 dev/ harness 截图（方案 §7 R0 出口）。environment 用 node 保持封闭。
export default defineConfig({
  test: {
    environment: 'node',
    include: ['test/**/*.test.ts'],
  },
});
