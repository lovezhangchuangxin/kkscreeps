// 生命值比例伤害玩法：只改独立 JS 包就能换公式，无第二条 Rust 分派。
export default function register(api) {
  api.handlers.replace('builtin:creep.attack', { id: 'formula' }, (ctx) => {
    const target = ctx.combat.get(ctx.args.id);
    if (!target || target.hits <= 0 || target.spawning) return api.decline('unavailable');
    const armor = target.body?.filter((part) => part.type === 'tough' && part.hits > 0).length || 0;
    const damage = Math.ceil(ctx.target.fields.hits * 0.3 / (1 + armor));
    ctx.combat.applyDamage(ctx.args.id, damage);
    ctx.state = { applied: (ctx.state?.applied || 0) + 1, lastDamage: damage };
  });
}
