// G22.b 确定性协议验收包：模块顶层捕获 init 流抽值与装载期 Date.now()
// （协议规定恒 0）；每 tick 抽两次、记录 Date.now()/new Date() 终值。
// 全部产物写 ctx.state，跨进程/冷恢复/重试等值即协议成立。
// 不记录 Date() 字符串全文——其文本含进程本地时区，属机器相关面。
const initDraw = Math.random();
const initNow = Date.now();
const initNewDate = new Date().getTime();
const temporalGone = typeof Temporal === 'undefined';
const dateCallable = typeof Date();

export default function register(api) {
  api.objects.define('dice:roller', {
    version: 1,
    playerClass: 'DiceRoller',
    visibility: 'server-only',
    positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
    tick(ctx) {
      const s = ctx.state ?? {};
      s.init = { draw: initDraw, now: initNow, newDate: initNewDate, temporalGone, dateCallable };
      s.draws = [...(s.draws ?? []), Math.random(), Math.random()];
      s.now = Date.now();
      s.newDate = new Date().getTime();
      ctx.state = s;
    },
  });
}
