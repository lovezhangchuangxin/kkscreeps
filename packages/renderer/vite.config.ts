import { defineConfig } from 'vite';

// dev harness（方案 §2.1：回放验收页，不进库构建）。
// 资产经 publicDir 以 / 提供（库代码自身以 atlasBaseUrl 参数化，不耦合 Vite）。
export default defineConfig({
  root: 'dev',
  publicDir: '../assets',
  server: { port: 5173 },
});
