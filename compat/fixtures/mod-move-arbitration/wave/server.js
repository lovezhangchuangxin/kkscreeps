// replace 层"车队发车"策略：准入候选先经链预演，发现任何已获准
// 移动会被取消链级联吞掉时，本轮全房停下（宁可全停，不冒半队
// 出发的风险）。evaluate 返回按候选当前终值存活的胜者集合；宿主
// 用与正式仲裁完全相同的取消链计算，JS 无需自己推导让位级联。
export default function register(api) {
  api.handlers.replace('builtin:creep.moveAdmission', { id: 'wave' }, (ctx) => {
    const admitted = ctx.args.movers.filter((m) => ctx.admission.getAdmitted(m.id));
    const survivors = ctx.admission.evaluate();
    if (survivors.length < admitted.length) {
      for (const mover of admitted) ctx.admission.setAdmitted(mover.id, false);
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
