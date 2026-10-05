// 组合验收的独立对象中心：不按包 ID 进入专用宿主通路。
export default function register(api) {
  api.objects.define('base:counter', {
    version: 1, playerClass: 'CompositionCounter', visibility: 'room-visible', positionFields: ['x', 'y'],
    fields: { x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 }, value: { type: 'u32', default: 0 }, trace: { type: 'string', maxBytes: 64, default: '' } },
    tick(ctx) {
      ctx.target.fields.value++;
      ctx.target.fields.trace += 'C';
      ctx.state = { visits: (ctx.state?.visits ?? 0) + 1 };
    },
  });
}
