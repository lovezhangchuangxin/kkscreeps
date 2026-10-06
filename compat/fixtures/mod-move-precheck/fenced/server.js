// replace 层区域管制：西区（targetX<10）一律禁止进入。被 block 的
// 请求在竞争输入构造之前移除，等同从未登记——不参与竞争评分、
// 不占格、不让位。
export default function register(api) {
  api.handlers.replace('builtin:room.movePrecheck', { id: 'fenced' }, (ctx) => {
    for (const r of ctx.args.requests) {
      if (r.targetX < 10) ctx.precheck.block(r.moverId);
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
