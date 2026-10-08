// G22.b 确定性协议验收包：模块顶层捕获 init 流抽值与装载期 Date.now()
// （协议规定恒 0）；每 tick 抽两次、记录 Date.now()/new Date() 终值。
// 全部产物写 ctx.state，跨进程/冷恢复/重试等值即协议成立。
// UTC Date 文本、本地方法与无时区解析都记录，跨进程 TZ 不改变玩法结果。
const CapturedDate = Date;
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
      const local = new CapturedDate(1970, 0, 1, 8);
      local.setHours(9);
      // 函数也是可转换对象，不能把其无时区字符串交回原生 Date 构造。
      const dateFunction = () => {};
      dateFunction[Symbol.toPrimitive] = () => '1970-01-01T00:00:00';
      let rejectedObject = false;
      try {
        new Date({ [Symbol.toPrimitive]: () => dateFunction });
      } catch (error) {
        rejectedObject = error instanceof TypeError;
      }
      s.date = {
        hour: new Date().getHours(),
        offset: new Date().getTimezoneOffset(),
        parsed: Date.parse('1970-01-01T00:00:00'),
        constructed: new CapturedDate(1970, 0, 1).getTime(),
        coercedFunction: new Date(dateFunction).getTime(),
        rejectedObject,
        set: local.getTime(),
        text: Date(),
        localText: new Date().toLocaleString(),
        roundtrip: Date.parse(new Date(0).toString()),
      };
      ctx.state = s;
    },
  });
}
