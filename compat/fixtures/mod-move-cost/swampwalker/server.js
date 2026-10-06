// after 层沼泽步行：把无路沼泽格的移动代价压到平原当量（weight×2）。
// 策略不改平原与有路格的原生代价；getFatigue 缺省读原生值。
export default function register(api) {
  api.handlers.after('builtin:room.moveCost', { id: 'swampwalk' }, (ctx) => {
    for (const m of ctx.args.movers) {
      if (m.terrainSwamp && !m.hasRoad && ctx.cost.getFatigue(m.id) !== m.weight * 2) {
        ctx.cost.setFatigue(m.id, m.weight * 2);
      }
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
