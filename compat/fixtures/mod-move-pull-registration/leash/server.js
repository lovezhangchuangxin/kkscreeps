// after 层反绑架：只允许同主人配对，跨主人拉拽一律否决。被否决配对
// 等同从未登记；同主人配对维持原生拉拽语义。
export default function register(api) {
  api.handlers.after('builtin:room.movePullRegistration', { id: 'leash' }, (ctx) => {
    for (const p of ctx.args.pulls) {
      if (p.pullerOwner !== p.targetOwner && !ctx.pull.isDenied(p.pullerId)) {
        ctx.pull.deny(p.pullerId);
      }
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
