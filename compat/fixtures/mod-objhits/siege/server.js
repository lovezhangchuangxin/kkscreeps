// 独立耐久对象包：objhits-siege:barrel 声明 hitsMax 300（创建即满血，
// 携带 hits/hitsMax 信封，耐久只能经伤害动作变动）；objhits-siege:decoy
// 不声明耐久，作为「无信封不可攻击」对照。对象 tick 经 ctx.target.fields
// 读同一 payload（含耐久信封），把执行期读值记入 namespace state。
export default function register(api) {
  api.objects.define('objhits-siege:barrel', {
    version: 1,
    playerClass: 'SiegeBarrel',
    positionFields: ['x', 'y'],
    hitsMax: 300,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
    tick(ctx) {
      ctx.state = { hits: ctx.target.fields.hits };
    },
  });
  api.objects.define('objhits-siege:decoy', {
    version: 1,
    playerClass: 'SiegeDecoy',
    positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
  });
}
