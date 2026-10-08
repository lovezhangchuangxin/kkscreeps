// 完整替换默认受击公式：每个活跃 TOUGH 提供两层护甲，不再叠加默认强化倍率。
export default function register(api) {
  api.handlers.replace('builtin:creep.receiveDamage', { id: 'armor' }, (ctx) => {
    const plating = ctx.target.fields.body.filter(
      (part) => part.type === 'tough' && part.hits > 0,
    ).length;
    const damage = Math.ceil(ctx.args.rawDamage / (1 + 2 * plating));
    ctx.incoming.setDamage(damage);
    ctx.state = { applied: (ctx.state?.applied || 0) + 1, lastDamage: damage };
  });
}
