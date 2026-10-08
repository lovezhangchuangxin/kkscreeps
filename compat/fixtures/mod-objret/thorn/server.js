// 独立反击对象包：objret-thorn:spike 声明耐久 300 + 反击比率 0.5
// （官方 powerBank damage×0.5 同构：任意攻击类型触发、不耗自身耐久）。
// 对象 tick 把执行期耐久读值记入 namespace state。
export default function register(api) {
  api.objects.define('objret-thorn:spike', {
    version: 1,
    playerClass: 'ThornSpike',
    positionFields: ['x', 'y'],
    hitsMax: 300,
    retaliationRatio: 0.5,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
    tick(ctx) {
      ctx.state = { hits: ctx.target.fields.hits };
    },
  });
}
