// 独立减伤型包：objguard-warden:aegis 声明耐久 200（创建即满血）；包以
// Replace 覆写 builtin:mod.receiveDamage 把当次伤害减半——介入点在
// 结构扣减前，输出修改后的伤害量，正常拒绝回退默认伤害。
export default function register(api) {
  api.objects.define('objguard-warden:aegis', {
    version: 1,
    playerClass: 'WardenAegis',
    positionFields: ['x', 'y'],
    hitsMax: 200,
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
  });
  api.objects.define('objguard-warden:decoy', {
    version: 1,
    playerClass: 'WardenDecoy',
    positionFields: ['x', 'y'],
    fields: {
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
    },
  });
  api.handlers.replace('builtin:mod.receiveDamage', { id: 'halve' }, ctx => {
    ctx.incoming.setDamage(Math.ceil(ctx.args.damage / 2));
    ctx.state = { calls: (ctx.state?.calls || 0) + 1, lastDamage: ctx.args.damage };
  });
}
