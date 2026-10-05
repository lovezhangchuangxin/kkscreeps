// 固定伤害玩法；引擎不识别此包 ID，也不追加默认部件功率或反击。
export default function register(api) {
  api.handlers.replace('builtin:creep.attack', { id: 'formula' }, (ctx) => {
    const target = ctx.combat.get(ctx.args.id);
    if (!target || target.hits <= 0 || target.spawning) return api.decline('unavailable');
    const damage = 45;
    ctx.combat.applyDamage(ctx.args.id, damage);
    ctx.state = { applied: (ctx.state?.applied || 0) + 1, lastDamage: damage };
  });
}
