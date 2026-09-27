// 样例 bot：多模块 CommonJS + 循环依赖 + 子目录 + lodash 使用。
// 供 kk-runner 集成测试与 bot-check 冒烟使用。

module.exports.loop = function () {
  const harvester = require('role_harvester');
  const utils = require('utils');
  const sub = require('sub/mod');
  globalThis.__tickCount = (globalThis.__tickCount || 0) + 1;
  globalThis.__roles = { harvester: harvester.role, sub: sub.name, utils: utils.identity(1) };
};
