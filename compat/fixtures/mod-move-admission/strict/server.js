// after 层收紧：功率配重不足（movePower<weight）的执行者拒绝放行，
// 即便原生（疲劳 0 且有存活 MOVE）或更早处理器已放行。after 总能把
// 准入收回到自己的门槛之上。
export default function register(api) {
  api.handlers.after('builtin:creep.moveAdmission', { id: 'strict' }, (ctx) => {
    for (const mover of ctx.args.movers) {
      if (mover.movePower < mover.weight) {
        ctx.admission.setAdmitted(mover.id, false);
      }
    }
    ctx.state = { applied: (ctx.state?.applied || 0) + 1 };
  });
}
