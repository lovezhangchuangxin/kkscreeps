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
// UTC 是玩法 Date 唯一的本地时区；不依赖宿主 TZ。非标准日期文本拒绝，
// 避免原生宽松解析把没有时区的输入解释为机器本地时间。
function parseDate(value) {
  const text = String(value);
  if (
    /^[+-]?\d{4,6}-\d{2}-\d{2}(?:T\d{2}:\d{2}(?::\d{2}(?:\.\d{1,3})?)?(?:Z|[+-]\d{2}:\d{2})?)?$/.test(
      text,
    )
  ) {
    return realDate.parse(
      text.includes('T') && !/(Z|[+-]\d{2}:\d{2})$/.test(text) ? `${text}Z` : text,
    );
  }
  // toUTCString 的输出有显式 GMT，解析不读取本地时区。
  if (
    /^(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun), \d{2} [A-Z][a-z]{2} [+-]?\d{4,6} \d{2}:\d{2}:\d{2} GMT$/.test(
      text,
    )
  )
    return realDate.parse(text);
  return NaN;
}
function dateArgument(value) {
  if (value !== null && (typeof value === 'object' || typeof value === 'function')) {
    try {
      return realDate.prototype.getTime.call(value);
    } catch {}
    const convert = value[Symbol.toPrimitive];
    if (convert !== undefined) value = convert.call(value, 'default');
    else {
      const object = value;
      value = typeof object.valueOf === 'function' ? object.valueOf() : object;
      if (value !== null && (typeof value === 'object' || typeof value === 'function'))
        value = object.toString();
    }
    if (value !== null && (typeof value === 'object' || typeof value === 'function'))
      throw new TypeError('Cannot convert date argument');
  }
  return typeof value === 'string' ? parseDate(value) : value;
}
function KkDate(...args) {
  if (!new.target) return new realDate(deterministicNow()).toUTCString();
  const time =
    args.length === 0
      ? deterministicNow()
      : args.length === 1
        ? dateArgument(args[0])
        : realDate.UTC(...args);
  return Reflect.construct(realDate, [time], new.target);
}
Object.setPrototypeOf(KkDate.prototype, realDate.prototype);
Object.defineProperty(realDate.prototype, 'constructor', {
  value: KkDate,
  writable: true,
  configurable: true,
  enumerable: false,
});
for (const stem of [
  'FullYear',
  'Month',
  'Date',
  'Day',
  'Hours',
  'Minutes',
  'Seconds',
  'Milliseconds',
]) {
  realDate.prototype[`get${stem}`] = realDate.prototype[`getUTC${stem}`];
  if (stem !== 'Day') realDate.prototype[`set${stem}`] = realDate.prototype[`setUTC${stem}`];
}
realDate.prototype.getTimezoneOffset = function () {
  return Number.isNaN(this.getTime()) ? NaN : 0;
};
realDate.prototype.getYear = function () {
  return this.getUTCFullYear() - 1900;
};
realDate.prototype.setYear = function (year) {
  year = Number(year);
  year = Number.isNaN(year) ? year : Math.trunc(year);
  if (year >= 0 && year <= 99) year += 1900;
  if (Number.isNaN(this.getTime())) this.setTime(0);
  return this.setUTCFullYear(year);
};
realDate.prototype.toString = realDate.prototype.toUTCString;
realDate.prototype.toDateString = function () {
  const text = this.toUTCString();
  return text === 'Invalid Date' ? text : text.split(' ').slice(0, 4).join(' ');
};
realDate.prototype.toTimeString = function () {
  const text = this.toUTCString();
  return text === 'Invalid Date' ? text : text.split(' ').slice(4).join(' ');
};
for (const name of ['toLocaleString', 'toLocaleDateString', 'toLocaleTimeString']) {
  const native = realDate.prototype[name];
  realDate.prototype[name] = function (locales, options) {
    return native.call(this, locales ?? 'en-US', { ...options, timeZone: 'UTC' });
  };
}
for (const [name, value] of [
  ['now', deterministicNow],
  ['parse', parseDate],
  ['UTC', realDate.UTC],
]) {
  Object.defineProperty(KkDate, name, {
    value,
    writable: true,
    configurable: true,
    enumerable: false,
  });
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
