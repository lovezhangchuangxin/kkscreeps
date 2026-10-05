export default function register(api) {
  api.handlers.replace('builtin:creep.attack', { id: 'policy' }, (ctx) => {
    const amount = Math.ceil(ctx.target.fields.body.filter(p => p.type === 'attack' && p.hits > 0).length * 30 / 5);
    ctx.combat.setRetaliation([{ id: ctx.args.id, amount }]);
    ctx.combat.request(ctx.args.id);
    ctx.state = { applied: (ctx.state?.applied || 0) + 1, amount };
  });
}
