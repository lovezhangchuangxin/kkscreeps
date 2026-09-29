// 官方循环依赖锁值对（game.js:565/:590-592）：cycle_a → cycle_b → cycle_a，
// 第二次命中占坑 -1 → 抛 Circular reference to module 'cycle_a'
const b = require('cycle_b');
module.exports.a = 1;
module.exports.bSeen = typeof b.b;
