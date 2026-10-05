export default function register(api) {
  api.handlers.replace('builtin:creep.rangedMassAttack', { id: 'formula' }, (ctx) => {
    const requests = [];
    let after = '';
    do {
      const page = ctx.combat.list(after, 2);
      for (const target of page.objects) {
        const distance = Math.max(Math.abs(target.x - ctx.target.fields.x), Math.abs(target.y - ctx.target.fields.y));
        if (target.owner && target.owner !== ctx.target.fields.owner && !target.spawning && target.hits > 0 && distance <= 6) {
          requests.push({ id: target.id, amount: 23 });
        }
      }
      after = page.next;
    } while (after !== null);
    ctx.combat.applyDamageBatch(requests);
    ctx.state = { applied: (ctx.state?.applied || 0) + 1, targets: requests.length };
  });
}
