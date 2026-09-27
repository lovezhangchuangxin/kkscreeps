// 与 main 形成加载链；对 utils 保持循环引用（utils → role_harvester → utils）
const role = require('role_harvester');

module.exports.identity = function (x) { return x; };
module.exports.roleSeenDuringCycle = role.role; // 循环点读半成品属性 → 应为 undefined
// 循环点持有的半成品对象引用：模块完成后 role_harvester.role 的后续赋值应对其可见
// （若加载器在循环点返回新空对象，lateProbe 将不是同一引用、lateProbe.role 为 undefined）
module.exports.lateProbe = role;
