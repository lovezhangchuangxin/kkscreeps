// @kkscreeps/mod 公共入口；内部模块只能由 SDK 静态装载。
// determinism.js 的副作用先行：确定性 Date/Math.random 在任何包代码
// 装载前就位（G22.b 协议）。
import './determinism.js';
export { createApi, seal, decline } from './registration.js';
export { installRules } from './rules.js';
export { dispatch, installChains, runChain } from './chains.js';
