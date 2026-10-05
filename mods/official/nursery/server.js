// 公共对象能力的官方原型：发射器周期生成火花，火花自行更新并到期删除。
export default function register(api) {
  const config = api.config ?? {};
  if (typeof config !== 'object' || Array.isArray(config) || Object.keys(config).some(key => !['period', 'life'].includes(key))) {
    throw new Error('nursery config accepts only period and life');
  }
  const period = config.period ?? 5;
  const life = config.life ?? 3;
  if (!Number.isInteger(period) || period < 2 || period > 100 || !Number.isInteger(life) || life < 1 || life > 100) {
    throw new Error('nursery period must be 2..100 and life must be 1..100');
  }
  const position = { x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 } };
  api.objects.define('nursery:emitter', {
    version: 1, playerClass: 'NurseryEmitter', visibility: 'room-visible', positionFields: ['x', 'y'],
    fields: { ...position, emitted: { type: 'u32', default: 0 } },
    tick(ctx) {
      if (ctx.tick % period === 0) {
        ctx.objects.create('nursery:spark', { x: Math.min(49, ctx.target.fields.x + 1), y: ctx.target.fields.y, life });
        ctx.target.fields.emitted++;
        ctx.state = { ...ctx.state, born: (ctx.state?.born ?? 0) + 1 };
      }
    },
  });
  api.objects.define('nursery:spark', {
    version: 1, playerClass: 'NurserySpark', visibility: 'room-visible', positionFields: ['x', 'y'],
    fields: { ...position, age: { type: 'u32', default: 0 }, life: { type: 'u32', min: 1, max: 100 } },
    tick(ctx) {
      const age = ctx.target.fields.age + 1;
      const life = ctx.target.fields.life;
      ctx.objects.update(ctx.target.id, { age });
      if (age >= life) {
        const id = ctx.target.id;
        ctx.objects.remove(id);
        if (ctx.objects.get(id) !== null) throw new Error('deleted spark remains in candidate');
        ctx.state = { ...ctx.state, expired: (ctx.state?.expired ?? 0) + 1 };
      }
    },
  });
}
