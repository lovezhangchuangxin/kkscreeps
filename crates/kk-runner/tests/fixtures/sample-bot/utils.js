// 单向依赖：官方语义下模块图不允许顶层循环（真循环首 require 即抛
// Circular reference——行为锁值见 cycle_a/cycle_b 与 cjs_and_loop 测试）
module.exports.identity = function (x) { return x; };
