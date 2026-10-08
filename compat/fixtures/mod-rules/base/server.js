// 有效规则读取的独立对象中心：节奏与增益每轮取自冻结有效规则，
// 不把注册闭包里的声明默认值缓存进玩法。
export default function register(api) {
  api.rules.define('base:period', {
    type: 'i32',
    min: 1,
    max: 100,
    default: 4,
    consumers: ['base:meter.tick.process'],
  });
  api.rules.define('base:rate', {
    type: 'f64',
    min: 0,
    max: 1,
    default: 0.5,
    consumers: ['base:meter.tick.process'],
  });
  api.objects.define('base:meter', {
    version: 1,
    playerClass: 'RulesMeter',
    visibility: 'room-visible',
    positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'u32', default: 0 },
      pool: { type: 'f64', min: 0, max: 1e9, default: 0 },
    },
    tick(ctx) {
      const period = api.rules.effective('base:period');
      if (ctx.tick % period === 0) ctx.target.fields.value += 1;
      const rate = api.rules.effective('base:rate');
      ctx.target.fields.pool += rate;
      ctx.state = {
        period,
        rate,
        capacity: api.rules.effective('builtin:link.capacity'),
        ticks: (ctx.state?.ticks ?? 0) + 1,
      };
    },
  });
}
