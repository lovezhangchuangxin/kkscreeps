// 每个目标有独立的 18 点护盾；可叠加默认 TOUGH 或其他 Mod 的受击公式。
export default function register(api) {
  api.handlers.after('builtin:creep.receiveDamage', { id: 'shield' }, (ctx) => {
    const id = ctx.target.id;
    const pool = ctx.state?.pools?.[id] ?? 18;
    const absorbed = Math.min(pool, ctx.incoming.getDamage());
    const damage = ctx.incoming.getDamage() - absorbed;
    ctx.incoming.setDamage(damage);
    ctx.state = {
      pools: { ...ctx.state?.pools, [id]: pool - absorbed },
      applied: (ctx.state?.applied || 0) + 1,
      lastDamage: damage,
      lastAbsorbed: absorbed,
    };
  });
}
