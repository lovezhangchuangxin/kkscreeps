// 与 main 形成加载链；对 utils 保持循环引用（utils → role_harvester → utils）
const role = require('role_harvester');

module.exports.identity = function (x) { return x; };
module.exports.roleSeenDuringCycle = role.role; // role_harvester 此刻可能已缓存（取决于加载序）
