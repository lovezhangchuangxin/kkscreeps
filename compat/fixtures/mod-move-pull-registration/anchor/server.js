// replace 层全量禁拖：房内所有拉拽配对一律否决。被否决配对在预检
// 输入构造之前双向移除，等同从未登记——被拉者回到自身门槛（无 MOVE
// 部件即不能移动），拉者自身移动不受影响。
export default function register(api) {
  api.handlers.replace('builtin:room.movePullRegistration', { id: 'anchor' }, (ctx) => {
    for (const p of ctx.args.pulls) {
      if (!ctx.pull.isDenied(p.pullerId)) ctx.pull.deny(p.pullerId);
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
