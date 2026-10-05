export default function register(api) {
  for (const action of ['heal', 'rangedHeal']) {
    api.handlers.replace(`builtin:creep.${action}`, { id: action }, (ctx) => {
      const target = ctx.combat.get(ctx.args.id);
      if (!target?.body || target.hits <= 0 || target.spawning) return api.decline('unavailable');
      ctx.combat.applyHeal(target.id, 23);
      ctx.state = { applied: (ctx.state?.applied || 0) + 1, lastHeal: 23 };
    });
  }
}
