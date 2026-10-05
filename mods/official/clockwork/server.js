// 官方生命周期样例使用公共 SDK；没有依赖包 ID 的宿主分支。
export default function register(api) {
  const config = api.config ?? {};
  if (typeof config !== 'object' || Array.isArray(config) || Object.keys(config).some(key => key !== 'step')) {
    throw new Error('clockwork config must be an object with optional step');
  }
  const step = config.step ?? 1;
  if (!Number.isInteger(step) || step < 1 || step > 100) {
    throw new Error('clockwork step must be an integer from 1 to 100');
  }
  api.objects.define('clockwork:counter', {
    version: 1,
    playerClass: 'ClockworkCounter',
    visibility: 'room-visible',
    positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'u32', min: 0, max: 1000000, default: 0 },
    },
    tick(ctx) {
      ctx.target.fields.value = (ctx.target.fields.value + step) % 1000001;
      ctx.state = { visits: (ctx.state?.visits ?? 0) + 1 };
    },
  });
}
