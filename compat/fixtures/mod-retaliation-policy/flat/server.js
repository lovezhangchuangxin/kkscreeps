export default function register(api) {
  api.handlers.replace('builtin:creep.attack', { id: 'policy' }, (ctx) => {
    const amount = 7;
    ctx.combat.setRetaliation([{ id: ctx.args.id, amount }]);
    ctx.combat.request(ctx.args.id);
    ctx.state = { applied: (ctx.state?.applied || 0) + 1, amount };
  });
}
