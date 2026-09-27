const utils = require('utils');

module.exports.role = 'harvester';
module.exports.sawEmptyAtLoad = false;
module.exports.usesLodash = function (arr) {
  return _.map(arr, (x) => x * 2);
};

// 循环依赖：加载时 utils 尚未完成初始化（缓存半成品语义）
module.exports.utilsLoadedAtInit = typeof utils.identity === 'function' ? false : true;
