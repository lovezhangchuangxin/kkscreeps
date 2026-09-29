const utils = require('utils');

module.exports.role = 'harvester';
module.exports.utilsOk = typeof utils.identity === 'function';
module.exports.usesLodash = function (arr) {
  return _.map(arr, (x) => x * 2);
};
