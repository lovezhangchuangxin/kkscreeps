// G22.b：玩法 realm 的确定性时间与随机全局。本文件随 SDK 模块求值，
// 先于任何包代码装载，Mod 代码从一开始看到的 Date/Math.random 就是
// 确定性版本；原生实现不再暴露（设计承诺：不能只新增确定性入口却保留
// 真实时间/系统随机供规则读取）。协议契约见
// docs/architecture/gameplay-determinism.md。
//
// 玩家沙箱不受本文件影响：玩家代码的 Date/Math.random 保持生产行为，
// 属玩家输入源非确定边界（玩家结果作为已执行结果记录，不参与玩法回放
// 的重放）。注册期（模块装载）不在玩法时间轴上：Math.random 走该命名
// 空间的 init 流（不含 tick，冷恢复重装同值），Date 恒为 0。
const ops = Deno.core.ops;
const TICK_MILLIS = 3000;
let currentScope = null; // { domain, tick }

/// 在绑定作用域内同步执行 fn；嵌套调用保存并恢复外层作用域（wrap 链的
/// 内层处理器在内层作用域抽值，退栈后外层继续用自己的域）。
export function withStreamScope(scope, fn) {
  const prev = currentScope;
  currentScope = scope;
  try {
    return fn();
  } finally {
    currentScope = prev;
  }
}

/// 由绑定键与链 ctx 拼装流域：`绑定键#房间#目标ID`。房间级槽的目标是
/// 固定哨兵 ID，等效于按房分流；对象级槽按对象 ID 分流——无关绑定、
/// 房间与对象互不挤占序列。
export function streamDomain(bindingKey, ctx) {
  const room = ctx?.room ?? '';
  const target = ctx?.target?.id ?? '';
  return `${bindingKey}#${room}#${target}`;
}

const realDate = Date;
function deterministicNow() {
  // 结算期 = 当前绑定作用域的 tick×3000；注册期（装载）恒 0，与装载
  // 时刻无关（重装不变式）。
  return (currentScope?.tick ?? 0) * TICK_MILLIS;
}
// 可调用形态与构造形态都收敛到确定性时钟；显式参数构造、parse/UTC 等
// 纯函数保持原语义。
function KkDate(...args) {
  if (!new.target) return new realDate(deterministicNow()).toString();
  return Reflect.construct(realDate, args.length ? args : [deterministicNow()], new.target);
}
// 实例方法经原型链继承，但原生构造器不得可经实例原型取回（其 now()
// 是墙钟）：遮蔽 constructor 指回确定性包装。
Object.setPrototypeOf(KkDate.prototype, realDate.prototype);
Object.defineProperty(realDate.prototype, 'constructor', {
  value: KkDate, writable: true, configurable: true, enumerable: false,
});
// 静态面不做原型继承——`Object.getPrototypeOf(Date)` 会直接拿到原生
// Date。需要的静态成员以不可枚举自有属性提供；parse/UTC 是纯函数，
// 引用本身不暴露原生构造器。
for (const [name, value] of [['now', deterministicNow], ['parse', realDate.parse], ['UTC', realDate.UTC]]) {
  Object.defineProperty(KkDate, name, { value, writable: true, configurable: true, enumerable: false });
}
Object.defineProperty(KkDate, 'name', { value: 'Date', configurable: true });
Object.defineProperty(KkDate, 'length', { value: 7, configurable: true });
Object.defineProperty(globalThis, 'Date', { value: KkDate, writable: false, configurable: false });

// Temporal 的 Now 族是墙钟面，玩法 realm 不提供；其确定性部分待出现
// 真实消费者再评估（架构文档登记为边界）。
delete globalThis.Temporal;

Math.random = function () {
  if (currentScope === null) return ops.op_mod_random_init();
  return ops.op_mod_random(currentScope.domain, currentScope.tick);
};
