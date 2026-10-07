/**
 * 渲染后端标识（普通数据；零 pixi 导入——公共 d.ts 闭包守卫要求，评审 B-B3：
 * Backend 原住 core/renderer.ts（pixi 模块），经 index 转手会把 pixi 类型拉进
 * 公共类型闭包）。
 */
export type Backend = 'webgpu' | 'webgl';
