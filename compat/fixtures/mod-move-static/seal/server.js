// ghost 放行静态玩法障碍；seal 保留独立区域否决，组合时不能被放行清除。
export default function register(api) {
  api.handlers.after('builtin:room.movePrecheck', { id: 'seal' }, (ctx) => {
    for (const r of ctx.args.requests) {
      if (r.targetX >= 30) ctx.precheck.block(r.moverId);
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
