// 独立三声明组合包：objret-reactor:core 同时声明耐久 200、反击比率
// 1.0（全额反弹）与容量 100——耐久×反击×库存并存；反击不动库存、
// 资源动作不动耐久与反击声明。
export default function register(api) {
  api.objects.define('objret-reactor:core', {
    version: 1,
    playerClass: 'ReactorCore',
    positionFields: ['x', 'y'],
    hitsMax: 200,
    retaliationRatio: 1.0,
    capacity: 100,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
    tick(ctx) {
      ctx.state = { hits: ctx.target.fields.hits, energy: ctx.target.fields.store.energy };
    },
  });
}
