// 独立双信封对象包：objhits-vault:cache 同时声明 hitsMax 200 与
// capacity 100（耐久×库存并存，键集不相交）。对象 tick 把两类执行期
// 读值记入 namespace state，核对伤害动作不动库存、资源动作不动耐久。
export default function register(api) {
  api.objects.define('objhits-vault:cache', {
    version: 1,
    playerClass: 'VaultCache',
    positionFields: ['x', 'y'],
    capacity: 100,
    hitsMax: 200,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
    tick(ctx) {
      ctx.state = { hits: ctx.target.fields.hits, energy: ctx.target.fields.store.energy };
    },
  });
}
