// replace 层沼泽化：一切移动代价翻倍（封顶 u32::MAX）。平原累积疲劳
// 使移动者隔 tick 前进，沼泽立即停走。
export default function register(api) {
  api.handlers.replace('builtin:room.moveCost', { id: 'bog' }, (ctx) => {
    for (const m of ctx.args.movers) {
      ctx.cost.setFatigue(m.id, Math.min(ctx.cost.getFatigue(m.id) * 2, 4294967295));
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
